use axum::{extract::State, http::StatusCode};
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;

use crate::endpoints::DCState;
use crate::peso_dolar::peso_client;
use crate::persistence::peso_dolar_repository::{PesoDolarPoint};

const HISTORICAL_VALUE_TYPE: &str = "oficial";

#[derive(Serialize, ToSchema)]
pub struct PesoDolarCurrentResponse {
    /// HTTP status code
    pub status: u16,
    /// Raw current dolar API payload
    #[schema(value_type = Object)]
    pub data: Value,
}

#[derive(Serialize, ToSchema)]
pub struct PesoDolarHistoricalResponse {
    /// HTTP status code
    pub status: u16,
    /// Cached historical peso/dolar points
    #[schema(value_type = Vec<Object>)]
    pub data: Vec<PesoDolarPoint>,
    /// `true` if served from cache, `false` if freshly fetched
    pub cached: bool,
}

#[utoipa::path(
    post,
    path = "/peso-dolar/current",
    responses(
        (status = 200, description = "Current peso/dolar values from the external API", body = PesoDolarCurrentResponse),
        (status = 500, description = "Failed to fetch current peso/dolar rates", body = serde_json::Value)
    ),
    tag = "Peso Dolar"
)]
pub async fn api_get_current_rates(
    State(_dc_state): State<DCState>,
) -> axum::Json<serde_json::Value> {
    match peso_client::fetch_current().await {
        Ok(data) => axum::Json(serde_json::json!({
            "status": StatusCode::OK.as_u16(),
            "data": data,
        })),
        Err(err) => {
            eprintln!("Failed to fetch current peso/dolar: {}", err);
            axum::Json(serde_json::json!({
                "status": StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                "message": { "error": "Failed to fetch current peso/dolar values" },
            }))
        }
    }
}

#[utoipa::path(
    post,
    path = "/peso-dolar/historical",
    responses(
        (status = 200, description = "Historical peso/dolar values, updated if not cached", body = PesoDolarHistoricalResponse),
        (status = 500, description = "Failed to fetch or persist historical peso/dolar values", body = serde_json::Value)
    ),
    tag = "Peso Dolar"
)]
pub async fn api_get_historical_rates(
    State(dc_state): State<DCState>,
) -> axum::Json<serde_json::Value> {
    let pool = dc_state.sqlx_pool;

    match peso_client::fetch_historical_and_persist(pool).await {
        Ok((data, was_cached)) => axum::Json(serde_json::json!({
            "status": StatusCode::OK.as_u16(),
            "data": data,
            "cached": was_cached,
        })),
        Err(err) => {
            eprintln!("Failed to fetch historical peso/dolar data: {}", err);
            axum::Json(serde_json::json!({
                "status": StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                "message": { "error": "Failed to fetch or persist historical peso/dolar values" },
            }))
        }
    }
}