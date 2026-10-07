use axum::routing::{get, post};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::endpoints::{
    available_tickers::AvailableTickersResponse,
    health::HealthResponse,
    historical_data::HistoricalDataResponse,
    historical_movement::{DailyPrice, MovementResponse},
    interest_rates::InterestRateResponse,
    ticker_sector::TickerSectorResponse,
};
use yfinance_rs::YfClient;

pub mod available_tickers;
pub mod health;
pub mod historical_data;
pub mod historical_movement;
pub mod interest_rates;
pub mod macro_series;
pub mod model_ready_tickers;
pub mod root;
pub mod ticker_sector;

#[derive(Clone)]
pub struct DCState {
    pub sqlx_pool: sqlx::PgPool,
    pub yf_client: YfClient,
}

#[derive(OpenApi)]
#[openapi(
    paths(
        root::root_check,
        health::health_check,
        available_tickers::api_get_available_tickers,
        historical_data::api_get_historical_data,
        historical_movement::api_get_historical_movement,
        interest_rates::api_get_us_interest_rate,
        interest_rates::api_get_ar_interest_rate,
        macro_series::api_get_argdatos_series,
        ticker_sector::api_get_ticker_sector,
    ),
    components(
        schemas(
            HealthResponse,
            AvailableTickersResponse,
            HistoricalDataResponse,
            MovementResponse,
            DailyPrice,
            InterestRateResponse,
            TickerSectorResponse,
        )
    ),
    tags(
        (name = "General", description = "Root and health check endpoints"),
        (name = "Tickers", description = "Listing of tickers (BYMA + commodities) cached by the collector"),
        (name = "Historical Data", description = "Historical OHLCV candles fetched from Yahoo Finance and cached in Postgres. Use GOLD or OIL como ticker para commodities."),
        (name = "Interest Rates", description = "Series de tasas de interés US (Yahoo) y AR (BCRA), cacheadas en Postgres"),
        (name = "Macro Series", description = "Series macro diarias de Argentina (ArgentinaDatos: dólar CCL/MEP/oficial/mayorista/blue y riesgo país), cacheadas en Postgres")
    ),
    info(
        title = "Data Collector API",
        description = "Background-collector exposing cached BYMA tickers + Yahoo Finance historical data (including GOLD and OIL commodities).",
        version = "0.1.0"
    )
)]
pub struct ApiDoc;

pub fn data_collector_router(dc_state: DCState) -> axum::Router {
    let swagger = SwaggerUi::new("/swagger").url("/swagger-endpoints.json", ApiDoc::openapi());

    axum::Router::new()
        .route(
            "/health",
            get(health::health_check).post(health::health_check),
        )
        .route("/", get(root::root_check))
        .route(
            "/available-tickers",
            post(available_tickers::api_get_available_tickers),
        )
        .route(
            "/model-ready-tickers",
            post(model_ready_tickers::api_get_model_ready_tickers),
        )
        .route(
            "/historical-data/{ticker}",
            post(historical_data::api_get_historical_data),
        )
        .route(
            "/historical-data/{ticker}/movement",
            get(historical_movement::api_get_historical_movement),
        )
        .route(
            "/interest-rate/us/{series}",
            post(interest_rates::api_get_us_interest_rate),
        )
        .route(
            "/interest-rate/ar/{series}",
            post(interest_rates::api_get_ar_interest_rate),
        )
        .route(
            "/macro/argdatos/{series}",
            post(macro_series::api_get_argdatos_series),
        )
        .route(
            "/ticker/sector/{ticker_name}",
            get(ticker_sector::api_get_ticker_sector),
        )
        .with_state(dc_state)
        .merge(swagger)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;
    use sqlx::PgPool;

    fn state(pool: PgPool) -> DCState {
        DCState {
            sqlx_pool: pool,
            yf_client: YfClient::builder()
                .user_agent("coverage-test")
                .build()
                .unwrap(),
        }
    }

    #[sqlx::test]
    async fn router_and_general_endpoints_cover_healthy_ready_and_database_errors(pool: PgPool) {
        sqlx::query("INSERT INTO available_tickers_byma (symbol, market) VALUES ('READY', 'test')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            r#"
            INSERT INTO ticker_history_data_cached_yf
                (ticker, ts, volume, open_amount, high_amount, low_amount, close_amount, close_unadj_amount)
            SELECT 'READY', value, 1, 1, 1, 1, 1, 1
            FROM generate_series(1, 100) AS value
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let dc_state = state(pool.clone());
        let _router = data_collector_router(dc_state.clone());
        assert!(root::root_check().await.contains("Data Collector API"));
        let health = health::health_check(State(dc_state.clone())).await;
        assert_eq!(health.0["status"], 200);
        let ready = model_ready_tickers::api_get_model_ready_tickers(State(dc_state.clone())).await;
        assert_eq!(ready.0["status"], 200);
        assert_eq!(ready.0["message"]["tickers"], serde_json::json!(["READY"]));

        pool.close().await;
        let health = health::health_check(State(dc_state.clone())).await;
        assert_eq!(health.0["status"], 503);
        let available = available_tickers::api_get_available_tickers(State(dc_state.clone())).await;
        assert_eq!(available.0["status"], 500);
        let ready = model_ready_tickers::api_get_model_ready_tickers(State(dc_state)).await;
        assert_eq!(ready.0["status"], 500);
    }
}
