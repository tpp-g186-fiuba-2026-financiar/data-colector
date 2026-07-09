//! Precio real (dia por dia) de un ticker entre una fecha base y N ruedas
//! habiles despues.
//!
//! Pensado para corroborar a mano las predicciones de `api-ml`: se les pasa
//! el mismo `as_of` y `horizon_days` que devolvio una prediccion, y este
//! endpoint devuelve la serie de precios reales de esas ruedas (mas un
//! resumen del retorno base->target), usando *solo* los datos ya cacheados
//! en `ticker_history_data_cached_yf` (no pega contra Yahoo).
//!
//! El horizonte se cuenta en ruedas presentes en la tabla, no en dias de
//! calendario -- es la misma nocion de "horizon" que usa el LSTM/XGBoost al
//! entrenar (`build_features` en api-ml).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{NaiveDate, TimeZone, Utc};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::endpoints::DCState;
use crate::persistence::ticker_repository;

/// Banda neutral (en valor absoluto) por debajo de la cual el movimiento se
/// considera "neutral" en vez de alza/baja. Mismo valor que el
/// `neutral_band` default de los modelos de tendencia en api-ml, para que
/// `signal` sea directamente comparable contra el campo homonimo que
/// devuelve `/predict/trend`.
const NEUTRAL_BAND: f64 = 0.01;

#[derive(Debug, Deserialize, IntoParams)]
pub struct MovementQuery {
    /// Fecha base, formato YYYY-MM-DD (el "as_of" de la prediccion a corroborar).
    pub from: String,
    /// Cantidad de ruedas habiles hacia adelante (el "horizon_days" de la prediccion).
    pub days: i64,
}

/// Precio real de una rueda puntual (tal cual esta cacheado en la tabla).
#[derive(Serialize, ToSchema)]
pub struct DailyPrice {
    pub date: String,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: i64,
}

#[derive(Serialize, ToSchema)]
pub struct MovementResponse {
    /// Status logico (200 en exito; el HTTP status siempre es 200, igual que el resto de la API)
    pub status: u16,
    pub ticker: String,
    /// Fecha pedida por query param, tal cual vino
    pub requested_from: String,
    /// Rueda base realmente usada (puede diferir de `requested_from` si esa fecha no es habil / no esta cacheada)
    pub base_date: String,
    pub base_close: String,
    /// Ruedas habiles hacia adelante pedidas
    pub days: i64,
    /// Rueda encontrada `days` ruedas habiles despues de `base_date`
    pub target_date: String,
    pub target_close: String,
    pub abs_change: String,
    /// Retorno simple (%), directamente comparable contra `expected_return * 100` de `/predict/trend`
    pub pct_return: f64,
    /// Retorno logaritmico acumulado, directamente comparable contra la salida cruda de los modelos
    pub log_return: f64,
    /// alza / baja / neutral, con la misma banda que usan los modelos de tendencia
    pub signal: String,
    /// Precio real dia por dia, desde `base_date` hasta `target_date` inclusive
    pub series: Vec<DailyPrice>,
}

#[utoipa::path(
    get,
    path = "/historical-data/{ticker}/movement",
    params(
        ("ticker" = String, Path, description = "Ticker limpio (ej: GGAL)"),
        MovementQuery
    ),
    responses(
        (status = 200, description = "Precio real dia por dia entre `from` y `days` ruedas habiles despues (mas un resumen del retorno), usando solo datos ya cacheados", body = MovementResponse, example = json!({
            "status": 200,
            "ticker": "GGAL",
            "requested_from": "2026-06-25",
            "base_date": "2026-06-25",
            "base_close": "7605",
            "days": 5,
            "target_date": "2026-07-02",
            "target_close": "7910",
            "abs_change": "305",
            "pct_return": 4.0105,
            "log_return": 0.039309,
            "signal": "alza",
            "series": [
                { "date": "2026-06-25", "open": "7650", "high": "7720", "low": "7580", "close": "7605", "volume": 1200000_i64 },
                { "date": "2026-06-26", "open": "7610", "high": "7750", "low": "7600", "close": "7715", "volume": 980000_i64 },
                { "date": "2026-06-29", "open": "7720", "high": "7900", "low": "7710", "close": "7885", "volume": 1500000_i64 },
                { "date": "2026-06-30", "open": "7880", "high": "7890", "low": "7760", "close": "7790", "volume": 1100000_i64 },
                { "date": "2026-07-01", "open": "7800", "high": "7810", "low": "7650", "close": "7685", "volume": 900000_i64 },
                { "date": "2026-07-02", "open": "7690", "high": "7920", "low": "7680", "close": "7910", "volume": 1700000_i64 }
            ]
        })),
        (status = 404, description = "No hay datos cacheados para el ticker (correr POST /historical-data/{ticker} primero)", body = serde_json::Value),
        (status = 422, description = "Fecha fuera del rango cacheado, o todavia no pasaron esas `days` ruedas habiles", body = serde_json::Value)
    ),
    tag = "Historical Data"
)]
pub async fn api_get_historical_movement(
    State(dc_state): State<DCState>,
    Path(ticker): Path<String>,
    Query(query): Query<MovementQuery>,
) -> axum::Json<serde_json::Value> {
    let ticker = ticker.trim().to_uppercase();

    if query.days < 1 {
        return error_response(StatusCode::UNPROCESSABLE_ENTITY, "'days' debe ser >= 1");
    }

    let requested_from = match NaiveDate::parse_from_str(query.from.trim(), "%Y-%m-%d") {
        Ok(date) => date,
        Err(_) => {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "'from' invalido, usar formato YYYY-MM-DD",
            );
        }
    };

    let rows = match ticker_repository::fetch_ordered_history(&dc_state.sqlx_pool, &ticker).await
    {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("Failed to fetch ordered history for {}: {}", ticker, e);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch historical data from database",
            );
        }
    };

    if rows.is_empty() {
        return error_response(
            StatusCode::NOT_FOUND,
            &format!(
                "no hay datos cacheados para '{}'; correr POST /historical-data/{} primero",
                ticker, ticker
            ),
        );
    }

    // Cada rueda cacheada, con su fecha (UTC) ya resuelta.
    let dated_rows: Vec<(NaiveDate, &ticker_repository::TickerHistoricalData)> = rows
        .iter()
        .filter_map(|row| {
            Utc.timestamp_millis_opt(row.ts)
                .single()
                .map(|dt| (dt.date_naive(), row))
        })
        .collect();

    // Ultima rueda <= la fecha pedida (mismo criterio que usa el modelo: "as_of" = ultimo cierre conocido).
    let base_index = dated_rows
        .iter()
        .rposition(|(date, _)| *date <= requested_from);

    let Some(base_index) = base_index else {
        let first_date = dated_rows.first().map(|(d, _)| d.to_string()).unwrap_or_default();
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            &format!(
                "no hay datos de '{}' antes de {}; la serie cacheada arranca el {}",
                ticker, requested_from, first_date
            ),
        );
    };

    let target_index = base_index + query.days as usize;

    if target_index >= dated_rows.len() {
        let (last_date, _) = dated_rows.last().expect("dated_rows no esta vacio");
        let available = dated_rows.len() - 1 - base_index;
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            &format!(
                "todavia no pasaron esas {} ruedas habiles desde {}; hay datos hasta {} ({} rueda(s) disponible(s) desde la base)",
                query.days,
                dated_rows[base_index].0,
                last_date,
                available
            ),
        );
    }

    let (base_date, base_row) = dated_rows[base_index];
    let (target_date, target_row) = dated_rows[target_index];

    let base_close = base_row.close_amount;
    let target_close = target_row.close_amount;
    let abs_change = target_close - base_close;

    let (base_f64, target_f64) = match (base_close.to_f64(), target_close.to_f64()) {
        (Some(b), Some(t)) if b != 0.0 => (b, t),
        _ => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "no se pudo convertir los precios cacheados a numero",
            );
        }
    };

    let return_fraction = target_f64 / base_f64 - 1.0;
    let pct_return = return_fraction * 100.0;
    let log_return = (target_f64 / base_f64).ln();

    let signal = if return_fraction > NEUTRAL_BAND {
        "alza"
    } else if return_fraction < -NEUTRAL_BAND {
        "baja"
    } else {
        "neutral"
    };

    // Precio real de cada rueda entre la base y el target, inclusive.
    let series: Vec<serde_json::Value> = dated_rows[base_index..=target_index]
        .iter()
        .map(|(date, row)| {
            serde_json::json!({
                "date": date.to_string(),
                "open": row.open_amount.to_string(),
                "high": row.high_amount.to_string(),
                "low": row.low_amount.to_string(),
                "close": row.close_amount.to_string(),
                "volume": row.volume,
            })
        })
        .collect();

    let response = serde_json::json!({
        "status": StatusCode::OK.as_u16(),
        "ticker": ticker,
        "requested_from": requested_from.to_string(),
        "base_date": base_date.to_string(),
        "base_close": base_close.to_string(),
        "days": query.days,
        "target_date": target_date.to_string(),
        "target_close": target_close.to_string(),
        "abs_change": abs_change.to_string(),
        "pct_return": pct_return,
        "log_return": log_return,
        "signal": signal,
        "series": series,
    });

    axum::Json(response)
}

fn error_response(status: StatusCode, message: &str) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": status.as_u16(),
        "message": { "error": message },
    }))
}
