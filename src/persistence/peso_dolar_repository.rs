use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct PesoDolarPoint {
    pub origin: String,
    pub ts: i64,
    pub buy_price: Decimal,
    pub sell_price: Decimal,
}

pub async fn is_cached(
    pool: PgPool,
    origin: &str,
    ts: i64,
) -> Result<Option<(Vec<PesoDolarPoint>, i64)>, sqlx::Error> {
    let result: Vec<PesoDolarPoint> = sqlx::query_as(
        r#"
        SELECT origin, ts, buy_price, sell_price
        FROM peso_dolar_cached
        WHERE origin = $1 AND ts = $2
        ORDER BY ts ASC
        "#,
    )
    .bind(origin)
    .bind(ts)
    .fetch_all(&pool)
    .await?;

    match result.is_empty() {
        true => Ok(None),
        false => {
            let latest_ts = result.iter().map(|p| p.ts).max().unwrap_or(0);
            Ok(Some((result, latest_ts)))
        }
    }
}

pub async fn update_cache(
    pool: PgPool,
    origin: &str,
    ts: i64,
    new_data: Vec<PesoDolarPoint>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        DELETE FROM peso_dolar_cached
        WHERE origin = $1 AND ts = $2
        "#,
    )
    .bind(origin)
    .bind(ts)
    .execute(&mut *tx)
    .await?;

    for point in new_data {
        sqlx::query(
            r#"
            INSERT INTO peso_dolar_cached (origin, ts, buy_price, sell_price)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(&point.origin)
        .bind(&point.ts)
        .bind(point.buy_price)
        .bind(point.sell_price)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}