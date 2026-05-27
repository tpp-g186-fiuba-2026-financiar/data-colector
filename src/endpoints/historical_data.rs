use axum::{
    extract::{Path, State},
    http::StatusCode,
};
use yfinance_rs::{Range, Ticker, YfClient}; // Added YFClient import

pub struct HistoricalData;

impl HistoricalData {
    pub async fn api_get_historical_data(
        State(_): State<sqlx::PgPool>,
        Path(ticker): Path<String>,
    ) -> axum::Json<serde_json::Value> {
        // lets make a yahoo finance api call to get the historical data of the ticker, and return it as a json
        let yfinance_client = YfClient::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
            .build()
            .expect("Failed to create Yahoo Finance client");

        let yfinance_ticker = Ticker::new(&yfinance_client, format!("{}.BA", &ticker));
        let history = yfinance_ticker
            .history(Some(Range::Y10), Some(yfinance_rs::Interval::D1), false)
            .await;

        let (code, message) = match history {
            Ok(data) => {
                if data.is_empty() {
                    (
                        StatusCode::NOT_FOUND,
                        serde_json::json!({ "error": "No historical data found for the given ticker" }),
                    )
                } else {
                    (
                        StatusCode::OK,
                        serde_json::json!({ "historical_data": data }),
                    )
                }
            }
            Err(e) => {
                eprintln!("Failed to fetch historical data from Yahoo Finance: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    serde_json::json!({ "error": "Failed to fetch historical data from Yahoo Finance" }),
                )
            }
        };

        let response = serde_json::json!({
            "status": code.as_u16(),
            "message": message
        });

        axum::Json(response)
    }
}
