use std::{collections::HashMap, sync::Arc};

use sqlx::PgPool;

use crate::byma_scrapper::byma_session::TickerQuote;

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
        INSERT INTO tickers (symbol, market)
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
