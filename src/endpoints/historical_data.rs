use axum::{
    extract::{Path, State},
    http::StatusCode,
};
use yfinance_rs::{Range, Ticker, YfClient};

use crate::persistence::ticker_repository::{self, TickerHistoricalData};

pub async fn api_get_historical_data(
    State(pool): State<sqlx::PgPool>,
    Path(ticker): Path<String>,
) -> axum::Json<serde_json::Value> {
    let historical_data =
        ticker_repository::is_historical_data_available(pool.clone(), &ticker).await;

    if historical_data.is_err() {
        eprintln!(
            "Failed to fetch historical data from database: {}",
            historical_data.err().unwrap()
        );

        let response = serde_json::json!({
            "status": StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            "message": serde_json::json!({ "error": "Failed to fetch historical data from database" }),
        });
        return axum::Json(response);
    }

    let (historical_data, was_cached) = match historical_data {
        Ok(Some((data, last_updated_timestamp))) => {
            // If last update timestamp is older than 5 days, we need to update.
            let five_days_ago = chrono::Utc::now() - chrono::Duration::days(5);

            if five_days_ago.timestamp() > last_updated_timestamp {
                // Fixed: Calling the clean standalone helper function instead of a local closure variable
                let new_data = fetch_yf_history_helper(&ticker).await;

                match new_data {
                    Ok(new_data) => {
                        let update_result = ticker_repository::update_historical_data(
                            pool,
                            &ticker,
                            new_data.clone(),
                        )
                        .await;

                        if update_result.is_err() {
                            eprintln!(
                                "Failed to update historical data in database: {}",
                                update_result.err().unwrap()
                            );
                            let response = serde_json::json!({
                                "status": StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                                "message": serde_json::json!({ "error": "Failed to update historical data in database" }),
                            });
                            return axum::Json(response);
                        }

                        (new_data, true)
                    }
                    Err(e) => {
                        eprintln!("Error fetching updated data: {}", e);
                        let response = serde_json::json!({
                            "status": StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                            "message": serde_json::json!({ "error": "Failed to fetch historical data from Yahoo Finance" }),
                        });
                        return axum::Json(response);
                    }
                }
            } else {
                (data, true)
            }
        }
        Ok(None) => {
            // Fetch cleanly from Yahoo Finance using our extracted routine
            match fetch_yf_history_helper(&ticker).await {
                Ok(historical_data) => {
                    // lets persist this data in the database.
                    let update_result = ticker_repository::update_historical_data(
                        pool,
                        &ticker,
                        historical_data.clone(),
                    )
                    .await;

                    if update_result.is_err() {
                        eprintln!(
                            "Failed to persist historical data in database: {}",
                            update_result.err().unwrap()
                        );
                    }

                    (historical_data, false)
                }
                Err(e) => {
                    eprintln!("Failed to fetch historical data from Yahoo Finance: {}", e);
                    let response = serde_json::json!({
                        "status": StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                        "message": serde_json::json!({ "error": "Failed to fetch historical data from Yahoo Finance" }),
                    });
                    return axum::Json(response);
                }
            }
        }
        Err(e) => {
            let formatted_error = format!("Failed to fetch historical data from database: {}", e);
            let response = serde_json::json!({
                "status": StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                "message": serde_json::json!({ "error": formatted_error }),
            });
            return axum::Json(response);
        }
    };

    let response = serde_json::json!({
        "status": StatusCode::OK.as_u16(),
        "data": historical_data,
        "cached": was_cached
    });

    axum::Json(response)
}

/// Standalone helper function to cleanly process the Yahoo Finance remote tracking
async fn fetch_yf_history_helper(ticker: &str) -> Result<Vec<TickerHistoricalData>, String> {
    let yfinance_client = YfClient::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
        .build()
        .map_err(|e| format!("Failed to create YF client: {}", e))?;

    let yfinance_ticker = Ticker::new(&yfinance_client, format!("{}.BA", ticker));

    let history_data = yfinance_ticker
        .history(Some(Range::Y10), Some(yfinance_rs::Interval::D1), false)
        .await
        .map_err(|e| format!("{}", e))?;

    let historical_data: Vec<TickerHistoricalData> = history_data
        .into_iter()
        .map(|entry| TickerHistoricalData::from_candle(&entry, ticker))
        .collect();

    Ok(historical_data)
}
