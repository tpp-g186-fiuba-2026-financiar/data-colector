use axum::{routing::get, Router, Extension, http::StatusCode};
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    // Lets load the .env file and apply it, if it fails, we throw error
    if let Err(e) = dotenv::dotenv() {
        eprintln!("[Data-Collector error] Failed to load .env {}", e);
        std::process::exit(1);
    }


    let db_url = std::env::var("DATABASE_URL").expect("[Data-Collector error] DATABASE_URL must be set");
    let api_port = std::env::var("API_PORT").expect("[Data-Collector error] API_PORT must be set");
    
    // Create a connection pool
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .expect("Failed to connect to Postgres");

    let app = Router::new()
        .route("/health", get(health_check))
        .layer(Extension(pool));

    let formatted_addr = format!("0.0.0.0:{}", api_port);

    let listener = match tokio::net::TcpListener::bind(formatted_addr).await {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("[Data-Collector error] Failed to bind to address: {}", e);
            std::process::exit(1);
        }
    };

    match axum::serve(listener, app).await {
        Ok(_) => {
            println!("[Data Collector] API is now running on port {}", api_port);
        },
        Err(e) => {
            eprintln!("[Data-Collector error] Failed to start server: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

async fn health_check(Extension(pool): Extension<sqlx::PgPool>) -> axum::Json<serde_json::Value> {

    // lets return a simple json response with the status of the database connection
    let (code, message) = match sqlx::query("SELECT 1").execute(&pool).await {
        Ok(_) => (StatusCode::OK, String::from("Database connection is healthy and its working!")),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, format!("Database connection is unhealthy ({})", e)),
    };

    let response = serde_json::json!({
        "status": code.as_u16(),
        "message": message
    });

    axum::Json(response)
}
