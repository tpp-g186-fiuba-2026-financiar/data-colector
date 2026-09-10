use std::{collections::HashMap, sync::Arc};

use chrono::TimeZone;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use yfinance_rs::Candle;

use crate::site_scrappers::{
    byma_scrapper::byma_session::TickerQuote,
    common::TickerInformationFromDataCollector,
    rava_scrapper::rava_structures_responses::{ItemDescriptionData, PriceData},
};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct TickerHistoricalData {
    pub ticker: String,
    pub ts: i64,
    pub volume: i64,
    pub open_amount: Decimal,
    pub high_amount: Decimal,
    pub low_amount: Decimal,
    pub close_amount: Decimal,
    pub close_unadj_amount: Decimal,
}

impl TickerHistoricalData {
    pub fn from_candle(candle: &Candle, ticker: &str) -> Self {
        let volume_int = candle.volume.unwrap_or(0) as i64;

        let unadj_price = match &candle.close_unadj {
            Some(price) => price.amount(),
            None => candle.close.amount(),
        };

        Self {
            ticker: ticker.to_string(),
            ts: candle.ts.timestamp_millis(),
            volume: volume_int,
            open_amount: candle.open.amount(),
            high_amount: candle.high.amount(),
            low_amount: candle.low.amount(),
            close_amount: candle.close.amount(),
            close_unadj_amount: unadj_price,
        }
    }
}

pub async fn persist_quotes(pool: Arc<PgPool>, quotes: &[TickerQuote]) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let mut catalog: HashMap<&str, &str> = HashMap::new();
    for q in quotes {
        catalog.insert(q.symbol.as_str(), q.market.as_str());
    }

    let symbols: Vec<String> = catalog.keys().map(|s| s.to_string()).collect();
    let markets: Vec<String> = symbols
        .iter()
        .map(|s| catalog.get(s.as_str()).copied().unwrap_or("").to_string())
        .collect();

    let rows: Vec<(i32, String)> = sqlx::query_as(
        r#"
        INSERT INTO available_tickers_byma (symbol, market)
        SELECT * FROM UNNEST($1::text[], $2::text[])
        ON CONFLICT (symbol) DO UPDATE SET market = EXCLUDED.market
        RETURNING id, symbol
        "#,
    )
    .bind(&symbols)
    .bind(&markets)
    .fetch_all(&mut *tx)
    .await?;

    /*
    let symbol_to_id: HashMap<String, i32> = rows.into_iter().map(|(id, sym)| (sym, id)).collect();

    let mut ticker_ids: Vec<i32> = Vec::with_capacity(quotes.len());
    let mut recorded_ats: Vec<chrono::DateTime<chrono::Utc>> = Vec::with_capacity(quotes.len());
    let mut opening_prices: Vec<f64> = Vec::with_capacity(quotes.len());
    let mut offered_prices: Vec<f64> = Vec::with_capacity(quotes.len());

    for q in quotes {
        let Some(&ticker_id) = symbol_to_id.get(&q.symbol) else {
            continue;
        };
        ticker_ids.push(ticker_id);
        recorded_ats.push(q.recorded_at);
        opening_prices.push(q.opening_price);
        offered_prices.push(q.offered_price);
    }

    let result = sqlx::query(
        r#"
        INSERT INTO ticker_quotes (ticker_id, recorded_at, opening_price, offered_price)
        SELECT * FROM UNNEST($1::int4[], $2::timestamptz[], $3::float8[], $4::float8[])
        ON CONFLICT (ticker_id, recorded_at) DO NOTHING
        "#,
    )
    .bind(&ticker_ids)
    .bind(&recorded_ats)
    .bind(&opening_prices)
    .bind(&offered_prices)
    .execute(&mut *tx)
    .await?;*/

    tx.commit().await?;

    //Ok(result.rows_affected())
    Ok(rows.len() as u64)
}

/// Incorpora al catalogo un ticker descubierto por una consulta historica.
/// Esto permite que los jobs de modelos lo encuentren en `/available-tickers`.
pub async fn ensure_available_ticker(pool: &PgPool, symbol: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO available_tickers_byma (symbol, market)
        VALUES ($1, 'ON_DEMAND')
        ON CONFLICT (symbol) DO NOTHING
        "#,
    )
    .bind(symbol)
    .execute(pool)
    .await?;
    Ok(())
}

/// If there is historical data for the given ticker, returns an option.
/// If there is no historical data for the given ticker, returns None.
/// if there is historical data for the given ticker, returns a vector of TickerHistoricalData.
/// If there is an error while fetching the historical data from the database, returns an error.
pub async fn is_historical_data_available(
    pool: PgPool,
    ticker_symbol: &str,
) -> Result<Option<(Vec<TickerHistoricalData>, i64)>, sqlx::Error> {
    let result: Vec<TickerHistoricalData> = sqlx::query_as(
        r#"
        SELECT * FROM ticker_history_data_cached_yf WHERE ticker = $1
        "#,
    )
    .bind(ticker_symbol)
    .fetch_all(&pool)
    .await?;

    match result.is_empty() {
        true => Ok(None),
        false => {
            // since primary key is ts is ts, we can get the latest ts from the result vector
            let latest_ts = result.iter().map(|data| data.ts).max().unwrap_or(0);
            Ok(Some((result, latest_ts)))
        }
    }
}

/// Historico de un ticker ordenado por fecha ascendente (mas viejo primero).
/// A diferencia de `is_historical_data_available`, no trae el timestamp de
/// ultima actualizacion: esto es para leer datos ya cacheados tal cual estan,
/// no para decidir si hay que refrescarlos.
pub async fn fetch_ordered_history(
    pool: &PgPool,
    ticker_symbol: &str,
) -> Result<Vec<TickerHistoricalData>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT * FROM ticker_history_data_cached_yf WHERE ticker = $1 ORDER BY ts ASC
        "#,
    )
    .bind(ticker_symbol)
    .fetch_all(pool)
    .await
}

pub async fn update_historical_data(
    pool: PgPool,
    ticker_symbol: &str,
    new_data: Vec<TickerHistoricalData>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    // Delete old data for the ticker
    sqlx::query(
        r#"
        DELETE FROM ticker_history_data_cached_yf WHERE ticker = $1
        "#,
    )
    .bind(ticker_symbol)
    .execute(&mut *tx)
    .await?;

    // Insert new data for the ticker
    for data in new_data {
        sqlx::query(
            r#"
            INSERT INTO ticker_history_data_cached_yf (ticker, ts, volume, open_amount, high_amount, low_amount, close_amount, close_unadj_amount)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(&data.ticker)
        .bind(data.ts)
        .bind(data.volume)
        .bind(data.open_amount)
        .bind(data.high_amount)
        .bind(data.low_amount)
        .bind(data.close_amount)
        .bind(data.close_unadj_amount)
        .execute(&mut *tx)
        .await?;
    }

    // Now, lets add a update into last_history_price_cached_at from table available_tickers_byma

    sqlx::query(
        r#"
        UPDATE available_tickers_byma SET last_history_price_cached_at = NOW() WHERE symbol = $1
        "#,
    )
    .bind(ticker_symbol)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

pub async fn remove_ticker_from_available_tickers(
    pool: PgPool,
    ticker_symbol: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    // Delete the ticker from available_tickers_byma
    sqlx::query(
        r#"
        DELETE FROM available_tickers_byma WHERE symbol = $1
        "#,
    )
    .bind(ticker_symbol)
    .execute(&mut *tx)
    .await?;

    // Delete the historical data for the ticker
    sqlx::query(
        r#"
        DELETE FROM ticker_history_data_cached_yf WHERE ticker = $1
        "#,
    )
    .bind(ticker_symbol)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

pub async fn persist_bid_offers_historical(
    pool: Arc<PgPool>,
    quotes: &[TickerQuote],
) -> Result<(u64, u64), sqlx::Error> {
    let mut tx = pool.begin().await?;

    let mut inserted_count = 0;
    let mut updated_count = 0;

    // lets generate current time as only YYYY-MM-DD
    let current_time = chrono::Utc::now().date_naive();
    let current_time =
        TimeZone::from_utc_datetime(&chrono::Utc, &current_time.and_hms_opt(0, 0, 0).unwrap());

    for q in quotes {
        let result = sqlx::query(
            r#"
            INSERT INTO bid_offer_historical_prices (ticker, recorded_at, bid, offered)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (ticker, recorded_at) DO UPDATE SET
                bid = EXCLUDED.bid,
                offered = EXCLUDED.offered
            "#,
        )
        .bind(&q.symbol)
        .bind(current_time)
        .bind(q.bid_price)
        .bind(q.offered_price)
        .execute(&mut *tx)
        .await?;

        match result.rows_affected() {
            1 => inserted_count += 1,
            0 => updated_count += 1,
            _ => {}
        }
    }

    tx.commit().await?;

    Ok((inserted_count, updated_count))
}

pub async fn persist_rava_tickers(
    pool: Arc<PgPool>,
    merval_reference_data_tickers: &HashMap<String, ItemDescriptionData>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    for (ticker, item_data) in merval_reference_data_tickers {
        sqlx::query(
            r#"
            INSERT INTO rava_tickers (ticker, short_name, long_name, description)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (ticker) DO UPDATE SET
                short_name = EXCLUDED.short_name,
                long_name = EXCLUDED.long_name,
                description = EXCLUDED.description
            "#,
        )
        .bind(ticker)
        .bind(&item_data.nombre_corto)
        .bind(&item_data.nombre_largo)
        .bind(&item_data.descripcion)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(())
}

pub async fn persist_rava_historical_prices(
    pool: Arc<PgPool>,
    historical_prices: &HashMap<String, Vec<PriceData>>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    /* CREATE TABLE ticker_history (
    simbolo VARCHAR(50),
    fecha DATE,
    precio NUMERIC(10, 4),
    maximo NUMERIC(10, 4),
    minimo NUMERIC(10, 4),
    apertura NUMERIC(10, 4),
    volumen BIGINT,
    timestamp BIGINT,
    PRIMARY KEY (simbolo, fecha)
    ); */

    for (ticker, prices) in historical_prices {
        for price_data in prices {
            sqlx::query(
                r#"
                INSERT INTO rava_ticker_history (ticker, fecha, precio, maximo, minimo, apertura, volumen, timestamp)
                VALUES ($1, $2::date, $3, $4, $5, $6, $7, $8)
                ON CONFLICT (ticker, fecha) DO UPDATE SET
                    precio = EXCLUDED.precio,
                    maximo = EXCLUDED.maximo,
                    minimo = EXCLUDED.minimo,
                    apertura = EXCLUDED.apertura,
                    volumen = EXCLUDED.volumen,
                    timestamp = EXCLUDED.timestamp
                "#,
            )
            .bind(ticker)
            .bind(price_data.fecha.clone())
            .bind(price_data.precio)
            .bind(price_data.maximo)
            .bind(price_data.minimo)
            .bind(price_data.apertura)
            .bind(price_data.volumen)
            .bind(price_data.timestamp)
            .execute(&mut *tx)
            .await?;
        }
    }

    tx.commit().await?;

    Ok(())
}

pub async fn get_extended_info_for_ticker(
    pool: Arc<PgPool>,
    ticker: &str,
) -> Result<Option<ItemDescriptionData>, sqlx::Error> {
    let result: Option<ItemDescriptionData> = sqlx::query_as(
        r#"
        SELECT * FROM rava_tickers WHERE ticker = $1
        "#,
    )
    .bind(ticker)
    .fetch_optional(&*pool)
    .await?;

    Ok(result)
}

// Pair (ticker, is_commodity)
pub async fn get_tickers_openbymadata(
    pool: Arc<PgPool>,
) -> Result<Vec<TickerInformationFromDataCollector>, sqlx::Error> {
    let result: Vec<(String, String)> = sqlx::query_scalar(
        r#"
        SELECT symbol, market FROM available_tickers_byma
        "#,
    )
    .fetch_all(&*pool)
    .await?;

    let tickers_info: Vec<TickerInformationFromDataCollector> = result
        .into_iter()
        .map(|(symbol, market)| {
            let is_commodity = market.contains("COMMODITY");
            let ticker_yfinance_name = match is_commodity {
                false => format!("{}.BA", symbol),
                true => symbol.clone(),
            };
            TickerInformationFromDataCollector {
                ticker_symbol: symbol,
                is_commodity,
                yfinance_ticker_name: ticker_yfinance_name,
            }
        })
        .collect();

    Ok(tickers_info)
}

pub async fn get_tickers_rava(
    pool: Arc<PgPool>,
) -> Result<Vec<TickerInformationFromDataCollector>, sqlx::Error> {
    let result: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT ticker FROM rava_tickers
        "#,
    )
    .fetch_all(&*pool)
    .await?;

    // rava doesn't contain tickers that are commodities
    let tickers_info: Vec<TickerInformationFromDataCollector> = result
        .into_iter()
        .map(|symbol| TickerInformationFromDataCollector {
            ticker_symbol: symbol.clone(),
            is_commodity: false,
            yfinance_ticker_name: format!("{}.BA", symbol),
        })
        .collect();
    Ok(tickers_info)
}
