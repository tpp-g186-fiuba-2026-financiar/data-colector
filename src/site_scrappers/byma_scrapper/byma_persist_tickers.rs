use std::sync::Arc;

use sqlx::PgPool;

use crate::{
    errors::project_errors::DataCollectorError, persistence::ticker_repository,
    site_scrappers::byma_scrapper::byma_session::BymaScrapper,
};

pub struct BymaTickersPersistor;

impl BymaTickersPersistor {
    pub async fn persist_available_tickers(
        sqlx_pool: PgPool,
    ) -> Result<(), DataCollectorError<'static>> {
        let arc_sql_pool = Arc::new(sqlx_pool);
        loop {
            let byma_scrapper: BymaScrapper = match BymaScrapper::new().await {
                Ok(scrapper) => scrapper,
                Err(e) => {
                    eprintln!(
                        "[Data Collector] Failed to create BymaScrapper: {} — retrying in 60s",
                        e
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                    return Err(e);
                }
            };

            println!("[Data Collector] Fetching tickers information from Byma...");
            match byma_scrapper.get_all_available_tickers().await {
                Ok(quotes) => {
                    match ticker_repository::persist_quotes(arc_sql_pool.clone(), &quotes).await {
                        Ok(inserted) => println!(
                            "[Data Collector] Persisted {} quote rows ({} fetched)",
                            inserted,
                            quotes.len()
                        ),
                        Err(e) => eprintln!("[Data Collector] Failed to persist quotes: {}", e),
                    }

                    match ticker_repository::persist_bid_offers_historical(
                        arc_sql_pool.clone(),
                        &quotes,
                    )
                    .await
                    {
                        Ok((inserted, updated)) => println!(
                            "[Data Collector] Persisted {} bid/offer historical rows | Updated {} rows ({} fetched)",
                            inserted,
                            updated,
                            quotes.len()
                        ),
                        Err(e) => eprintln!(
                            "[Data Collector] Failed to persist bid/offer historical data: {}",
                            e
                        ),
                    }
                }
                Err(e) => {
                    let msg = format!("{}", e);
                    eprintln!("{}", msg);
                }
            }

            let time_to_sleep = 60 * 60; // 60 minutes in seconds
            tokio::time::sleep(tokio::time::Duration::from_secs(time_to_sleep)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use chrono::Timelike;
    use sqlx::PgPool;

    use crate::site_scrappers::byma_scrapper::byma_persist_tickers::BymaTickersPersistor;

    fn is_byma_opendata_available() -> bool {
        let now = chrono::Utc::now() - chrono::Duration::hours(3);
        let hour = now.hour();
        hour >= 10 && hour <= 18
    }

    #[tokio::test]
    async fn test_persist_available_tickers() {
        // lets create a dummy sqlx pool for testing purposes. In a real test, you would use a test database.
        let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set for tests");
        let sqlx_pool = PgPool::connect(&database_url)
            .await
            .expect("Failed to connect to the database");

        match is_byma_opendata_available() {
            true => {
                // We must check if BymaTickersPersistor throws an error, IT MUST not throw an error.
                assert_eq!(
                    BymaTickersPersistor::persist_available_tickers(sqlx_pool)
                        .await
                        .is_ok(),
                    true
                );
            }
            false => {
                println!(
                    "Byma open data is not available, test_persist_available_tickers will be skipped..."
                );
                return;
            }
        }
    }
}
