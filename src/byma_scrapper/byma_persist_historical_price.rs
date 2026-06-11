use std::sync::Arc;

use rand::Rng;
use sqlx::Row;

use crate::endpoints::DCState;
use crate::persistence::ticker_repository::{self, TickerHistoricalData};

pub struct BymaTickerHistoricalDataPersistor;

impl BymaTickerHistoricalDataPersistor {
    pub async fn persist_historical_price_tickers(dc_state: DCState) {
        let pool = Arc::new(dc_state.sqlx_pool);
        let yfinance_client = Arc::new(dc_state.yf_client);

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
                .range(yfinance_rs::Range::Max)
                .symbols(
                    tickers
                        .iter()
                        .map(|s| format!("{}.BA", s))
                        .collect::<Vec<String>>(),
                );

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
                    eprintln!(
                        "[Data-Collector-Historical-Task] Failed to download historical data from yfinance for tickers {:?}: {}",
                        tickers, e
                    );
                }
            }

            let random_timeout = rand::thread_rng().gen_range(2..=4);
            tokio::time::sleep(std::time::Duration::from_secs(random_timeout * 60)).await;
        }
    }
}
