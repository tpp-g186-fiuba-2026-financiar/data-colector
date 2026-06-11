use crate::endpoints::DCState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
};
use serde::Serialize;
use utoipa::ToSchema;
use yfinance_rs::{Range, Ticker, YfClient};

use crate::persistence::ticker_repository::{self, TickerHistoricalData};

#[derive(Serialize, ToSchema)]
pub struct HistoricalDataResponse {
    /// HTTP status code returned by the endpoint
    pub status: u16,
    /// Vector of historical candles for the requested ticker
    #[schema(value_type = Vec<Object>)]
    pub data: serde_json::Value,
    /// `true` if the data was served from cache (or freshly cached), `false` if it was fetched live
    pub cached: bool,
}

#[utoipa::path(
    post,
    path = "/historical-data/{ticker}",
    params(
        ("ticker" = String, Path, description = "Clean ticker symbol (e.g. GGAL, YPF, GOLD, OIL). BYMA tickers are resolved with the .BA suffix; commodities use their internal symbol.")
    ),
    responses(
        (status = 200, description = "Historical candles for the ticker", body = HistoricalDataResponse, example = json!({
            "status": 200,
            "data": [{
                "ticker": "GGAL",
                "ts": 1747008000000_i64,
                "volume": 12345,
                "open_amount": "100.50",
                "high_amount": "105.00",
                "low_amount": "99.75",
                "close_amount": "104.20",
                "close_unadj_amount": "104.20"
            }],
            "cached": true
        })),
        (status = 500, description = "Either Yahoo Finance or the database failed", body = serde_json::Value)
    ),
    tag = "Historical Data"
)]
pub async fn api_get_historical_data(
    State(dc_state): State<DCState>,
    Path(ticker): Path<String>,
) -> axum::Json<serde_json::Value> {
    let (pool, yf_client) = (dc_state.sqlx_pool.clone(), dc_state.yf_client.clone());

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
                let new_data = fetch_yf_history_helper(&yf_client, &ticker).await;

                match new_data {
                    Ok(new_data) => {
                        let data = new_data.clone();
                        tokio::spawn(async move {
                            let update_result = ticker_repository::update_historical_data(
                                pool.clone(),
                                &ticker,
                                new_data.clone(),
                            )
                            .await;

                            if update_result.is_err() {
                                eprintln!(
                                    "Failed to update historical data in database: {}",
                                    update_result.err().unwrap()
                                );
                            }
                        });

                        (data, true)
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
            match fetch_yf_history_helper(&yf_client, &ticker).await {
                Ok(historical_data) => {
                    let data: Vec<TickerHistoricalData> = historical_data.clone();
                    tokio::spawn(async move {
                        let update_result =
                            ticker_repository::update_historical_data(pool, &ticker, data.clone())
                                .await;

                        if update_result.is_err() {
                            eprintln!(
                                "Failed to persist historical data in database: {}",
                                update_result.err().unwrap()
                            );
                        }
                    });

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
async fn fetch_yf_history_helper(
    yf_client: &YfClient,
    ticker: &str,
) -> Result<Vec<TickerHistoricalData>, String> {
    let yfinance_ticker = Ticker::new(yf_client, format!("{}.BA", ticker));

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
