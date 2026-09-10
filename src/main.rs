use axum::Router;
use data_collector::{
    commodities_scrapper::commodities_persist_historical_price::CommoditiesHistoricalPersistor,
    endpoints::DCState,
    errors::project_errors::DataCollectorError,
    site_scrappers::{
        byma_scrapper::{
            byma_persist_historical_price::BymaTickerHistoricalDataPersistor,
            byma_persist_tickers::BymaTickersPersistor,
        },
        rava_scrapper::rava_scrapper_handler::RavaFetcher,
    },
};
use tower_http::cors::{Any, CorsLayer};
use http::Method;
use sqlx::postgres::PgPoolOptions;
use yfinance_rs::YfClient;

#[tokio::main]
async fn main() -> Result<(), DataCollectorError<'static>> {
    if dotenv::dotenv().is_err() {
        // No longer crashing because dotenv isn't available now.
        eprintln!("[Data-Collector] No .env found, so most likely you're on prod!")
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
    let frontend_url = match std::env::var("FRONTEND_URL") {
        Ok(url) => url,
        Err(e) => {
            return Err(DataCollectorError::EnviromentVariableError(
                "FRONTEND_URL",
                e,
            ));
        }
    };

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

    tokio::spawn(BymaTickersPersistor::persist_available_tickers(
        scraper_pool.clone(),
    ));

    tokio::spawn(RavaFetcher::fetch_rava_tickers(scraper_pool.clone()));

    let yfinance_client = match YfClient::builder()
    .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
    .build()
    {
        Ok(client) => client,
        Err(e) => {
            return Err(DataCollectorError::YFinanceClientError(e));
        }
    };

    let dc_state = DCState {
        sqlx_pool: pool.clone(),
        yf_client: yfinance_client,
    };

    tokio::spawn(
        BymaTickerHistoricalDataPersistor::persist_historical_price_tickers(dc_state.clone()),
    );

    tokio::spawn(CommoditiesHistoricalPersistor::persist_commodities_historical(dc_state.clone()));
    let cors = CorsLayer::new()
        .allow_origin(frontend_url.parse::<http::HeaderValue>().unwrap())
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any);
    let app = Router::new().merge(data_collector::endpoints::data_collector_router(dc_state)).layer(cors);

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

    if let Err(e) = axum::serve(listener, app).await {
        return Err(DataCollectorError::AxumServeError(e));
    }

    Ok(())
}
