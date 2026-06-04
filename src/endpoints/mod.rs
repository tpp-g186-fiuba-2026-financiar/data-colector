use axum::routing::{get, post};

use crate::endpoints::{
    available_tickers::AvailableTickers, health::HealthHandler, root::RootHandler,
};
use yfinance_rs::YfClient;

pub mod available_tickers;
pub mod health;
pub mod historical_data;
pub mod root;

#[derive(Clone)]
pub struct DCState {
    pub sqlx_pool: sqlx::PgPool,
    pub yf_client: YfClient,
}

pub fn data_collector_router(dc_state: DCState) -> axum::Router {
    axum::Router::new()
        // Endpoint that returns the health status of the application, including the database connection
        // In order to have access to UptimeRobot, we need to have a allowed HEAD method.
        .route(
            "/health",
            get(HealthHandler::health_check).post(HealthHandler::health_check),
        )
        // Shows a message on the browser;
        .route("/", get(RootHandler::root_check))
        .route(
            "/available-tickers",
            post(AvailableTickers::api_get_available_tickers),
        )
        .route(
            "/historical-data/{ticker}",
            post(historical_data::api_get_historical_data),
        )
        .with_state(dc_state)
}
