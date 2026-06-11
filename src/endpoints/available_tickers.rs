use crate::endpoints::DCState;
use axum::{extract::State, http::StatusCode};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct AvailableTickersResponse {
    /// HTTP status code returned by the endpoint
    pub status: u16,
    /// Either `{ "tickers": [...] }` on success or `{ "error": "..." }` on failure
    #[schema(value_type = Object)]
    pub message: serde_json::Value,
}

#[utoipa::path(
    post,
    path = "/available-tickers",
    responses(
        (status = 200, description = "List of all tickers cached in the database (BYMA + commodities)", body = AvailableTickersResponse, example = json!({
            "status": 200,
            "message": { "tickers": ["GGAL", "YPF", "GOLD", "OIL"] }
        })),
        (status = 404, description = "No tickers found in database", body = AvailableTickersResponse),
        (status = 500, description = "Database error while fetching tickers", body = AvailableTickersResponse)
    ),
    tag = "Tickers"
)]
pub async fn api_get_available_tickers(
    State(dc_state): State<DCState>,
) -> axum::Json<serde_json::Value> {
    let pool = dc_state.sqlx_pool;
    let tickers: Option<Vec<String>> =
        match sqlx::query_scalar("SELECT symbol FROM available_tickers_byma")
            .fetch_all(&pool)
            .await
        {
            Ok(tickers) => Some(tickers),
            Err(e) => {
                eprintln!("Failed to fetch tickers from database: {}", e);
                None
            }
        };

    let (code, message) = match tickers {
        Some(tickers) => {
            if tickers.is_empty() {
                (
                    StatusCode::NOT_FOUND,
                    serde_json::json!({ "error": "No tickers found in database" }),
                )
            } else {
                (StatusCode::OK, serde_json::json!({ "tickers": tickers }))
            }
        }
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": "Failed to fetch tickers from database" }),
        ),
    };

    let response = serde_json::json!({
        "status": code.as_u16(),
        "message": message
    });

    axum::Json(response)
}
