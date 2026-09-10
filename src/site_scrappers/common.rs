use std::{collections::HashSet, sync::Arc, time::Duration};

use tokio::{sync::Semaphore, task::JoinSet};
use yfinance_rs::profile::Profile::Company;

use crate::{
    endpoints::DCState, errors::project_errors::DataCollectorError, persistence::ticker_repository,
};

pub struct CommonScrapper;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TickerInformationFromDataCollector {
    pub ticker_symbol: String,
    pub is_commodity: bool,
    pub yfinance_ticker_name: String,
}

impl CommonScrapper {
    pub async fn persist_sector_which_ticker_belongs(
        dc_state: DCState,
    ) -> Result<(), DataCollectorError<'static>> {
        // The issue comes with the limitation of yfinance, so we wait a little longer in order to make this requests
        // The time is 6 minutes
        tokio::time::sleep(Duration::from_secs(6 * 60)).await;

        let sqlx_pool = Arc::new(dc_state.sqlx_pool.clone());

        let (tickers_openbymadata, tickers_rava) = tokio::join!(
            ticker_repository::get_tickers_openbymadata(sqlx_pool.clone()),
            ticker_repository::get_tickers_rava(sqlx_pool.clone()),
        );

        if let Err(e_openbyma) = &tickers_openbymadata
            && let Err(e_rava) = &tickers_rava
        {
            eprintln!(
                "[Data Collector] Failed to fetch tickers from both sources: {} | {}",
                e_openbyma, e_rava
            );
            return Err(DataCollectorError::PostgresQueryError(
                sqlx::Error::Protocol("Not found any tickers!".into()),
            ));
        }

        let mut sectors_set: HashSet<TickerInformationFromDataCollector> = HashSet::new();
        sectors_set.extend(tickers_openbymadata.unwrap_or_default());
        sectors_set.extend(tickers_rava.unwrap_or_default());

        let vec_tickers = sectors_set
            .into_iter()
            .collect::<Vec<TickerInformationFromDataCollector>>();

        let yfinance_client = Arc::new(dc_state.yf_client.clone());
        let semaphore = Arc::new(Semaphore::new(2));

        const CHUNK_SIZE: usize = 4;
        for chunk in vec_tickers.chunks(CHUNK_SIZE) {
            let mut join_set = JoinSet::new();

            for ticker_retrieved_from_db in chunk {
                let client = yfinance_client.clone();
                let sym = ticker_retrieved_from_db.ticker_symbol.clone();
                let ticker_curated = ticker_retrieved_from_db.yfinance_ticker_name.clone();
                let sem = semaphore.clone();

                join_set.spawn(async move {
                    // Acquire permit to throttle outgoing request volume
                    let _permit = sem.acquire().await.unwrap();

                    let ticker = yfinance_rs::Ticker::new(&client, ticker_curated.clone());

                    tokio::time::sleep(Duration::from_millis(200)).await;

                    match ticker.info().await {
                        Ok(info) => {
                            if let Some(Company(company_profile)) = info.profile {
                                match company_profile.sector {
                                    Some(sector) => Some((sym, Some(sector))),
                                    None => Some((sym, None)),
                                }
                            } else {
                                Some((sym, None))
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "[Data Collector] Failed to fetch profile for ticker {}: {}",
                                sym, e
                            );
                            None
                        }
                    }
                });
            }

            while let Some(res) = join_set.join_next().await {
                match res {
                    Ok(opt_retrieve_data) => match opt_retrieve_data {
                        Some((ticker_symbol, sector_opt)) => {
                            if let Some(sector) = sector_opt {
                                if let Err(e) = ticker_repository::insert_ticker_sector(
                                    sqlx_pool.clone(),
                                    &ticker_symbol,
                                    &sector,
                                )
                                .await
                                {
                                    eprintln!(
                                        "[Data Collector] Failed to insert sector for ticker '{}': {}",
                                        ticker_symbol, e
                                    );
                                } else {
                                    println!(
                                        "[Data Collector] Successfully inserted sector for ticker '{}': {}",
                                        ticker_symbol, sector
                                    );
                                }
                            } else {
                                eprintln!(
                                    "[Data Collector] No sector information available for ticker '{}'",
                                    ticker_symbol
                                );
                            }
                        }
                        None => {
                            eprintln!(
                                "[Data Collector] No data retrieved for a ticker in the chunk."
                            );
                        }
                    },
                    Err(e) => {
                        eprintln!(
                            "[Data Collector] Failed to join task for a ticker in the chunk: {}",
                            e
                        );
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        Ok(())
    }
}
