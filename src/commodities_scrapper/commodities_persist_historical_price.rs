use std::sync::Arc;

use sqlx::Row;
use yfinance_rs::{Range, Ticker};

use crate::endpoints::DCState;
use crate::persistence::ticker_repository::{self, TickerHistoricalData};

const COMMODITY_MARKET: &str = "COMMODITY";

// (clean symbol stored in our DB, yahoo finance symbol)
const COMMODITIES: &[(&str, &str)] = &[("GOLD", "GC=F"), ("OIL", "CL=F")];

pub struct CommoditiesHistoricalPersistor;

impl CommoditiesHistoricalPersistor {
    pub async fn persist_commodities_historical(dc_state: DCState) {
        let pool = Arc::new(dc_state.sqlx_pool);
        let yfinance_client = Arc::new(dc_state.yf_client);

        // Seed available_tickers_byma with commodity entries so the FK from
        // ticker_history_data_cached_yf can resolve. Idempotent on conflict.
        for (symbol, _) in COMMODITIES {
            if let Err(e) = sqlx::query(
                r#"
                INSERT INTO available_tickers_byma (symbol, market)
                VALUES ($1, $2)
                ON CONFLICT (symbol) DO NOTHING
                "#,
            )
            .bind(symbol)
            .bind(COMMODITY_MARKET)
            .execute(&*pool)
            .await
            {
                eprintln!(
                    "[Data-Collector-Commodities] Failed to seed commodity {}: {}",
                    symbol, e
                );
            }
        }

        loop {
            let five_days_ago = chrono::Utc::now() - chrono::Duration::days(5);

            // Pick only the commodity tickers whose historical cache is stale.
            let stale_query = format!(
                "SELECT symbol FROM available_tickers_byma \
                 WHERE market = '{}' \
                 AND (last_history_price_cached_at IS NULL OR last_history_price_cached_at < to_timestamp({}))",
                COMMODITY_MARKET,
                five_days_ago.timestamp()
            );

            let stale_symbols: Vec<String> = match sqlx::query(&stale_query)
                .fetch_all(&*pool)
                .await
            {
                Ok(rows) => rows
                    .into_iter()
                    .filter_map(|row| row.get("symbol"))
                    .collect(),
                Err(e) => {
                    eprintln!(
                        "[Data-Collector-Commodities] Failed to query stale commodity tickers: {}",
                        e
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(60 * 60)).await;
                    continue;
                }
            };

            if stale_symbols.is_empty() {
                println!(
                    "[Data-Collector-Commodities] No commodities need an update. Sleeping for 6 hours."
                );
                tokio::time::sleep(std::time::Duration::from_secs(6 * 60 * 60)).await;
                continue;
            }

            for clean_symbol in &stale_symbols {
                let yf_symbol = match COMMODITIES
                    .iter()
                    .find(|(s, _)| *s == clean_symbol.as_str())
                    .map(|(_, yf)| *yf)
                {
                    Some(yf) => yf,
                    None => {
                        eprintln!(
                            "[Data-Collector-Commodities] No yahoo symbol mapping for {}",
                            clean_symbol
                        );
                        continue;
                    }
                };

                println!(
                    "[Data-Collector-Commodities] Fetching historical data for {} ({})",
                    clean_symbol, yf_symbol
                );

                let yfinance_ticker = Ticker::new(&*yfinance_client, yf_symbol.to_string());

                let candles = match yfinance_ticker
                    .history(Some(Range::Max), Some(yfinance_rs::Interval::D1), false)
                    .await
                {
                    Ok(candles) => candles,
                    Err(e) => {
                        eprintln!(
                            "[Data-Collector-Commodities] Failed to download {} from yfinance: {}",
                            clean_symbol, e
                        );
                        continue;
                    }
                };

                let historical_data_rows: Vec<TickerHistoricalData> = candles
                    .iter()
                    .map(|candle| TickerHistoricalData::from_candle(candle, clean_symbol))
                    .collect();

                if historical_data_rows.is_empty() {
                    println!(
                        "[Data-Collector-Commodities] Empty history for {}, skipping persist",
                        clean_symbol
                    );
                    continue;
                }

                match ticker_repository::update_historical_data(
                    (*pool).clone(),
                    clean_symbol,
                    historical_data_rows,
                )
                .await
                {
                    Ok(_) => println!(
                        "[Data-Collector-Commodities] Successfully updated historical data for {}",
                        clean_symbol
                    ),
                    Err(e) => eprintln!(
                        "[Data-Collector-Commodities] DB persist failed for {}: {}",
                        clean_symbol, e
                    ),
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(6 * 60 * 60)).await;
        }
    }
}
