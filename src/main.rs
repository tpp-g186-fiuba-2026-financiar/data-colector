use axum::Router;
use data_collector::errors::project_errors::DataCollectorError;
use sqlx::PgPool;
use config::Config;

#[tokio::main]
async fn main() -> Result<(), DataCollectorError<'static>> {
    let cfg = Config::from_env();
    let pool = PgPool::connect(&cfg.database_url)
        .await
        .expect("Failed to connect to database");


    let app = Router::new().merge(data_collector::endpoints::data_collector_router(pool));

    let formatted_addr = format!("0.0.0.0:{}", &cfg.port);

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
