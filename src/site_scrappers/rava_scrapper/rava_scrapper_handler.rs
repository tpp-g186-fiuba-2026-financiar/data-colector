use std::{collections::HashMap, sync::Arc};

use serde::Deserialize;
use sqlx::PgPool;

use crate::{errors::project_errors::DataCollectorError, persistence::ticker_repository};
pub struct RavaFetcher;

#[derive(Deserialize, Debug)]
struct RavaClasificacionResponse {
    datos: HashMap<String, ItemData>,
}

#[derive(Deserialize, Debug)]
struct RavaRefDataResponse {
    datos: HashMap<String, ItemDescriptionData>,
}

#[derive(Deserialize, Debug)]
pub struct ItemData {
    pub st: String,
    pub sst: String,
    pub text: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ItemDescriptionData {
    #[serde(rename = "nc")]
    pub nombre_corto: String,
    #[serde(rename = "nl")]
    pub nombre_largo: String,
    #[serde(rename = "desc", default)]
    // it can be nul so we use Option
    pub descripcion: Option<String>,
}

impl RavaFetcher {
    pub async fn fetch_rava_tickers(sqlx_pool: PgPool) -> Result<(), DataCollectorError<'static>> {
        let arc_sql_pool = Arc::new(sqlx_pool);

        loop {
            let (classification_tickers, ref_data_tickers) = tokio::join!(
                RavaFetcher::get_rava_classification_tickers(),
                RavaFetcher::get_rava_ref_data_tickers()
            );

            if classification_tickers.is_err() || ref_data_tickers.is_err() {
                eprintln!(
                    "[Data Collector] Failed to fetch Rava tickers: classification: {:?}, ref_data: {:?} — retrying in 60s",
                    classification_tickers, ref_data_tickers
                );
                // lets try again in 2 hours
                tokio::time::sleep(tokio::time::Duration::from_secs(60 * 60 * 2)).await;
                continue;
            }
            let classification_tickers = classification_tickers.unwrap();
            let mut ref_data_tickers = ref_data_tickers.unwrap();

            // from classification tickers, lets keep those who are part from MERVAL (sst == "M")
            let merval_tickers: Vec<_> = classification_tickers
                .iter()
                .filter(|(_, item_data)| item_data.sst == "M")
                .map(|(ticker, _)| ticker.clone())
                .collect();

            // now, lets fix the ref_data_tickers removing the 'arg:' from the key
            let fixed_ref_data_tickers: HashMap<String, ItemDescriptionData> = ref_data_tickers
                .drain()
                .map(|(key, value)| {
                    let fixed_key = key.trim_start_matches("arg:").to_string();
                    (fixed_key, value)
                })
                .collect();

            // Now, lets keep the ref_data_tickers that are part of the merval_tickers
            let merval_reference_data_tickers_hash: HashMap<String, ItemDescriptionData> =
                fixed_ref_data_tickers
                    .into_iter()
                    .filter(|(ticker, _)| merval_tickers.contains(ticker))
                    .collect();

            // At this point, we have the merval tickers + the reference, so lets save them'.
            let _ = ticker_repository::persist_rava_tickers(
                arc_sql_pool.clone(),
                &merval_reference_data_tickers_hash,
            )
            .await;

            println!(
                "[Data Collector] Fetched and persisted Rava tickers: {:?}",
                merval_tickers
            );
            println!(
                "[Data Collector] Fetched and persisted Rava tickers: {:?}",
                merval_reference_data_tickers_hash.keys()
            );

            // Sleep for 10 hours, we don't need to fetch this data too often, and the API is rate limited.
            let sleep_for_1_min = tokio::time::Duration::from_secs(60);
            tokio::time::sleep(sleep_for_1_min).await;
            //tokio::time::sleep(tokio::time::Duration::from_secs(60 * 60 * 10)).await;
        }
    }

    pub async fn get_rava_classification_tickers()
    -> Result<HashMap<String, ItemData>, DataCollectorError<'static>> {
        let classification_request = reqwest::Client::new()
            .get("https://mercado.rava.com/api/prices/clasificacion")
            .send()
            .await;

        if let Ok(response) = classification_request
            && let Ok(data) = response.json::<RavaClasificacionResponse>().await
        {
            return Ok(data.datos);
        }

        Err(DataCollectorError::RavaScrapperError(
            "Failed to fetch Rava classification tickers",
        ))
    }

    pub async fn get_rava_ref_data_tickers()
    -> Result<HashMap<String, ItemDescriptionData>, DataCollectorError<'static>> {
        let ref_data_request = reqwest::Client::new()
            .get("https://mercado.rava.com/api/prices/refdata")
            .send()
            .await;

        if let Ok(response) = ref_data_request
            && let Ok(data) = response.json::<RavaRefDataResponse>().await
        {
            return Ok(data.datos);
        }

        Err(DataCollectorError::RavaScrapperError(
            "Failed to fetch Rava reference data tickers",
        ))
    }
}
