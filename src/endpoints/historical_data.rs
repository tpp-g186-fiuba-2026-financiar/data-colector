use crate::endpoints::DCState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
};
use serde::Serialize;
use utoipa::ToSchema;
use yfinance_rs::{Range, Ticker, YfClient};

use crate::persistence::ticker_repository::{self, TickerHistoricalData};

const MIN_MODEL_HISTORY_ROWS: usize = 100;

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
            "ticker_info": {
                "nombre_corto": "GGAL",
                "nombre_largo": "Grupo Financiero Galicia S.A.",
                "descripcion": "Grupo Financiero Galicia S.A. es una empresa argentina ...."
            },
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

    let ticker_in_depth_data =
        match ticker_repository::get_extended_info_for_ticker(pool.clone().into(), &ticker).await {
            Ok(Some(data)) => Some(data),
            Ok(None) => {
                eprintln!("No extended info found for ticker: {}", ticker);
                None
            }
            Err(e) => {
                eprintln!("Failed to fetch extended info for ticker {}: {}", ticker, e);
                None
            }
        };

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
                        if data.len() < MIN_MODEL_HISTORY_ROWS {
                            eprintln!(
                                "Not adding {} to model catalog: only {} historical rows",
                                ticker,
                                data.len()
                            );
                            return;
                        }
                        if let Err(error) =
                            ticker_repository::ensure_available_ticker(&pool, &ticker).await
                        {
                            eprintln!("Failed to add ticker to available catalog: {}", error);
                            return;
                        }
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
        "ticker_info": if let Some(info) = ticker_in_depth_data {
            serde_json::json!({
                "nombre_corto": info.nombre_corto,
                "nombre_largo": info.nombre_largo,
                "descripcion": info.descripcion,
            })
        } else {
            serde_json::Value::Null
        },
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Path, State};
    use sqlx::PgPool;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn build_test_dc_state(pool: PgPool) -> DCState {
        let yf_client = YfClient::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
        .build().expect("Error generating YfClient for test state");

        DCState {
            sqlx_pool: pool,
            yf_client,
        }
    }

    /// Builds a `DCState` whose `YfClient` points its chart-API base URL at a
    /// mock server, so tests can exercise the Yahoo Finance fetch paths
    /// without touching the network.
    async fn build_test_dc_state_with_mock_chart(pool: PgPool, server: &MockServer) -> DCState {
        let yf_client = YfClient::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
            .base_chart(url::Url::parse(&format!("{}/v8/finance/chart/", server.uri())).unwrap())
            .build()
            .expect("Error generating YfClient with mocked chart base for test state");

        DCState {
            sqlx_pool: pool,
            yf_client,
        }
    }

    /// Builds a minimal, valid Yahoo Finance `/v8/finance/chart/{symbol}` JSON
    /// payload with `num_points` daily candles, starting from a fixed epoch.
    fn build_chart_json(symbol: &str, num_points: usize) -> String {
        let base_ts: i64 = 1_700_000_000;
        let mut timestamps = Vec::with_capacity(num_points);
        let mut open = Vec::with_capacity(num_points);
        let mut high = Vec::with_capacity(num_points);
        let mut low = Vec::with_capacity(num_points);
        let mut close = Vec::with_capacity(num_points);
        let mut adjclose = Vec::with_capacity(num_points);
        let mut volume = Vec::with_capacity(num_points);

        for i in 0..num_points {
            let price = 100.0 + i as f64;
            timestamps.push(base_ts + (i as i64) * 86_400);
            open.push(price);
            high.push(price + 1.0);
            low.push(price - 1.0);
            close.push(price);
            adjclose.push(price);
            volume.push(1_000_u64 + i as u64);
        }

        serde_json::json!({
            "chart": {
                "error": serde_json::Value::Null,
                "result": [{
                    "meta": {
                        "currency": "USD",
                        "symbol": symbol,
                        "timezone": "America/New_York",
                        "gmtoffset": -14_400,
                    },
                    "timestamp": timestamps,
                    "indicators": {
                        "quote": [{
                            "open": open,
                            "high": high,
                            "low": low,
                            "close": close,
                            "volume": volume,
                        }],
                        "adjclose": [{ "adjclose": adjclose }],
                    },
                }],
            }
        })
        .to_string()
    }

    async fn mount_chart_mock(server: &MockServer, ticker_symbol: &str, body: &str) {
        Mock::given(method("GET"))
            .and(path(format!("/v8/finance/chart/{}.BA", ticker_symbol)))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
            .mount(server)
            .await;
    }

    async fn mount_chart_error_mock(server: &MockServer, ticker_symbol: &str, status: u16) {
        Mock::given(method("GET"))
            .and(path(format!("/v8/finance/chart/{}.BA", ticker_symbol)))
            .respond_with(ResponseTemplate::new(status))
            .mount(server)
            .await;
    }

    #[sqlx::test]
    async fn test_get_historical_data_from_cache(pool: PgPool) {
        let ticker_symbol = "GGAL";

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
            "INSERT INTO rava_tickers (ticker, short_name, long_name, description) VALUES ($1, 'Galicia', 'Grupo Financiero Galicia', 'Banco argentino')",
        )
        .bind(ticker_symbol)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS ticker_history_data_cached_yf (
                ticker               VARCHAR(25) NOT NULL,
                ts                   BIGINT NOT NULL,
                volume               BIGINT NOT NULL, 
                open_amount          NUMERIC NOT NULL,
                high_amount          NUMERIC NOT NULL,
                low_amount           NUMERIC NOT NULL,
                close_amount         NUMERIC NOT NULL,
                close_unadj_amount   NUMERIC NOT NULL,
                PRIMARY KEY (ticker, ts),
                CONSTRAINT fk_ticker
                    FOREIGN KEY (ticker)
                    REFERENCES available_tickers_byma (symbol)
                    ON DELETE CASCADE
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO available_tickers_byma (symbol, market) VALUES ($1, 'leading-equity') ON CONFLICT (symbol) DO NOTHING"
        )
        .bind(ticker_symbol)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO ticker_history_data_cached_yf (ticker, ts, volume, open_amount, high_amount, low_amount, close_amount, close_unadj_amount) 
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"
        )
        .bind(ticker_symbol)
        .bind(1747008000000_i64)
        .bind(12345_i64)
        .bind(100.50_f64)
        .bind(105.00_f64)
        .bind(99.75_f64)
        .bind(104.20_f64)
        .bind(104.20_f64)
        .execute(&pool)
        .await
        .unwrap();

        let now_timestamp = chrono::Utc::now().timestamp();
        sqlx::query(
            "UPDATE available_tickers_byma SET last_history_price_cached_at = TO_TIMESTAMP($1) WHERE symbol = $2"
        )
        .bind(now_timestamp)
        .bind(ticker_symbol)
        .execute(&pool)
        .await
        .unwrap();

        let dc_state = build_test_dc_state(pool).await;

        let response =
            api_get_historical_data(State(dc_state), Path(ticker_symbol.to_string())).await;

        assert_eq!(response.0["status"], 200);
        assert_eq!(response.0["cached"], true);
        assert_eq!(response.0["ticker_info"]["descripcion"], "Banco argentino");

        let data_array = response.0["data"].as_array().unwrap();
        assert!(!data_array.is_empty());
    }

    #[sqlx::test]
    async fn test_get_historical_data_reports_database_failure(pool: PgPool) {
        let dc_state = build_test_dc_state(pool.clone()).await;
        pool.close().await;

        let response = api_get_historical_data(State(dc_state), Path("GGAL".to_string())).await;

        assert_eq!(response.0["status"], 500);
        assert_eq!(
            response.0["message"]["error"],
            "Failed to fetch historical data from database"
        );
    }

    #[sqlx::test]
    async fn test_get_historical_data_fetches_from_yahoo_when_uncached_and_backfills_catalog(
        pool: PgPool,
    ) {
        let ticker_symbol = "NEWT";
        let server = MockServer::start().await;
        mount_chart_mock(
            &server,
            ticker_symbol,
            &build_chart_json(ticker_symbol, 105),
        )
        .await;

        let dc_state = build_test_dc_state_with_mock_chart(pool.clone(), &server).await;

        let response =
            api_get_historical_data(State(dc_state), Path(ticker_symbol.to_string())).await;

        assert_eq!(response.0["status"], 200);
        assert_eq!(response.0["cached"], false);
        assert!(response.0["ticker_info"].is_null());

        let data_array = response.0["data"].as_array().unwrap();
        assert_eq!(data_array.len(), 105);

        // The catalog/history persistence happens in a spawned background
        // task; give it a chance to run before asserting on its side effects.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let ticker_added: Option<String> =
            sqlx::query_scalar("SELECT symbol FROM available_tickers_byma WHERE symbol = $1")
                .bind(ticker_symbol)
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert_eq!(ticker_added.as_deref(), Some(ticker_symbol));

        let cached_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM ticker_history_data_cached_yf WHERE ticker = $1",
        )
        .bind(ticker_symbol)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cached_rows, 105);
    }

    #[sqlx::test]
    async fn test_get_historical_data_skips_catalog_when_yahoo_history_is_too_short(pool: PgPool) {
        let ticker_symbol = "TINY";
        let server = MockServer::start().await;
        mount_chart_mock(&server, ticker_symbol, &build_chart_json(ticker_symbol, 5)).await;

        let dc_state = build_test_dc_state_with_mock_chart(pool.clone(), &server).await;

        let response =
            api_get_historical_data(State(dc_state), Path(ticker_symbol.to_string())).await;

        assert_eq!(response.0["status"], 200);
        assert_eq!(response.0["cached"], false);
        assert_eq!(response.0["data"].as_array().unwrap().len(), 5);

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let ticker_added: Option<String> =
            sqlx::query_scalar("SELECT symbol FROM available_tickers_byma WHERE symbol = $1")
                .bind(ticker_symbol)
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert!(
            ticker_added.is_none(),
            "a ticker with too little history must not be added to the model catalog"
        );
    }

    #[sqlx::test]
    async fn test_get_historical_data_reports_yahoo_failure_when_uncached(pool: PgPool) {
        let ticker_symbol = "FAIL";
        let server = MockServer::start().await;
        mount_chart_error_mock(&server, ticker_symbol, 500).await;

        let dc_state = build_test_dc_state_with_mock_chart(pool, &server).await;

        let response =
            api_get_historical_data(State(dc_state), Path(ticker_symbol.to_string())).await;

        assert_eq!(response.0["status"], 500);
        assert_eq!(
            response.0["message"]["error"],
            "Failed to fetch historical data from Yahoo Finance"
        );
    }

    async fn seed_stale_cached_history(pool: &PgPool, ticker_symbol: &str) {
        sqlx::query(
            "INSERT INTO available_tickers_byma (symbol, market) VALUES ($1, 'leading-equity') ON CONFLICT (symbol) DO NOTHING"
        )
        .bind(ticker_symbol)
        .execute(pool)
        .await
        .unwrap();

        // A tiny `ts` (well below "5 days ago" in seconds) makes the cached
        // row look stale, forcing the endpoint down the refetch path.
        sqlx::query(
            "INSERT INTO ticker_history_data_cached_yf (ticker, ts, volume, open_amount, high_amount, low_amount, close_amount, close_unadj_amount)
             VALUES ($1, 1000, 10, 1.0, 2.0, 0.5, 1.5, 1.5)"
        )
        .bind(ticker_symbol)
        .execute(pool)
        .await
        .unwrap();
    }

    #[sqlx::test]
    async fn test_get_historical_data_refetches_when_cache_is_stale(pool: PgPool) {
        let ticker_symbol = "STAL";
        seed_stale_cached_history(&pool, ticker_symbol).await;

        let server = MockServer::start().await;
        mount_chart_mock(&server, ticker_symbol, &build_chart_json(ticker_symbol, 10)).await;

        let dc_state = build_test_dc_state_with_mock_chart(pool.clone(), &server).await;

        let response =
            api_get_historical_data(State(dc_state), Path(ticker_symbol.to_string())).await;

        assert_eq!(response.0["status"], 200);
        assert_eq!(response.0["cached"], true);
        assert_eq!(response.0["data"].as_array().unwrap().len(), 10);

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let cached_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM ticker_history_data_cached_yf WHERE ticker = $1",
        )
        .bind(ticker_symbol)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cached_rows, 10);
    }

    #[sqlx::test]
    async fn test_get_historical_data_reports_yahoo_failure_when_cache_is_stale(pool: PgPool) {
        let ticker_symbol = "STLF";
        seed_stale_cached_history(&pool, ticker_symbol).await;

        let server = MockServer::start().await;
        mount_chart_error_mock(&server, ticker_symbol, 500).await;

        let dc_state = build_test_dc_state_with_mock_chart(pool, &server).await;

        let response =
            api_get_historical_data(State(dc_state), Path(ticker_symbol.to_string())).await;

        assert_eq!(response.0["status"], 500);
        assert_eq!(
            response.0["message"]["error"],
            "Failed to fetch historical data from Yahoo Finance"
        );
    }
}
