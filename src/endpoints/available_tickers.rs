use crate::endpoints::DCState;
use axum::{extract::State, http::StatusCode};

pub struct AvailableTickers;

impl AvailableTickers {
    pub async fn api_get_available_tickers(
        State(dc_state): State<DCState>,
    ) -> axum::Json<serde_json::Value> {
        // lets make a sql query to get all the tickers available in the database, and return them as a json response
        let pool = dc_state.sqlx_pool;
        // Notice we use `query_scalar` instead of `query`
        let tickers: Option<Vec<String>> =
            match sqlx::query_scalar("SELECT symbol FROM available_tickers_byma")
                .fetch_all(&pool)
                .await
            {
                Ok(tickers) => Some(tickers), // This is now a Vec<String>
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
}
