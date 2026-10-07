use axum::{
    extract::{Path, State},
    http::StatusCode,
};
use serde::Serialize;
use utoipa::ToSchema;

use crate::endpoints::DCState;
use crate::interest_rates::{ar_client, us_client};
use crate::persistence::interest_rate_repository::{self, InterestRatePoint};

const CACHE_TTL_DAYS: i64 = 5;

#[derive(Serialize, ToSchema)]
pub struct InterestRateResponse {
    /// HTTP status code
    pub status: u16,
    /// Origin of the series: `"US"` (Yahoo Finance) or `"AR"` (BCRA)
    pub source: String,
    /// Normalized series identifier (e.g. `TNX`, `TPM`, `BADLAR`)
    pub series: String,
    /// Vector of `{ source, series_id, ts, value }` points
    #[schema(value_type = Vec<Object>)]
    pub data: serde_json::Value,
    /// `true` if served from cache, `false` if just fetched live
    pub cached: bool,
}

#[utoipa::path(
    post,
    path = "/interest-rate/us/{series}",
    params(
        ("series" = String, Path, description = "US interest rate series: IRX, FVX, TNX, TYX (con o sin '^')")
    ),
    responses(
        (status = 200, description = "Serie de tasas US (Yahoo Finance), cacheada 5 días", body = InterestRateResponse, example = json!({
            "status": 200,
            "source": "US",
            "series": "TNX",
            "data": [{ "source": "US", "series_id": "TNX", "ts": 1747008000000_i64, "value": "4.25" }],
            "cached": true
        })),
        (status = 500, description = "Falla de Yahoo Finance o de la base", body = serde_json::Value)
    ),
    tag = "Interest Rates"
)]
pub async fn api_get_us_interest_rate(
    State(dc_state): State<DCState>,
    Path(series): Path<String>,
) -> axum::Json<serde_json::Value> {
    handle_request(
        dc_state.sqlx_pool,
        us_client::SOURCE,
        &series,
        |s| async move { us_client::fetch_series(&s).await },
    )
    .await
}

#[utoipa::path(
    post,
    path = "/interest-rate/ar/{series}",
    params(
        ("series" = String, Path, description = "AR interest rate series: TPM, BADLAR o variable_id numérico del BCRA")
    ),
    responses(
        (status = 200, description = "Serie de tasas AR (BCRA), cacheada 5 días", body = InterestRateResponse, example = json!({
            "status": 200,
            "source": "AR",
            "series": "TPM",
            "data": [{ "source": "AR", "series_id": "TPM", "ts": 1747008000000_i64, "value": "40.00" }],
            "cached": true
        })),
        (status = 500, description = "Falla del BCRA o de la base", body = serde_json::Value)
    ),
    tag = "Interest Rates"
)]
pub async fn api_get_ar_interest_rate(
    State(dc_state): State<DCState>,
    Path(series): Path<String>,
) -> axum::Json<serde_json::Value> {
    handle_request(
        dc_state.sqlx_pool,
        ar_client::SOURCE,
        &series,
        |s| async move { ar_client::fetch_series(&s).await },
    )
    .await
}

pub(crate) async fn handle_request<F, Fut>(
    pool: sqlx::PgPool,
    source: &str,
    series: &str,
    fetcher: F,
) -> axum::Json<serde_json::Value>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<InterestRatePoint>, String>>,
{
    handle_request_with_ttl(pool, source, series, CACHE_TTL_DAYS, fetcher).await
}

/// Like `handle_request`, refetching when the latest observation is older than `ttl_days`.
pub(crate) async fn handle_request_with_ttl<F, Fut>(
    pool: sqlx::PgPool,
    source: &str,
    series: &str,
    ttl_days: i64,
    fetcher: F,
) -> axum::Json<serde_json::Value>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<InterestRatePoint>, String>>,
{
    let cached = interest_rate_repository::is_cached(pool.clone(), source, series).await;

    let cached = match cached {
        Ok(value) => value,
        Err(e) => {
            eprintln!("Failed to read interest_rate_cached: {}", e);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to read interest rate cache",
            );
        }
    };

    let (data, was_cached) = match cached {
        Some((data, last_updated_ts)) => {
            let stale_threshold = chrono::Utc::now() - chrono::Duration::days(ttl_days);

            if stale_threshold.timestamp_millis() > last_updated_ts {
                match fetcher(series.to_string()).await {
                    Ok(fresh) => match interest_rate_repository::update_cache(
                        pool,
                        source,
                        series,
                        fresh.clone(),
                    )
                    .await
                    {
                        Ok(()) => (fresh, false),
                        Err(e) => {
                            eprintln!("Failed to update interest_rate_cached: {}", e);
                            return error_response(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Failed to update interest rate cache",
                            );
                        }
                    },
                    Err(e) => {
                        eprintln!("Failed to fetch fresh interest rate data: {}", e);
                        (data, true)
                    }
                }
            } else {
                (data, true)
            }
        }
        None => match fetcher(series.to_string()).await {
            Ok(fresh) => {
                if let Err(e) =
                    interest_rate_repository::update_cache(pool, source, series, fresh.clone())
                        .await
                {
                    eprintln!("Failed to persist interest_rate_cached: {}", e);
                }
                (fresh, false)
            }
            Err(e) => {
                eprintln!("Failed to fetch interest rate series '{}': {}", series, e);
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to fetch interest rate series",
                );
            }
        },
    };

    axum::Json(serde_json::json!({
        "status": StatusCode::OK.as_u16(),
        "source": source,
        "series": series,
        "data": data,
        "cached": was_cached,
    }))
}

fn error_response(status: StatusCode, message: &str) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": status.as_u16(),
        "message": { "error": message },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Path, State};
    use rust_decimal::Decimal;
    use sqlx::PgPool;
    use yfinance_rs::YfClient;

    fn point(source: &str, series: &str, ts: i64, value: i64) -> InterestRatePoint {
        InterestRatePoint {
            source: source.to_string(),
            series_id: series.to_string(),
            ts,
            value: Decimal::new(value, 0),
        }
    }

    fn state(pool: PgPool) -> DCState {
        DCState {
            sqlx_pool: pool,
            yf_client: YfClient::builder()
                .user_agent("coverage-test")
                .build()
                .unwrap(),
        }
    }

    #[sqlx::test]
    async fn interest_rate_cache_covers_fresh_stale_missing_and_error_paths(pool: PgPool) {
        let now = chrono::Utc::now().timestamp_millis();
        interest_rate_repository::update_cache(
            pool.clone(),
            "US",
            "TNX",
            vec![point("US", "TNX", now, 4)],
        )
        .await
        .unwrap();
        let response = handle_request(pool.clone(), "US", "TNX", |_| async {
            panic!("a fresh cache must not invoke the fetcher")
        })
        .await;
        assert_eq!(response.0["status"], 200);
        assert_eq!(response.0["cached"], true);

        interest_rate_repository::update_cache(
            pool.clone(),
            "US",
            "IRX",
            vec![point("US", "IRX", 0, 1)],
        )
        .await
        .unwrap();
        let response = handle_request(pool.clone(), "US", "IRX", |_| async {
            Ok(vec![point("US", "IRX", 10, 2)])
        })
        .await;
        assert_eq!(response.0["cached"], false);
        let cached = interest_rate_repository::is_cached(pool.clone(), "US", "IRX")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cached.1, 10);

        interest_rate_repository::update_cache(
            pool.clone(),
            "AR",
            "TPM",
            vec![point("AR", "TPM", 0, 30)],
        )
        .await
        .unwrap();
        let response = handle_request(pool.clone(), "AR", "TPM", |_| async {
            Err("BCRA unavailable".to_string())
        })
        .await;
        assert_eq!(response.0["status"], 200);
        assert_eq!(response.0["cached"], true);

        let response = handle_request(pool.clone(), "AR", "BADLAR", |_| async {
            Ok(vec![point("AR", "BADLAR", 20, 35)])
        })
        .await;
        assert_eq!(response.0["cached"], false);
        assert!(
            interest_rate_repository::is_cached(pool.clone(), "AR", "BADLAR")
                .await
                .unwrap()
                .is_some()
        );

        let response = handle_request(pool.clone(), "US", "MISSING", |_| async {
            Err("unsupported".to_string())
        })
        .await;
        assert_eq!(response.0["status"], 500);

        let response =
            api_get_us_interest_rate(State(state(pool.clone())), Path("TNX".to_string())).await;
        assert_eq!(response.0["status"], 200);
        let response =
            api_get_ar_interest_rate(State(state(pool.clone())), Path("TPM".to_string())).await;
        assert_eq!(response.0["status"], 200);

        pool.close().await;
        let response = handle_request(pool, "US", "TNX", |_| async { Ok(Vec::new()) }).await;
        assert_eq!(response.0["status"], 500);
        assert_eq!(
            response.0["message"]["error"],
            "Failed to read interest rate cache"
        );
    }

    /// `series_id` values longer than the `interest_rate_cached.series_id`
    /// column (`VARCHAR(20)`) make `update_cache`'s INSERT fail, which is a
    /// convenient way to exercise the "fetch succeeded but persisting the
    /// cache failed" branches without needing to sever the DB connection.
    fn point_with_oversized_series_id(source: &str) -> InterestRatePoint {
        InterestRatePoint {
            source: source.to_string(),
            series_id: "X".repeat(64),
            ts: 999,
            value: Decimal::new(1, 0),
        }
    }

    #[sqlx::test]
    async fn handle_request_reports_cache_write_failure_after_stale_refetch(pool: PgPool) {
        interest_rate_repository::update_cache(
            pool.clone(),
            "US",
            "STALE",
            vec![point("US", "STALE", 0, 1)],
        )
        .await
        .unwrap();

        let response = handle_request(pool.clone(), "US", "STALE", |_| async {
            Ok(vec![point_with_oversized_series_id("US")])
        })
        .await;

        assert_eq!(response.0["status"], 500);
        assert_eq!(
            response.0["message"]["error"],
            "Failed to update interest rate cache"
        );
    }

    #[sqlx::test]
    async fn handle_request_serves_fresh_data_even_when_cache_write_fails(pool: PgPool) {
        let response = handle_request(pool.clone(), "US", "NOCACHE", |_| async {
            Ok(vec![point_with_oversized_series_id("US")])
        })
        .await;

        // Persisting the newly-fetched data fails, but the endpoint should
        // still hand back the freshly fetched (uncached) data to the caller.
        assert_eq!(response.0["status"], 200);
        assert_eq!(response.0["cached"], false);
        assert!(
            interest_rate_repository::is_cached(pool, "US", "NOCACHE")
                .await
                .unwrap()
                .is_none()
        );
    }
}
