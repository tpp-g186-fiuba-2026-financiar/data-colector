use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::errors::project_errors::DataCollectorError;

#[derive(Debug, Clone)]
pub struct TickerQuote {
    pub symbol: String,
    pub market: String,
    pub offered_price: f64,
    pub bid_price: f64,
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
            /*(
                "general-equity",
                "https://open.bymadata.com.ar/vanoms-be-core/rest/api/bymadata/free/general-equity",
            ),*/
            (
                "leading-equity",
                "https://open.bymadata.com.ar/vanoms-be-core/rest/api/bymadata/free/leading-equity",
            ),
            /*(
                "cedears",
                "https://open.bymadata.com.ar/vanoms-be-core/rest/api/bymadata/free/cedears",
            ),*/
        ];

        #[derive(Serialize, Deserialize, Debug)]
        struct TickersResponse {
            symbol: String,
            #[serde(rename = "offerPrice")]
            offered_price: f64,
            #[serde(rename = "bidPrice")]
            bid_price: f64,
        }

        #[allow(clippy::type_complexity)]
        let mut handles: Vec<JoinHandle<Result<(Vec<TickerQuote>, String), String>>> = Vec::new();

        for (market, url) in urls_data {
            let client_clone = self.client.clone();
            let url_string = url.to_string();
            let market_string = market.to_string();

            let handle: JoinHandle<Result<(Vec<TickerQuote>, String), String>> =
                tokio::spawn(async move {
                    let payload: serde_json::Value = serde_json::json!({
                        "excludeZeroPxAndQty": true,
                        "T2": false,
                        "T1": true,
                        "T0": false,
                        "Content-Type": "application/json",
                    });

                    let response = match client_clone.post(&*url_string).json(&payload).send().await
                    {
                        Ok(resp) => resp,
                        Err(e) => {
                            eprintln!("Failed to send request to {}: {}", url_string, e);
                            return Err(market.to_string());
                        }
                    };

                    if !response.status().is_success() {
                        eprintln!(
                            "Received non-success status code {} from {}",
                            response.status(),
                            url_string
                        );
                        return Err(market.to_string());
                    }

                    let site_response: serde_json::Value = match response.json().await {
                        Ok(json) => json,
                        Err(e) => {
                            eprintln!("Failed to parse JSON response from {}: {}", url_string, e);
                            return Err(market.to_string());
                        }
                    };

                    let tickers_data: Vec<TickersResponse> = match site_response["data"].as_array()
                    {
                        Some(data) => data
                            .iter()
                            .filter_map(|item| serde_json::from_value(item.clone()).ok())
                            .collect(),
                        None => {
                            // Cedears endpoint returns as object of Tickersdata so we need to get them from there or else
                            // throw error

                            let response_as_vec_object: Vec<serde_json::Value> =
                                match site_response.as_array() {
                                    Some(data) => data.clone(),
                                    None => {
                                        eprintln!(
                                            "Expected response to be an array in response from {}",
                                            url_string
                                        );
                                        return Err(market.to_string());
                                    }
                                };

                            response_as_vec_object
                                .into_iter()
                                .filter_map(|item| serde_json::from_value(item).ok())
                                .collect()
                        }
                    };

                    // lets get GMT -3 timezone for Argentina
                    let recorded_at = Utc::now() - chrono::Duration::hours(3);

                    Ok((
                        tickers_data
                            .into_iter()
                            .filter(|t| t.offered_price != 0.0 && t.bid_price != 0.0)
                            .map(|t| TickerQuote {
                                symbol: t.symbol,
                                market: market_string.clone(),
                                offered_price: t.offered_price,
                                bid_price: t.bid_price,
                                recorded_at,
                            })
                            .collect(),
                        market_string,
                    ))
                });

            handles.push(handle);

            tokio::time::sleep(Duration::from_millis(650)).await;
        }

        let mut quotes: Vec<TickerQuote> = Vec::new();

        for handle in handles {
            match handle.await {
                Ok(opt_batch) => match opt_batch {
                    Ok((batch, site)) => {
                        eprintln!(
                            "[Data Collector] Fetched {} tickers in this {}!",
                            batch.len(),
                            site
                        );
                        quotes.append(&mut batch.clone());
                    }
                    Err(market_err) => {
                        let msg = format!(
                            "[Data Collector] Endpoint from {} is returning no information",
                            market_err
                        );
                        eprintln!("{}", msg);
                        return Err(DataCollectorError::BymaScrapperError(Box::leak(
                            msg.into_boxed_str(),
                        )));
                    }
                },
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
                "Failed to fetch any tickers from BYMA (No data)",
            )),
            false => Ok(quotes),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::byma_scrapper::byma_session::BymaScrapper;
    use chrono::Timelike;
    use tokio::runtime::Runtime;

    fn is_byma_opendata_available() -> bool {
        let now = chrono::Utc::now() - chrono::Duration::hours(3);
        let hour = now.hour();
        hour >= 10 && hour <= 18
    }
    #[test]
    fn test_get_all_available_tickers() {
        if !is_byma_opendata_available() {
            eprintln!("BYMA open data is not available at this time. Skipping test.");
            return;
        }

        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let scrapper = BymaScrapper::new().await.unwrap();
            let tickers = scrapper.get_all_available_tickers().await.unwrap();
            assert!(!tickers.is_empty());
        });
    }
}
