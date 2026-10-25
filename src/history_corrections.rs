//! Correcciones del historico de tickers donde Yahoo trae datos malos.
//!
//! Se aplican cada vez que se escribe un historico (job de refresco y
//! `/historical-data`), porque `update_historical_data` reemplaza la serie
//! entera: corregir una sola vez a mano se pierde en el proximo refresco.
//! Para corregir un ticker nuevo, agregarlo al `match` de `apply_corrections_with`.
//! El relevamiento de cada caso esta en el analisis exploratorio de api-ml.

use std::collections::BTreeMap;

use chrono::{FixedOffset, NaiveDate, TimeZone, Utc};
use rust_decimal::{
    Decimal,
    prelude::{FromPrimitive, ToPrimitive},
};
use serde::Deserialize;

use crate::{
    persistence::ticker_repository::TickerHistoricalData,
    site_scrappers::rava_scrapper::rava_structures_responses::{PriceData, RavaHistoricalResponse},
};

const BYMA_HISTORY_URL: &str = "https://open.bymadata.com.ar/vanoms-be-core/rest/api/bymadata/free/chart/historical-series/history";
const RAVA_HISTORY_URL: &str = "https://mercado.rava.com/api/prices/historico/arg";

/// Ruedas en comun entre Rava y BYMA usadas para calcular el factor de empalme.
const SPLICE_OVERLAP_DAYS: usize = 60;
/// Rango aceptable del factor Rava/BYMA. Fuera de esto las dos fuentes no estan
/// en la misma escala (ej. ECOG da ~10x, un split que Rava no ajusto) y se usa solo BYMA.
const SPLICE_FACTOR_RANGE: (f64, f64) = (0.5, 2.0);
/// Retorno log diario a partir del cual una rueda aislada se considera un pico.
const MAX_DAILY_LOG_JUMP: f64 = 1.0;
/// Hora UTC con la que Yahoo guarda cada rueda; las series reconstruidas usan la misma.
const DAILY_CANDLE_HOUR_UTC: u32 = 14;
/// Misma ventana que el `Range::Y10` que se le pide a Yahoo.
const HISTORY_YEARS: i64 = 10;
/// La API gratis de BYMA devuelve ~2 anios aunque se pidan mas.
const BYMA_REQUEST_YEARS: i64 = 3;

/// Origen de los historicos alternativos a Yahoo (se cambia en los tests).
pub struct CorrectionSources<'a> {
    pub byma_url: &'a str,
    pub rava_url: &'a str,
}

const DEFAULT_SOURCES: CorrectionSources<'static> = CorrectionSources {
    byma_url: BYMA_HISTORY_URL,
    rava_url: RAVA_HISTORY_URL,
};

/// Devuelve el historico de `ticker` corregido. Los tickers sin correccion
/// pasan tal cual. Si falla una fuente alternativa devuelve error, para no
/// pisar la base con la serie mala de Yahoo.
pub async fn apply_corrections(
    ticker: &str,
    rows: Vec<TickerHistoricalData>,
) -> Result<Vec<TickerHistoricalData>, String> {
    apply_corrections_with(ticker, rows, &DEFAULT_SOURCES).await
}

pub async fn apply_corrections_with(
    ticker: &str,
    rows: Vec<TickerHistoricalData>,
    sources: &CorrectionSources<'_>,
) -> Result<Vec<TickerHistoricalData>, String> {
    let corrected = match ticker {
        // Yahoo dejo de actualizar (A3) o tiene saltos sin ajustar: se arma con BYMA + Rava
        "A3" | "CARC" | "ECOG" | "GAMI" | "RICH" => {
            rebuild_from_byma_and_rava(ticker, sources).await?
        }
        // En Rava hay solo algunas ruedas sueltas antes de 2024
        "DOME" => trim_before(
            rebuild_from_byma_and_rava(ticker, sources).await?,
            date(2024, 1, 2),
        ),
        // 2024-12-16: Yahoo ajusto hacia atras el split ~4:1 del 17-12 pero no ese dia
        "SEMI" => remove_spikes(rows),
        // Saltos y ruedas sin volumen al inicio de la serie de Yahoo
        "CEPU" => trim_before(rows, date(2017, 2, 6)),
        "VALO" => trim_before(rows, date(2017, 8, 1)),
        _ => return Ok(rows),
    };
    println!(
        "[Data-Collector-Corrections] Historico de {} corregido: {} ruedas",
        ticker,
        corrected.len()
    );
    Ok(corrected)
}

fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).expect("fecha valida")
}

fn date_to_ts_millis(day: NaiveDate) -> i64 {
    Utc.from_utc_datetime(&day.and_hms_opt(DAILY_CANDLE_HOUR_UTC, 0, 0).unwrap())
        .timestamp_millis()
}

fn to_decimal(value: f64) -> Decimal {
    Decimal::from_f64(value).unwrap_or_default().round_dp(4)
}

/// Descarta las ruedas anteriores a `since`.
pub fn trim_before(rows: Vec<TickerHistoricalData>, since: NaiveDate) -> Vec<TickerHistoricalData> {
    let since_ts = Utc
        .from_utc_datetime(&since.and_hms_opt(0, 0, 0).unwrap())
        .timestamp_millis();
    rows.into_iter().filter(|row| row.ts >= since_ts).collect()
}

/// Reemplaza por la rueda anterior las ruedas aisladas que saltan mas de
/// `MAX_DAILY_LOG_JUMP` y vuelven al dia siguiente (el volumen se conserva).
pub fn remove_spikes(mut rows: Vec<TickerHistoricalData>) -> Vec<TickerHistoricalData> {
    rows.sort_by_key(|row| row.ts);
    let closes: Vec<f64> = rows
        .iter()
        .map(|row| row.close_amount.to_f64().unwrap_or(0.0))
        .collect();
    let log_return = |i: usize| (closes[i] / closes[i - 1]).ln();

    let spikes: Vec<usize> = (1..rows.len().saturating_sub(1))
        .filter(|&i| {
            let (current, next) = (log_return(i), log_return(i + 1));
            current.abs() > MAX_DAILY_LOG_JUMP
                && next.abs() > MAX_DAILY_LOG_JUMP
                && current.signum() != next.signum()
        })
        .collect();

    for i in spikes {
        let previous = rows[i - 1].clone();
        let row = &mut rows[i];
        row.open_amount = previous.open_amount;
        row.high_amount = previous.high_amount;
        row.low_amount = previous.low_amount;
        row.close_amount = previous.close_amount;
        row.close_unadj_amount = previous.close_unadj_amount;
    }
    rows
}

#[derive(Deserialize, Debug)]
struct BymaHistoryResponse {
    s: String,
    #[serde(default)]
    t: Vec<i64>,
    #[serde(default)]
    o: Vec<f64>,
    #[serde(default)]
    h: Vec<f64>,
    #[serde(default)]
    l: Vec<f64>,
    #[serde(default)]
    c: Vec<f64>,
    #[serde(default)]
    v: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DailyCandle {
    pub day: NaiveDate,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())
}

/// OHLCV diario de BYMA (plazo 24hs), ajustado por dividendos.
async fn fetch_byma_history(base_url: &str, ticker: &str) -> Result<Vec<DailyCandle>, String> {
    let now = Utc::now().timestamp();
    let from = now - 86_400 * 365 * BYMA_REQUEST_YEARS;
    let response: BymaHistoryResponse = http_client()?
        .get(base_url)
        .query(&[
            ("symbol", format!("{} 24HS", ticker)),
            ("resolution", "D".to_string()),
            ("from", from.to_string()),
            ("to", now.to_string()),
        ])
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("BYMA {}: {}", ticker, e))?
        .json()
        .await
        .map_err(|e| format!("BYMA {}: respuesta invalida: {}", ticker, e))?;

    if response.s != "ok" {
        return Err(format!("BYMA {}: status {}", ticker, response.s));
    }
    // BYMA marca cada rueda a las 00:00 de Argentina
    let argentina = FixedOffset::west_opt(3 * 3600).unwrap();
    let candles = (0..response.t.len())
        .filter_map(|i| {
            Some(DailyCandle {
                day: argentina
                    .timestamp_opt(response.t[i], 0)
                    .single()?
                    .date_naive(),
                open: *response.o.get(i)?,
                high: *response.h.get(i)?,
                low: *response.l.get(i)?,
                close: *response.c.get(i)?,
                volume: *response.v.get(i)?,
            })
        })
        .collect();
    Ok(candles)
}

/// OHLCV diario de Rava, sin ajustar. Si falta apertura/maximo/minimo se usa el cierre.
async fn fetch_rava_history(base_url: &str, ticker: &str) -> Result<Vec<DailyCandle>, String> {
    let response: RavaHistoricalResponse = http_client()?
        .get(format!("{}/{}", base_url, ticker))
        .query(&[("dias", "4435")])
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Rava {}: {}", ticker, e))?
        .json()
        .await
        .map_err(|e| format!("Rava {}: respuesta invalida: {}", ticker, e))?;
    Ok(response.datos.iter().filter_map(rava_candle).collect())
}

fn rava_candle(price: &PriceData) -> Option<DailyCandle> {
    if price.precio <= 0.0 {
        return None;
    }
    let or_close = |value: f64| if value > 0.0 { value } else { price.precio };
    Some(DailyCandle {
        day: NaiveDate::parse_from_str(&price.fecha, "%Y-%m-%d").ok()?,
        open: or_close(price.apertura),
        high: or_close(price.maximo),
        low: or_close(price.minimo),
        close: price.precio,
        volume: price.volumen,
    })
}

async fn rebuild_from_byma_and_rava(
    ticker: &str,
    sources: &CorrectionSources<'_>,
) -> Result<Vec<TickerHistoricalData>, String> {
    let (byma, rava) = tokio::join!(
        fetch_byma_history(sources.byma_url, ticker),
        fetch_rava_history(sources.rava_url, ticker)
    );
    let since = (Utc::now() - chrono::Duration::days(365 * HISTORY_YEARS)).date_naive();
    splice_rava_into_byma(ticker, &byma?, &rava?, since)
}

/// Historico armado con BYMA y Rava:
/// - desde que hay datos en BYMA se usa BYMA (limpio y ajustado por dividendos);
/// - antes, Rava dividida por la mediana de la razon Rava/BYMA en las primeras
///   `SPLICE_OVERLAP_DAYS` ruedas en comun, para llevarla a la misma base ajustada.
///   `close_unadj_amount` de esas ruedas es el cierre original de Rava.
pub fn splice_rava_into_byma(
    ticker: &str,
    byma: &[DailyCandle],
    rava: &[DailyCandle],
    since: NaiveDate,
) -> Result<Vec<TickerHistoricalData>, String> {
    let byma: BTreeMap<NaiveDate, &DailyCandle> = byma
        .iter()
        .filter(|c| c.close > 0.0)
        .map(|c| (c.day, c))
        .collect();
    let Some(&byma_start) = byma.keys().next() else {
        return Err(format!("BYMA {}: sin datos", ticker));
    };
    let rava: BTreeMap<NaiveDate, &DailyCandle> = rava
        .iter()
        .filter(|c| c.day >= since)
        .map(|c| (c.day, c))
        .collect();

    let mut ratios: Vec<f64> = rava
        .iter()
        .filter_map(|(day, r)| byma.get(day).map(|b| r.close / b.close))
        .take(SPLICE_OVERLAP_DAYS)
        .collect();
    ratios.sort_by(f64::total_cmp);
    let factor = match ratios.len() {
        0 => None,
        n if n % 2 == 1 => Some(ratios[n / 2]),
        n => Some((ratios[n / 2 - 1] + ratios[n / 2]) / 2.0),
    };

    let mut rows = Vec::with_capacity(rava.len() + byma.len());
    match factor {
        Some(f) if (SPLICE_FACTOR_RANGE.0..=SPLICE_FACTOR_RANGE.1).contains(&f) => {
            for candle in rava.range(..byma_start).map(|(_, c)| *c) {
                rows.push(TickerHistoricalData {
                    ticker: ticker.to_string(),
                    ts: date_to_ts_millis(candle.day),
                    volume: candle.volume as i64,
                    open_amount: to_decimal(candle.open / f),
                    high_amount: to_decimal(candle.high / f),
                    low_amount: to_decimal(candle.low / f),
                    close_amount: to_decimal(candle.close / f),
                    close_unadj_amount: to_decimal(candle.close),
                });
            }
        }
        Some(f) => eprintln!(
            "[Data-Collector-Corrections] {}: factor Rava/BYMA {:.4} fuera de rango, se usa solo BYMA",
            ticker, f
        ),
        None => eprintln!(
            "[Data-Collector-Corrections] {}: Rava y BYMA no tienen ruedas en comun, se usa solo BYMA",
            ticker
        ),
    }
    let rava_rows = rows.len();

    for candle in byma.values() {
        rows.push(TickerHistoricalData {
            ticker: ticker.to_string(),
            ts: date_to_ts_millis(candle.day),
            volume: candle.volume as i64,
            open_amount: to_decimal(candle.open),
            high_amount: to_decimal(candle.high),
            low_amount: to_decimal(candle.low),
            close_amount: to_decimal(candle.close),
            close_unadj_amount: to_decimal(candle.close),
        });
    }
    println!(
        "[Data-Collector-Corrections] {} reconstruida: factor Rava/BYMA {:?}, {} ruedas de Rava + {} de BYMA",
        ticker,
        factor,
        rava_rows,
        byma.len()
    );
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn row(ticker: &str, day: NaiveDate, close: f64) -> TickerHistoricalData {
        TickerHistoricalData {
            ticker: ticker.to_string(),
            ts: date_to_ts_millis(day),
            volume: 100,
            open_amount: to_decimal(close),
            high_amount: to_decimal(close),
            low_amount: to_decimal(close),
            close_amount: to_decimal(close),
            close_unadj_amount: to_decimal(close),
        }
    }

    fn candle(day: NaiveDate, close: f64) -> DailyCandle {
        DailyCandle {
            day,
            open: close,
            high: close * 1.1,
            low: close * 0.9,
            close,
            volume: 10.0,
        }
    }

    fn days(from: NaiveDate, n: usize) -> Vec<NaiveDate> {
        from.iter_days().take(n).collect()
    }

    #[tokio::test]
    async fn tickers_sin_correccion_pasan_tal_cual() {
        let rows = vec![row("GGAL", date(2024, 1, 2), 100.0)];
        let result = apply_corrections("GGAL", rows.clone()).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].close_amount, rows[0].close_amount);
    }

    #[tokio::test]
    async fn recorta_cepu_y_valo_antes_de_la_fecha_confiable() {
        let rows = vec![
            row("CEPU", date(2017, 2, 3), 1.0),
            row("CEPU", date(2017, 2, 6), 2.0),
            row("CEPU", date(2017, 2, 7), 3.0),
        ];
        let result = apply_corrections("CEPU", rows).await.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].ts, date_to_ts_millis(date(2017, 2, 6)));

        let rows = vec![
            row("VALO", date(2017, 7, 31), 1.0),
            row("VALO", date(2017, 8, 1), 2.0),
        ];
        assert_eq!(apply_corrections("VALO", rows).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn semi_quita_el_pico_aislado() {
        let rows = vec![
            row("SEMI", date(2024, 12, 13), 100.0),
            row("SEMI", date(2024, 12, 16), 400.0),
            row("SEMI", date(2024, 12, 17), 101.0),
        ];
        let result = apply_corrections("SEMI", rows).await.unwrap();
        assert_eq!(result[1].close_amount, to_decimal(100.0));
        assert_eq!(result[1].volume, 100);
        assert_eq!(result[2].close_amount, to_decimal(101.0));
    }

    #[test]
    fn remove_spikes_no_toca_saltos_que_se_sostienen() {
        // Un split sin ajustar que no vuelve al dia siguiente no es un pico
        let rows = vec![
            row("X", date(2024, 1, 3), 100.0),
            row("X", date(2024, 1, 2), 100.0),
            row("X", date(2024, 1, 4), 400.0),
            row("X", date(2024, 1, 5), 410.0),
        ];
        let result = remove_spikes(rows);
        let closes: Vec<Decimal> = result.iter().map(|r| r.close_amount).collect();
        assert_eq!(
            closes,
            vec![
                to_decimal(100.0),
                to_decimal(100.0),
                to_decimal(400.0),
                to_decimal(410.0)
            ]
        );
        assert!(remove_spikes(vec![]).is_empty());
    }

    #[test]
    fn empalma_rava_reescalada_antes_de_byma() {
        let rava_days = days(date(2024, 1, 1), 70);
        let byma_start = rava_days[10];
        // Rava esta 2% por encima de BYMA en el solape
        let rava: Vec<DailyCandle> = rava_days.iter().map(|&d| candle(d, 102.0)).collect();
        let byma: Vec<DailyCandle> = rava_days[10..].iter().map(|&d| candle(d, 100.0)).collect();

        let rows = splice_rava_into_byma("RICH", &byma, &rava, date(2016, 1, 1)).unwrap();

        assert_eq!(rows.len(), 70);
        assert_eq!(rows[0].close_amount, to_decimal(100.0));
        assert_eq!(rows[0].close_unadj_amount, to_decimal(102.0));
        assert_eq!(rows[9].ts, date_to_ts_millis(date(2024, 1, 10)));
        assert_eq!(rows[10].ts, date_to_ts_millis(byma_start));
        assert!(rows.windows(2).all(|w| w[0].ts < w[1].ts));
    }

    #[test]
    fn usa_solo_byma_si_el_factor_es_absurdo_o_no_hay_solape() {
        let d = days(date(2025, 1, 1), 20);
        let byma: Vec<DailyCandle> = d[5..].iter().map(|&x| candle(x, 10.0)).collect();

        let rava_10x: Vec<DailyCandle> = d.iter().map(|&x| candle(x, 100.0)).collect();
        let rows = splice_rava_into_byma("ECOG", &byma, &rava_10x, date(2016, 1, 1)).unwrap();
        assert_eq!(rows.len(), 15);

        let rava_sin_solape: Vec<DailyCandle> = d[..5].iter().map(|&x| candle(x, 10.0)).collect();
        let rows =
            splice_rava_into_byma("ECOG", &byma, &rava_sin_solape, date(2016, 1, 1)).unwrap();
        assert_eq!(rows.len(), 15);
    }

    #[test]
    fn recorta_rava_a_la_ventana_y_falla_sin_byma() {
        let d = days(date(2024, 1, 1), 30);
        let rava: Vec<DailyCandle> = d.iter().map(|&x| candle(x, 50.0)).collect();
        let byma: Vec<DailyCandle> = d[20..].iter().map(|&x| candle(x, 50.0)).collect();
        let rows = splice_rava_into_byma("CARC", &byma, &rava, d[15]).unwrap();
        assert_eq!(rows.len(), 15);

        assert!(splice_rava_into_byma("CARC", &[], &rava, d[0]).is_err());
    }

    #[test]
    fn rava_completa_ohlc_faltantes_con_el_cierre() {
        let json = r#"{"simbolo":"A3","datos":[
            {"precio": 10.0, "volumen": 5, "fecha": "2024-01-02T00:00:00.000Z", "timestamp": 1},
            {"precio": 0, "fecha": "2024-01-03T00:00:00.000Z", "timestamp": 2}
        ]}"#;
        let response: RavaHistoricalResponse = serde_json::from_str(json).unwrap();
        let candles: Vec<DailyCandle> = response.datos.iter().filter_map(rava_candle).collect();
        assert_eq!(candles.len(), 1);
        assert_eq!(candles[0].open, 10.0);
        assert_eq!(candles[0].high, 10.0);
        assert_eq!(candles[0].low, 10.0);
    }

    async fn mount_sources(server: &MockServer, ticker: &str, byma_status: &str) {
        // 2024-01-02 y 2024-01-03 a las 00:00 de Argentina
        let byma = serde_json::json!({
            "s": byma_status,
            "t": [1704164400_i64, 1704250800_i64],
            "o": [10.0, 11.0], "h": [10.5, 11.5], "l": [9.5, 10.5],
            "c": [10.0, 11.0], "v": [100, 200]
        });
        Mock::given(method("GET"))
            .and(path("/byma"))
            .and(query_param("symbol", format!("{} 24HS", ticker)))
            .respond_with(ResponseTemplate::new(200).set_body_json(byma))
            .mount(server)
            .await;
        let rava = serde_json::json!({
            "simbolo": ticker,
            "datos": [
                {"precio": 9.0, "maximo": 9.5, "minimo": 8.5, "apertura": 9.0, "volumen": 50,
                 "fecha": "2023-12-29T00:00:00.000Z", "timestamp": 1703851200},
                {"precio": 10.0, "maximo": 10.5, "minimo": 9.5, "apertura": 10.0, "volumen": 100,
                 "fecha": "2024-01-02T00:00:00.000Z", "timestamp": 1704196800}
            ]
        });
        Mock::given(method("GET"))
            .and(path(format!("/rava/{}", ticker)))
            .respond_with(ResponseTemplate::new(200).set_body_json(rava))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn reconstruye_desde_byma_y_rava_mockeados() {
        let server = MockServer::start().await;
        mount_sources(&server, "A3", "ok").await;
        let (byma_url, rava_url) = (
            format!("{}/byma", server.uri()),
            format!("{}/rava", server.uri()),
        );
        let sources = CorrectionSources {
            byma_url: &byma_url,
            rava_url: &rava_url,
        };

        // Lo que trae Yahoo se descarta
        let yahoo = vec![row("A3", date(2024, 1, 2), 999.0)];
        let rows = apply_corrections_with("A3", yahoo, &sources).await.unwrap();

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].ts, date_to_ts_millis(date(2023, 12, 29)));
        assert_eq!(rows[0].close_amount, to_decimal(9.0));
        assert_eq!(rows[1].ts, date_to_ts_millis(date(2024, 1, 2)));
        assert_eq!(rows[2].close_amount, to_decimal(11.0));
        assert_eq!(rows[2].volume, 200);
    }

    #[tokio::test]
    async fn dome_se_reconstruye_y_recorta() {
        let server = MockServer::start().await;
        mount_sources(&server, "DOME", "ok").await;
        let (byma_url, rava_url) = (
            format!("{}/byma", server.uri()),
            format!("{}/rava", server.uri()),
        );
        let sources = CorrectionSources {
            byma_url: &byma_url,
            rava_url: &rava_url,
        };

        let rows = apply_corrections_with("DOME", vec![], &sources)
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].ts, date_to_ts_millis(date(2024, 1, 2)));
    }

    #[tokio::test]
    async fn falla_si_byma_no_responde_ok() {
        let server = MockServer::start().await;
        mount_sources(&server, "GAMI", "no_data").await;
        let (byma_url, rava_url) = (
            format!("{}/byma", server.uri()),
            format!("{}/rava", server.uri()),
        );
        let sources = CorrectionSources {
            byma_url: &byma_url,
            rava_url: &rava_url,
        };
        assert!(
            apply_corrections_with("GAMI", vec![], &sources)
                .await
                .is_err()
        );

        let caido = CorrectionSources {
            byma_url: "http://127.0.0.1:1/byma",
            rava_url: "http://127.0.0.1:1/rava",
        };
        assert!(
            apply_corrections_with("CARC", vec![], &caido)
                .await
                .is_err()
        );
    }
}
