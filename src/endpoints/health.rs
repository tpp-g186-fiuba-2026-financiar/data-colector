use crate::endpoints::DCState;
use axum::{extract::State, http::StatusCode};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    /// HTTP status code returned for the health check
    pub status: u16,
    /// Human readable message describing the current health status
    pub message: String,
}

#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Database connection healthy", body = HealthResponse, example = json!({
            "status": 200,
            "message": "Database connection is healthy and its working!"
        })),
        (status = 503, description = "Database connection unhealthy", body = HealthResponse)
    ),
    tag = "General"
)]
pub async fn health_check(State(dc_state): State<DCState>) -> axum::Json<serde_json::Value> {
    let pool = dc_state.sqlx_pool;
    let (code, message) = match sqlx::query("SELECT 1").execute(&pool).await {
        Ok(_) => (
            StatusCode::OK,
            String::from("Database connection is healthy and its working!"),
        ),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Database connection is unhealthy ({})", e),
        ),
    };

    let response = serde_json::json!({
        "status": code.as_u16(),
        "message": message
    });

    axum::Json(response)
}
