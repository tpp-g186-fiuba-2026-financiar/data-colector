use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::errors::project_errors::DataCollectorError;

#[derive(Debug, Clone)]
pub struct TickerQuote {
    pub symbol: String,
    pub market: String,
    pub opening_price: f64,
    pub offered_price: f64,
    pub recorded_at: DateTime<Utc>,
}

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
    ) -> Result<Vec<TickerQuote>, DataCollectorError<'static>> {
        let urls_data = vec![
            (
                "general-equity",
                "https://open.bymadata.com.ar//vanoms-be-core/rest/api/bymadata/free/general-equity",
            ),
            (
                "leading-equity",
                "https://open.bymadata.com.ar//vanoms-be-core/rest/api/bymadata/free/leading-equity",
            ),
        ];

        #[derive(Serialize, Deserialize, Debug)]
        struct TickersResponse {
            symbol: String,
            #[serde(rename = "offerPrice")]
            offered_price: f64,
            #[serde(rename = "openingPrice")]
            opening_price: f64,
        }

        let mut handles: Vec<JoinHandle<Vec<TickerQuote>>> = Vec::new();

        for (market, url) in urls_data {
            let client_clone = self.client.clone();
            let url_string = url.to_string();
            let market_string = market.to_string();

            let handle: JoinHandle<Vec<TickerQuote>> = tokio::spawn(async move {
                let payload: serde_json::Value = serde_json::json!({
                    "excludeZeroPxAndQty": true,
                    "T2": false,
                    "T1": true,
                    "T0": false,
                    "Content-Type": "application/json",
                });

                let response = match client_clone.post(&*url_string).json(&payload).send().await {
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

                // We take a single timestamp per fetch so all quotes in the same batch
                // share the same recorded_at — makes it easier to group snapshots by time.
                let recorded_at = Utc::now();

                // Drop tickers with zero opening/offered price (they are not actively traded).
                tickers
                    .into_iter()
                    .filter(|t| t.opening_price != 0.0 && t.offered_price != 0.0)
                    .map(|t| TickerQuote {
                        symbol: t.symbol,
                        market: market_string.clone(),
                        opening_price: t.opening_price,
                        offered_price: t.offered_price,
                        recorded_at,
                    })
                    .collect()
            });

            handles.push(handle);
        }

        let mut quotes: Vec<TickerQuote> = Vec::new();

        for handle in handles {
            match handle.await {
                Ok(mut batch) => quotes.append(&mut batch),
                Err(e) => {
                    let text = format!("Failed to join task for fetching tickers: {}", e);
                    return Err(DataCollectorError::BymaScrapperError(Box::leak(
                        text.into_boxed_str(),
                    )));
                }
            }
        }

        match quotes.is_empty() {
            true => Err(DataCollectorError::BymaScrapperError(
                "Failed to fetch any tickers from Byma",
            )),
            false => Ok(quotes),
        }
    }
}
