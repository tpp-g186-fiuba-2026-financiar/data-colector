use axum::Router;
use data_collector::{
    byma_scrapper::byma_session::BymaScrapper, errors::project_errors::DataCollectorError,
    persistence::ticker_repository,
};
use sqlx::postgres::PgPoolOptions;
use tokio::task::JoinHandle;

#[tokio::main]
async fn main() -> Result<(), DataCollectorError<'static>> {
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

    if let Err(e) = sqlx::migrate!().run(&pool).await {
        return Err(DataCollectorError::MigrationError(e));
    }

    let scraper_pool = pool.clone();
    let tickers_info: JoinHandle<Result<(), DataCollectorError>> = tokio::spawn(async move {
        loop {
            let byma_scrapper: BymaScrapper = match BymaScrapper::new().await {
                Ok(scrapper) => scrapper,
                Err(e) => {
                    eprintln!("[Data Collector] Failed to create BymaScrapper: {} — retrying in 60s", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                    continue;
                }
            };

            println!("[Data Collector] Fetching tickers information from Byma...");
            match byma_scrapper.get_all_available_tickers().await {
                Ok(quotes) => {
                    match ticker_repository::persist_quotes(&scraper_pool, &quotes).await {
                        Ok(inserted) => println!(
                            "[Data Collector] Persisted {} quote rows ({} fetched)",
                            inserted,
                            quotes.len()
                        ),
                        Err(e) => eprintln!("[Data Collector] Failed to persist quotes: {}", e),
                    }
                }
                Err(e) => eprintln!("[Data Collector] Failed to fetch tickers: {}", e),
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        }
    });

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

    match tickers_info.await {
        Ok(result) => {
            if let Err(e) = result {
                return Err(e);
            }
        }
        Err(e) => {
            eprintln!("Tickers info task panicked: {}", e);
        }
    }
    Ok(())
}
