//! Daily dollar quotations and country risk from ArgentinaDatos.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::persistence::interest_rate_repository::InterestRatePoint;

pub const SOURCE: &str = "ARGDATOS";

const BASE_URL: &str = "https://api.argentinadatos.com/v1";
const START_DATE: &str = "2015-01-01";

#[derive(Debug, PartialEq)]
enum Kind {
    /// `/cotizaciones/dolares/{casa}`, rows `{ fecha, venta }`
    Dollar(&'static str),
    /// `/finanzas/indices/riesgo-pais`, rows `{ fecha, valor }`
    CountryRisk,
}

fn resolve(series: &str) -> Option<Kind> {
    match series.to_uppercase().as_str() {
        "CCL" => Some(Kind::Dollar("contadoconliqui")),
        "MEP" => Some(Kind::Dollar("bolsa")),
        "OFICIAL" => Some(Kind::Dollar("oficial")),
        "MAYORISTA" => Some(Kind::Dollar("mayorista")),
        "BLUE" => Some(Kind::Dollar("blue")),
        "RIESGO_PAIS" => Some(Kind::CountryRisk),
        _ => None,
    }
}

pub fn is_supported(series: &str) -> bool {
    resolve(series).is_some()
}

#[derive(Debug, Deserialize)]
struct DollarRow {
    fecha: String,
    venta: Option<Decimal>,
}

#[derive(Debug, Deserialize)]
struct IndexRow {
    fecha: String,
    valor: Option<Decimal>,
}

pub async fn fetch_series(series: &str) -> Result<Vec<InterestRatePoint>, String> {
    fetch_series_from(BASE_URL, series).await
}

pub async fn fetch_series_from(
    base_url: &str,
    series: &str,
) -> Result<Vec<InterestRatePoint>, String> {
    let kind =
        resolve(series).ok_or_else(|| format!("Unsupported ArgentinaDatos series: {}", series))?;
    let path = match &kind {
        Kind::Dollar(casa) => format!("cotizaciones/dolares/{}", casa),
        Kind::CountryRisk => "finanzas/indices/riesgo-pais".to_string(),
    };
    let url = format!("{}/{}", base_url, path);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to call ArgentinaDatos: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "ArgentinaDatos returned status {}",
            response.status()
        ));
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read ArgentinaDatos response: {}", e))?;

    parse_response(&body, series, &kind)
}

fn parse_response(body: &str, series: &str, kind: &Kind) -> Result<Vec<InterestRatePoint>, String> {
    let rows: Vec<(String, Option<Decimal>)> = match kind {
        Kind::Dollar(_) => serde_json::from_str::<Vec<DollarRow>>(body)
            .map_err(|e| format!("Unexpected ArgentinaDatos response: {}", e))?
            .into_iter()
            .map(|r| (r.fecha, r.venta))
            .collect(),
        Kind::CountryRisk => serde_json::from_str::<Vec<IndexRow>>(body)
            .map_err(|e| format!("Unexpected ArgentinaDatos response: {}", e))?
            .into_iter()
            .map(|r| (r.fecha, r.valor))
            .collect(),
    };

    let series_id = series.to_uppercase();
    let start = NaiveDate::parse_from_str(START_DATE, "%Y-%m-%d").expect("valid constant");

    Ok(rows
        .into_iter()
        .filter_map(|(fecha, value)| {
            let date = NaiveDate::parse_from_str(&fecha, "%Y-%m-%d").ok()?;
            if date < start {
                return None;
            }
            let ts = date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis();
            Some(InterestRatePoint {
                source: SOURCE.to_string(),
                series_id: series_id.clone(),
                ts,
                value: value?,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn resolves_supported_series_case_insensitively() {
        assert_eq!(resolve("ccl"), Some(Kind::Dollar("contadoconliqui")));
        assert_eq!(resolve("MEP"), Some(Kind::Dollar("bolsa")));
        assert_eq!(resolve("riesgo_pais"), Some(Kind::CountryRisk));
        assert_eq!(resolve("unknown"), None);
        assert!(is_supported("blue"));
        assert!(!is_supported("unknown"));
    }

    #[tokio::test]
    async fn rejects_unsupported_series_before_networking() {
        let error = fetch_series("unknown").await.unwrap_err();
        assert!(error.contains("Unsupported ArgentinaDatos series"));
    }

    #[test]
    fn parses_dollar_rows_using_the_selling_price_and_skipping_old_or_empty_ones() {
        let body = r#"[
            { "casa": "contadoconliqui", "compra": 6.67, "venta": 6.67, "fecha": "2013-01-02" },
            { "casa": "contadoconliqui", "compra": 1600.0, "venta": 1610.8, "fecha": "2026-10-06" },
            { "casa": "contadoconliqui", "compra": 1, "venta": null, "fecha": "2026-10-07" },
            { "casa": "contadoconliqui", "compra": 1, "venta": 5, "fecha": "no-fecha" }
        ]"#;

        let points = parse_response(body, "ccl", &Kind::Dollar("contadoconliqui")).unwrap();

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].series_id, "CCL");
        assert_eq!(points[0].source, SOURCE);
        assert_eq!(points[0].value, Decimal::new(16108, 1));
    }

    #[test]
    fn parses_country_risk_rows() {
        let body =
            r#"[{ "valor": 937, "fecha": "1999-01-22" }, { "valor": 599, "fecha": "2026-10-05" }]"#;

        let points = parse_response(body, "riesgo_pais", &Kind::CountryRisk).unwrap();

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].series_id, "RIESGO_PAIS");
        assert_eq!(points[0].value, Decimal::new(599, 0));
    }

    #[test]
    fn rejects_unexpected_payloads() {
        let error = parse_response("{\"error\":1}", "ccl", &Kind::Dollar("bolsa")).unwrap_err();
        assert!(error.contains("Unexpected ArgentinaDatos response"));
    }

    #[tokio::test]
    async fn fetches_from_the_dollar_and_country_risk_paths() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cotizaciones/dolares/contadoconliqui"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"[{ "casa": "contadoconliqui", "compra": 1, "venta": 1610.8, "fecha": "2026-10-06" }]"#,
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/finanzas/indices/riesgo-pais"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"[{ "valor": 599, "fecha": "2026-10-05" }]"#),
            )
            .mount(&server)
            .await;

        let ccl = fetch_series_from(&server.uri(), "ccl").await.unwrap();
        let rp = fetch_series_from(&server.uri(), "riesgo_pais")
            .await
            .unwrap();

        assert_eq!(ccl.len(), 1);
        assert_eq!(rp.len(), 1);
    }

    #[tokio::test]
    async fn surfaces_http_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let error = fetch_series_from(&server.uri(), "mep").await.unwrap_err();

        assert!(error.contains("500"));
    }
}
