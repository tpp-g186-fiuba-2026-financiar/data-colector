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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoints::DCState;
    use axum::extract::State;
    use sqlx::PgPool;
    use yfinance_rs::YfClient;

    fn build_test_state(pool: PgPool) -> DCState {
        DCState {
            sqlx_pool: pool,
            yf_client: YfClient::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
            .build().expect("Error generating YfClient for test state"),
        }
    }

    #[sqlx::test]
    async fn test_get_available_tickers_success(pool: PgPool) {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS available_tickers_byma (
                id          SERIAL PRIMARY KEY,
                symbol      TEXT NOT NULL UNIQUE,
                market      TEXT NOT NULL,
                created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                last_history_price_cached_at TIMESTAMP WITH TIME ZONE
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO available_tickers_byma (symbol, market) VALUES ('GGAL', 'leading-equity'), ('YPF', 'leading-equity')"
        )
        .execute(&pool)
        .await
        .unwrap();

        let state = build_test_state(pool);

        let response = api_get_available_tickers(State(state)).await;

        assert_eq!(response.0["status"], 200);

        let tickers = response.0["message"]["tickers"].as_array().unwrap();

        assert_eq!(tickers.len(), 2);
        assert!(tickers.contains(&serde_json::json!("GGAL")));
        assert!(tickers.contains(&serde_json::json!("YPF")));
    }

    #[sqlx::test]
    async fn test_get_available_tickers_not_found(pool: PgPool) {
        sqlx::query("CREATE TABLE IF NOT EXISTS available_tickers_byma (symbol TEXT PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();

        let state = build_test_state(pool);

        let response = api_get_available_tickers(State(state)).await;

        assert_eq!(response.0["status"], 404);
        assert_eq!(
            response.0["message"]["error"],
            "No tickers found in database"
        );
    }
}
