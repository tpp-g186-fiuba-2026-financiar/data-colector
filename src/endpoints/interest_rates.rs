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

async fn handle_request<F, Fut>(
    pool: sqlx::PgPool,
    source: &str,
    series: &str,
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
            let stale_threshold = chrono::Utc::now() - chrono::Duration::days(CACHE_TTL_DAYS);

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
