use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::errors::project_errors::DataCollectorError;

pub struct BymaScrapper {
    pub client: reqwest::Client,
}

impl BymaScrapper {
    pub async fn new() -> Result<Self, DataCollectorError<'static>> {
        let cookies_jar = Arc::new(reqwest::cookie::Jar::default());

        let headers = {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::USER_AGENT,
                reqwest::header::HeaderValue::from_static(
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36",
                ),
            );
            headers
        };

        let initial_client_request = reqwest::Client::builder()
            .cookie_provider(cookies_jar.clone())
            .danger_accept_invalid_certs(true)
            .default_headers(headers)
            .build()
            .expect("Failed to build initial client");

        let initial_request = initial_client_request
            .get("https://open.bymadata.com.ar/#/dashboard")
            .send()
            .await
            .expect("Failed to send initial request");

        if !initial_request.status().is_success() {
            return Err(DataCollectorError::BymaScrapperError(
                "Failed to create initial client that request to byma dashboard",
            ));
        };

        Ok(BymaScrapper {
            client: initial_client_request,
        })
    }

    pub async fn get_all_available_tickers(
        &self,
    ) -> Result<Vec<String>, DataCollectorError<'static>> {
        let urls_data = vec![
            "https://open.bymadata.com.ar//vanoms-be-core/rest/api/bymadata/free/general-equity",
            "https://open.bymadata.com.ar//vanoms-be-core/rest/api/bymadata/free/leading-equity",
        ];

        #[derive(Serialize, Deserialize, Debug)]
        struct TickersResponse {
            symbol: String,
            #[serde(rename = "offerPrice")]
            offered_price: f32,
            #[serde(rename = "openingPrice")]
            opening_price: f32,
        }

        let mut handles: Vec<JoinHandle<Vec<String>>> = Vec::new();

        for url in urls_data {
            let client_clone = self.client.clone();
            let url_string = url.to_string();

            let handle: JoinHandle<Vec<String>> = tokio::spawn(async move {
                let payload: serde_json::Value = serde_json::json!({
                    "excludeZeroPxAndQty": true,
                    "T2": false,
                    "T1": true,
                    "T0": false,
                    "Content-Type": "application/json",
                });

                let response = match client_clone.post(&*url).json(&payload).send().await {
                    Ok(resp) => resp,
                    Err(e) => {
                        eprintln!("Failed to send request to {}: {}", url_string, e);
                        return vec![];
                    }
                };

                if !response.status().is_success() {
                    eprintln!(
                        "Received non-success status code {} from {}",
                        response.status(),
                        url_string
                    );
                    return vec![];
                }

                let site_response: serde_json::Value = match response.json().await {
                    Ok(json) => json,
                    Err(e) => {
                        eprintln!("Failed to parse JSON response from {}: {}", url_string, e);
                        return vec![];
                    }
                };

                let site_data: Vec<serde_json::Value> = match site_response["data"].as_array() {
                    Some(data) => data.clone(),
                    None => {
                        eprintln!(
                            "Expected 'data' field to be an array in response from {}",
                            url_string
                        );
                        return vec![];
                    }
                };

                let tickers: Vec<TickersResponse> = site_data
                    .into_iter()
                    .filter_map(|item| serde_json::from_value(item).ok())
                    .collect();

                // now, we remove those tickers have opening price and offered price equal to 0, because those are not active tickers
                tickers
                    .into_iter()
                    .filter(|ticker| ticker.opening_price != 0.0 && ticker.offered_price != 0.0)
                    .map(|ticker| ticker.symbol)
                    .collect()
            });

            handles.push(handle);
        }

        let mut tickers: Vec<String> = Vec::new();

        for handle in handles {
            match handle.await {
                Ok(mut symbols) => tickers.append(&mut symbols),
                Err(e) => {
                    let text = format!("Failed to join task for fetching tickers: {}", e);
                    return Err(DataCollectorError::BymaScrapperError(Box::leak(
                        text.into_boxed_str(),
                    )));
                }
            }
        }

        match tickers.is_empty() {
            true => Err(DataCollectorError::BymaScrapperError(
                "Failed to fetch any tickers from Byma",
            )),
            false => Ok(tickers),
        }
    }
}
