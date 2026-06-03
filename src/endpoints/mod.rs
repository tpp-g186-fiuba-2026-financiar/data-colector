use axum::routing::{get, post};

use crate::endpoints::{
    available_tickers::AvailableTickers, health::HealthHandler, root::RootHandler,
};

pub mod available_tickers;
pub mod health;
pub mod historical_data;
pub mod root;

pub fn data_collector_router(state_sqlxpool: sqlx::PgPool) -> axum::Router {
    axum::Router::new()
        // Endpoint that returns the health status of the application, including the database connection
        .route("/health", get(HealthHandler::health_check))
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
        .with_state(state_sqlxpool)
}
