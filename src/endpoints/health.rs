use axum::{extract::State, http::StatusCode};

pub struct HealthHandler;

impl HealthHandler {
    pub async fn health_check(State(pool): State<sqlx::PgPool>) -> axum::Json<serde_json::Value> {
        // lets return a simple json response with the status of the database connection
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
}
