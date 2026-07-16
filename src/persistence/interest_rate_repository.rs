use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct InterestRatePoint {
    pub source: String,
    pub series_id: String,
    pub ts: i64,
    pub value: Decimal,
}

pub async fn is_cached(
    pool: PgPool,
    source: &str,
    series_id: &str,
) -> Result<Option<(Vec<InterestRatePoint>, i64)>, sqlx::Error> {
    let result: Vec<InterestRatePoint> = sqlx::query_as(
        r#"
        SELECT source, series_id, ts, value
        FROM interest_rate_cached
        WHERE source = $1 AND series_id = $2
        ORDER BY ts ASC
        "#,
    )
    .bind(source)
    .bind(series_id)
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
    source: &str,
    series_id: &str,
    new_data: Vec<InterestRatePoint>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        DELETE FROM interest_rate_cached
        WHERE source = $1 AND series_id = $2
        "#,
    )
    .bind(source)
    .bind(series_id)
    .execute(&mut *tx)
    .await?;

    for point in new_data {
        sqlx::query(
            r#"
            INSERT INTO interest_rate_cached (source, series_id, ts, value)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(&point.source)
        .bind(&point.series_id)
        .bind(point.ts)
        .bind(point.value)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}
