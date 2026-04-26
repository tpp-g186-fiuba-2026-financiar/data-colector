use axum::Router;
use data_collector::errors::project_errors::DataCollectorError;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), DataCollectorError<'static>> {
    // Lets load the .env file and apply it, if it fails, we throw error
    if let Err(e) = dotenv::dotenv() {
        return Err(DataCollectorError::EnviromentFileError(e));
    }

    let db_url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(e) => {
            return Err(DataCollectorError::EnviromentVariableError(
                "DATABASE_URL",
                e,
            ));
        }
    };
    let api_port = match std::env::var("API_PORT") {
        Ok(port) => port,
        Err(e) => {
            return Err(DataCollectorError::EnviromentVariableError("API_PORT", e));
        }
    };

    // Create a connection pool
    let pool = match PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
    {
        Ok(pool) => pool,
        Err(e) => {
            return Err(DataCollectorError::PosgresConnectionError(e));
        }
    };

    let app = Router::new().merge(data_collector::endpoints::data_collector_router(pool));

    let formatted_addr = format!("0.0.0.0:{}", api_port);

    let listener = match tokio::net::TcpListener::bind(formatted_addr.clone()).await {
        Ok(listener) => listener,
        Err(e) => {
            return Err(DataCollectorError::TcpBindError(e));
        }
    };

    println!(
        "[Data Collector] API is now starting to deliver on port {} ({})",
        api_port, formatted_addr
    );

    // 2. Then start the server
    if let Err(e) = axum::serve(listener, app).await {
        return Err(DataCollectorError::AxumServeError(e));
    }

    Ok(())
}
