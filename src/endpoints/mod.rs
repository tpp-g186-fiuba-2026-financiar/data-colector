use axum::routing::{get, post};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::endpoints::{
    available_tickers::AvailableTickersResponse,
    health::HealthResponse,
    historical_data::HistoricalDataResponse,
    historical_movement::{DailyPrice, MovementResponse},
    interest_rates::InterestRateResponse,
};
use yfinance_rs::YfClient;

pub mod available_tickers;
pub mod health;
pub mod historical_data;
pub mod historical_movement;
pub mod interest_rates;
pub mod model_ready_tickers;
pub mod root;

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
    ),
    components(
        schemas(
            HealthResponse,
            AvailableTickersResponse,
            HistoricalDataResponse,
            MovementResponse,
            DailyPrice,
            InterestRateResponse,
        )
    ),
    tags(
        (name = "General", description = "Root and health check endpoints"),
        (name = "Tickers", description = "Listing of tickers (BYMA + commodities) cached by the collector"),
        (name = "Historical Data", description = "Historical OHLCV candles fetched from Yahoo Finance and cached in Postgres. Use GOLD or OIL como ticker para commodities."),
        (name = "Interest Rates", description = "Series de tasas de interés US (Yahoo) y AR (BCRA), cacheadas en Postgres")
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
        .with_state(dc_state)
        .merge(swagger)
}
