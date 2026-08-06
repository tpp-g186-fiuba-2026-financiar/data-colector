use std::sync::Arc;

use sqlx::PgPool;

use crate::{
    byma_scrapper::byma_session::BymaScrapper, errors::project_errors::DataCollectorError,
    persistence::ticker_repository,
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
