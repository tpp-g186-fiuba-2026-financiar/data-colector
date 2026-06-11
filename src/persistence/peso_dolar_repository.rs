use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct PesoDolarPoint {
    pub value_type: String,
    pub ts: i64,
    pub buy_price: Decimal,
    pub sell_price: Decimal,
}

pub async fn is_cached(
    pool: PgPool,
    value_type: &str,
    ts: i64,
) -> Result<Option<(Vec<PesoDolarPoint>, i64)>, sqlx::Error> {
    let result: Vec<PesoDolarPoint> = sqlx::query_as(
        r#"
        SELECT value_type, ts, buy_price, sell_price
        FROM peso_dolar_cached
        WHERE value_type = $1 AND ts = $2
        ORDER BY ts ASC
        "#,
    )
    .bind(value_type)
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
    value_type: &str,
    ts: i64,
    new_data: Vec<PesoDolarPoint>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        DELETE FROM peso_dolar_cached
        WHERE value_type = $1 AND ts = $2
        "#,
    )
    .bind(value_type)
    .bind(ts)
    .execute(&mut *tx)
    .await?;

    for point in new_data {
        sqlx::query(
            r#"
            INSERT INTO peso_dolar_cached (value_type, ts, buy_price, sell_price)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(&point.value_type)
        .bind(&point.ts)
        .bind(point.buy_price)
        .bind(point.sell_price)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}