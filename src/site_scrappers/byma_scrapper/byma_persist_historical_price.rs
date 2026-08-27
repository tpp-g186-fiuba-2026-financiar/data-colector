use std::sync::Arc;

use rand::Rng;
use regex::Regex;
use sqlx::Row;

use crate::endpoints::DCState;
use crate::persistence::ticker_repository::{self, TickerHistoricalData};

pub struct BymaTickerHistoricalDataPersistor;

impl BymaTickerHistoricalDataPersistor {
    pub async fn persist_historical_price_tickers(dc_state: DCState) {
        let pool = Arc::new(dc_state.sqlx_pool);
        let yfinance_client = Arc::new(dc_state.yf_client);

        let re_expression_ticker = Regex::new(r"chart/(.+).BA\?").unwrap();

        loop {
            // Lets get the tickers from the database SORT by market and limited to 5.
            // last_history_price_cached_at should either be null or older than 5 days.

            let arc_yfinance_client = Arc::clone(&yfinance_client);
            let arc_pool = Arc::clone(&pool);

            let five_days_ago = chrono::Utc::now() - chrono::Duration::days(5);
            let query = format!(
                "SELECT symbol FROM available_tickers_byma WHERE market <> 'COMMODITY' AND (last_history_price_cached_at IS NULL OR last_history_price_cached_at < to_timestamp({})) ORDER BY market DESC LIMIT 8",
                five_days_ago.timestamp()
            );
            let tickers: Vec<String> = match sqlx::query(&query).fetch_all(&*arc_pool).await {
                Ok(tickers) => {
                    let tickers: Vec<String> = tickers
                        .into_iter()
                        .filter_map(|row| row.get("symbol"))
                        .collect();

                    if tickers.is_empty() {
                        println!(
                            "[Data-Collector-Historical-Task] No tickers found that need historical data update. Sleeping for 5 hours before checking again."
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(5 * 60 * 60)).await; // Sleep for 5 hours
                        continue;
                    }
                    tickers
                }
                Err(e) => {
                    eprintln!(
                        "[Data-Collector-Historical-Task] Failed to fetch tickers from database: {}",
                        e
                    );
                    continue; // Skip this iteration and try again after the sleep
                }
            };
            println!(
                "[Data-Collector-Historical-Task] Trying to update historical data for: {:?}",
                tickers
            );

            // now, given this tickers, let's fetch from yfinance downloader and convert it to TickerHistoricalData.

            let yfinance_downloader = yfinance_rs::DownloadBuilder::new(&arc_yfinance_client)
                .interval(yfinance_rs::Interval::D1)
                // Range::Max + D1 hace que Yahoo degrade la granularidad a velas
                // mensuales. Y10 fuerza data diaria (~2400 ruedas) que es lo que
                // necesitan los modelos. Ver api-ml (modelo LSTM entrena con diarias).
                .range(yfinance_rs::Range::Y10)
                .symbols(
                    tickers
                        .iter()
                        .map(|s| format!("{}.BA", s))
                        .collect::<Vec<String>>(),
                );

            // [Data-Collector-Historical-Task] Failed to download historical data from yfinance for tickers ["METRC", "GBAN", "BMA.C", "CVH", "LONG", "BYMAC", "LOMAC", "DOME"]: Not found at https://query1.finance.yahoo.com/v8/finance/chart/BMA.C.BA?range=10y&interval=1d&events=div%7Csplit%7CcapitalGains&includePrePost=false

            match yfinance_downloader.run().await {
                Ok(data) => {
                    let entries = data.entries;
                    let pool_reference = Arc::clone(&arc_pool);

                    for entry in entries {
                        // 1. If entry failed entirely on Yahoo's side, skip it gracefully
                        let candles = entry.history.candles;

                        // 2. Clean the symbol string upfront (e.g., "LOMA.BA" -> "LOMA")
                        let raw_symbol = entry.instrument.symbol.to_string();
                        let clean_symbol = raw_symbol.replace(".BA", "");

                        // 3. Map the candles using the CLEAN symbol name
                        let historical_data_rows: Vec<TickerHistoricalData> = candles
                            .iter()
                            .map(|candle| TickerHistoricalData::from_candle(candle, &clean_symbol))
                            .collect();

                        if historical_data_rows.is_empty() {
                            continue;
                        }

                        // 4. Pass the clean symbol and matching data rows to your repository
                        match ticker_repository::update_historical_data(
                            (*pool_reference).clone(),
                            &clean_symbol,
                            historical_data_rows,
                        )
                        .await
                        {
                            Ok(_) => println!(
                                "[Data-Collector] Successfully updated historical data for {}",
                                clean_symbol
                            ),
                            Err(e) => eprintln!(
                                "[Data-Collector] Critical error executing DB persist for {}: {}",
                                clean_symbol, e
                            ),
                        }
                    }
                }
                Err(e) => {
                    let error_as_string = e.to_string();

                    match re_expression_ticker.captures(&error_as_string) {
                        Some(captures) => {
                            if let Some(ticker_match) = captures.get(1) {
                                let ticker = ticker_match.as_str();
                                eprintln!(
                                    "[Data-Collector-Historical-Task] Failed to download historical data from yfinance for ticker {}: {}",
                                    ticker, e
                                );

                                // Lets remove this ticker now from the database
                                match ticker_repository::remove_ticker_from_available_tickers(
                                    (*pool).clone(),
                                    ticker,
                                )
                                .await
                                {
                                    Ok(_) => eprintln!(
                                        "[Data-Collector-Historical-Task] Successfully removed ticker {} from the main ticker list.",
                                        ticker
                                    ),
                                    Err(err) => eprintln!(
                                        "[Data-Collector-Historical-Task] Failed to remove ticker {} from the main ticker list: {}",
                                        ticker, err
                                    ),
                                }
                            } else {
                                eprintln!(
                                    "[Data-Collector-Historical-Task] Failed to download historical data from yfinance: {}",
                                    e
                                );
                                eprintln!(
                                    "[Data-Collector-Historical-Task] We also weren't able to remove the ticker from the main ticker list: {}",
                                    error_as_string
                                );
                            }
                        }
                        None => {
                            eprintln!(
                                "[Data-Collector-Historical-Task] Failed to download historical data from yfinance: {}",
                                e
                            );
                            eprintln!(
                                "[Data-Collector-Historical-Task] We also weren't able to remove the ticker from the main ticker list: {}",
                                error_as_string
                            );
                        }
                    }
                }
            }

            let random_timeout = rand::thread_rng().gen_range(2..=4);
            tokio::time::sleep(std::time::Duration::from_secs(random_timeout * 60)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use regex::Regex;

    #[test]
    fn test_yfinance_error_ticker_extraction_regex() {
        let re_expression_ticker = Regex::new(r"chart/(.+).BA\?").unwrap();

        let error_message = "Failed to download historical data from yfinance for tickers: Not found at https://query1.finance.yahoo.com/v8/finance/chart/BMA.C.BA?range=10y&interval=1d";

        let captures = re_expression_ticker.captures(error_message);
        assert!(captures.is_some());

        let ticker_match = captures.unwrap().get(1).map(|m| m.as_str());
        assert_eq!(ticker_match, Some("BMA.C"));
    }
}
