use axum::routing::get;

use crate::endpoints::{health::HealthHandler, root::RootHandler};

pub mod health;
pub mod root;

pub fn data_collector_router(state_sqlxpool: sqlx::PgPool) -> axum::Router {
    axum::Router::new()
        // Endpoint that returns the health status of the application, including the database connection
        .route("/health", get(HealthHandler::health_check))
        // Shows a message on the browser;
        .route("/", get(RootHandler::root_check))
        .with_state(state_sqlxpool)
}
