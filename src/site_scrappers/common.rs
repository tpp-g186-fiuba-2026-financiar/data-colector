use std::{collections::HashSet, sync::Arc};

use crate::{
    endpoints::DCState, errors::project_errors::DataCollectorError, persistence::ticker_repository,
};

pub struct CommonScrapper;

impl CommonScrapper {
    pub async fn persist_sector_which_ticker_belongs(
        dc_state: DCState,
    ) -> Result<(), DataCollectorError<'static>> {
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
        // lets create a unique set of sectors from both sources
        let mut sectors_set: HashSet<String> = HashSet::new();
        sectors_set.extend(tickers_openbymadata.unwrap_or_default());
        sectors_set.extend(tickers_rava.unwrap_or_default());
        /*let vec_tickers = sectors_set.into_iter().collect::<Vec<String>>();

        // Lets create yfinance Screener
        let yfinance_client = dc_state.yf_client.clone();*/

        Ok(())
    }
}
