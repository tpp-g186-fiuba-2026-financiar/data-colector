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

    if result.is_empty() {
        return Ok(None);
    }
    // Frescura = cuando se bajo por ultima vez; si nunca se registro (datos
    // anteriores a esa tabla), la fecha del ultimo dato.
    let fetched_at: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        r#"
        SELECT fetched_at FROM interest_rate_fetches
        WHERE source = $1 AND series_id = $2
        "#,
    )
    .bind(source)
    .bind(series_id)
    .fetch_optional(&pool)
    .await?;
    let latest_ts = result.iter().map(|p| p.ts).max().unwrap_or(0);
    let checked = fetched_at.map_or(latest_ts, |at| at.timestamp_millis());
    Ok(Some((result, checked)))
}

/// Registra que la serie se acaba de consultar, aunque no haya traido nada nuevo.
pub async fn mark_fetched(pool: PgPool, source: &str, series_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO interest_rate_fetches (source, series_id, fetched_at)
        VALUES ($1, $2, now())
        ON CONFLICT (source, series_id) DO UPDATE SET fetched_at = EXCLUDED.fetched_at
        "#,
    )
    .bind(source)
    .bind(series_id)
    .execute(&pool)
    .await?;
    Ok(())
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

    // Una sola insercion para toda la serie (miles de filas): fila por fila,
    // contra una base remota, tardaba mas que el timeout de quien pedia y la
    // transaccion se cancelaba sin guardar nada.
    let sources: Vec<String> = new_data.iter().map(|p| p.source.clone()).collect();
    let series: Vec<String> = new_data.iter().map(|p| p.series_id.clone()).collect();
    let ts: Vec<i64> = new_data.iter().map(|p| p.ts).collect();
    let values: Vec<rust_decimal::Decimal> = new_data.iter().map(|p| p.value).collect();
    sqlx::query(
        r#"
        INSERT INTO interest_rate_cached (source, series_id, ts, value)
        SELECT * FROM UNNEST($1::varchar[], $2::varchar[], $3::int8[], $4::numeric[])
        ON CONFLICT (source, series_id, ts) DO UPDATE SET value = EXCLUDED.value
        "#,
    )
    .bind(&sources)
    .bind(&series)
    .bind(&ts)
    .bind(&values)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO interest_rate_fetches (source, series_id, fetched_at)
        VALUES ($1, $2, now())
        ON CONFLICT (source, series_id) DO UPDATE SET fetched_at = EXCLUDED.fetched_at
        "#,
    )
    .bind(source)
    .bind(series_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}
