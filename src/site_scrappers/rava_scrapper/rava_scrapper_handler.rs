use std::{collections::HashMap, sync::Arc};

use sqlx::PgPool;
use tokio::task::JoinHandle;

use crate::{
    errors::project_errors::DataCollectorError,
    persistence::ticker_repository,
    site_scrappers::rava_scrapper::rava_structures_responses::{
        ItemData, ItemDescriptionData, PriceData, RavaClasificacionResponse,
        RavaHistoricalResponse, RavaRefDataResponse,
    },
};
pub struct RavaFetcher;

const BASE_RAVA_URL: &str = "https://mercado.rava.com";

impl RavaFetcher {
    pub async fn fetch_rava_tickers(sqlx_pool: PgPool) -> Result<(), DataCollectorError<'static>> {
        let arc_sql_pool = Arc::new(sqlx_pool);

        loop {
            let (classification_tickers, ref_data_tickers) = tokio::join!(
                RavaFetcher::get_rava_classification_tickers(BASE_RAVA_URL),
                RavaFetcher::get_rava_ref_data_tickers(BASE_RAVA_URL)
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
                merval_reference_data_tickers_hash
            );

            let historical_prices = match Self::fetch_historical_prices_for_rava_tickers(
                BASE_RAVA_URL,
                &merval_tickers,
            )
            .await
            {
                Ok(prices) => prices,
                Err(e) => {
                    eprintln!(
                        "[Data Collector] Failed to fetch historical prices: {:?}",
                        e
                    );
                    return Err(DataCollectorError::RavaScrapperError(
                        "Failed to fetch historical prices for Rava tickers",
                    ));
                }
            };

            match ticker_repository::persist_rava_historical_prices(
                arc_sql_pool.clone(),
                &historical_prices,
            )
            .await
            {
                Ok(_) => {}
                Err(e) => {
                    eprintln!(
                        "[Data Collector] Failed to persist historical prices: {:?}",
                        e
                    );
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(60 * 60 * 10)).await;
        }
    }

    pub async fn get_rava_classification_tickers(
        site_domain: &str,
    ) -> Result<HashMap<String, ItemData>, DataCollectorError<'static>> {
        let classification_request = reqwest::Client::new()
            .get(format!("{}/api/prices/clasificacion", site_domain))
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

    pub async fn get_rava_ref_data_tickers(
        site_domain: &str,
    ) -> Result<HashMap<String, ItemDescriptionData>, DataCollectorError<'static>> {
        let ref_data_request = reqwest::Client::new()
            .get(format!("{}/api/prices/refdata", site_domain))
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

    async fn fetch_historical_prices_for_rava_tickers(
        site_domain: &str,
        tickers: &[String],
    ) -> Result<HashMap<String, Vec<PriceData>>, DataCollectorError<'static>> {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(2));

        type JoinHandleRavaHistoricalPrices =
            Result<HashMap<String, Vec<PriceData>>, DataCollectorError<'static>>;

        let mut handles: Vec<JoinHandle<JoinHandleRavaHistoricalPrices>> = Vec::new();

        let client = reqwest::Client::new();

        for ticker in tickers {
            let ticker = ticker.clone();
            let semaphore_clone = semaphore.clone();
            let client = client.clone();
            let site_domain = site_domain.to_string();
            let handle = tokio::spawn(async move {
                let url = format!(
                    "{}/api/prices/historico/arg/{}?dias=4435",
                    site_domain, ticker
                );
                let _permit = semaphore_clone.acquire().await.unwrap();

                let response = match client.get(&url).send().await {
                    Ok(resp) => resp,
                    Err(e) => {
                        let error_message =
                            format!("Failed to fetch historical prices for {}: {:?}", ticker, e);
                        eprintln!("[Data Collector] {}", error_message);
                        return Err(DataCollectorError::RavaScrapperError(
                            "Failed to fetch historical prices for ticker",
                        ));
                    }
                };

                let text = match response.text().await {
                    Ok(text) => text,
                    Err(e) => {
                        let error_message =
                            format!("Failed to read response text for {}: {:?}", ticker, e);
                        eprintln!("[Data Collector] {}", error_message);
                        return Err(DataCollectorError::RavaScrapperError(
                            "Failed to read response text for ticker",
                        ));
                    }
                };

                let response_json: RavaHistoricalResponse = match serde_json::from_str(&text) {
                    Ok(data) => data,
                    Err(e) => {
                        let error_message =
                            format!("Failed to parse historical prices for {}: {:?}", ticker, e);
                        eprintln!(
                            "[Data Collector] {} {:?}",
                            error_message,
                            &text[..std::cmp::min(text.len(), 200)]
                        );
                        return Err(DataCollectorError::RavaScrapperError(
                            "Failed to parse historical prices for ticker",
                        ));
                    }
                };

                let mut result = HashMap::new();
                result.insert(response_json.simbolo, response_json.datos);
                Ok(result)
            });
            handles.push(handle);
        }

        let mut output_hashmap: HashMap<String, Vec<PriceData>> = HashMap::new();

        for handle in handles {
            if let Ok(Ok(ticker_prices)) = handle.await {
                for (ticker, prices) in ticker_prices.clone() {
                    output_hashmap.insert(ticker.clone(), prices.clone());
                    println!(
                        "[Data Collector] Rava Ticker {} found {} historical prices",
                        ticker,
                        prices.len()
                    );
                }
            } else {
                eprintln!("[Data Collector] Failed to fetch historical prices for a ticker");
            }
        }

        Ok(output_hashmap)
    }
}

#[cfg(test)]
mod test {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use crate::site_scrappers::rava_scrapper::{
        MOCK_BYMA_HISTORICAL_JSON, MOCK_REFDATA_JSON, rava_scrapper_handler::RavaFetcher,
    };

    #[tokio::test]
    async fn get_rava_ref_data_tickers_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/prices/refdata"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(MOCK_REFDATA_JSON, "application/json"),
            )
            .mount(&server)
            .await;

        let result = RavaFetcher::get_rava_ref_data_tickers(&server.uri())
            .await
            .unwrap();

        assert!(result.contains_key("arg:AAPL"));
        assert!(result.contains_key("arg:A30C80000J"));
        assert_eq!(result.keys().len(), 4);
    }

    #[tokio::test]
    async fn get_rava_classification_tickers_http_500_should_throw_err() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/prices/clasificacion"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        // This request is made, either fails or it has a wrong data it SHOULD throw error as type DataCollectorError::RavaScrapperError
        let result = RavaFetcher::get_rava_classification_tickers(&server.uri()).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_rava_classification_tickers_malformed_json_should_throw_err() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/prices/clasificacion"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw("{not valid json", "application/json"),
            )
            .mount(&server)
            .await;

        let result = RavaFetcher::get_rava_classification_tickers(&server.uri()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_rava_classification_tickers_empty_field_datos_is_ok_but_empty_map() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/prices/clasificacion"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(r#"{"datos":{}}"#, "application/json"),
            )
            .mount(&server)
            .await;

        let result = RavaFetcher::get_rava_classification_tickers(&server.uri())
            .await
            .unwrap();

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn fetch_historical_prices_single_ticker_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/prices/historico/arg/BYMA"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(MOCK_BYMA_HISTORICAL_JSON, "application/json"),
            )
            .mount(&server)
            .await;

        let tickers = vec!["BYMA".to_string()];
        const TOTAL_PRICES_AVAILABLE_FOR_BYMA: usize = 2;
        let server_uri = server.uri();
        let result = RavaFetcher::fetch_historical_prices_for_rava_tickers(&server_uri, &tickers)
            .await
            .unwrap();

        assert!(result.contains_key("BYMA"));

        assert_eq!(result["BYMA"].len(), TOTAL_PRICES_AVAILABLE_FOR_BYMA);
    }
}
