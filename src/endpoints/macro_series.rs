use axum::extract::{Path, State};

use crate::endpoints::DCState;
use crate::endpoints::interest_rates::handle_request_with_ttl;
use crate::interest_rates::argentinadatos_client;

/// Daily Argentine series feed a model that predicts every day: a 5-day window (as
/// for interest rates) would leave it a full week behind.
const MACRO_CACHE_TTL_DAYS: i64 = 2;

#[utoipa::path(
    post,
    path = "/macro/argdatos/{series}",
    params(
        ("series" = String, Path, description = "Argentine series (ArgentinaDatos): CCL, MEP, OFICIAL, MAYORISTA, BLUE, RIESGO_PAIS")
    ),
    responses(
        (status = 200, description = "Serie diaria de ArgentinaDatos, cacheada 2 días", body = crate::endpoints::interest_rates::InterestRateResponse, example = json!({
            "status": 200,
            "source": "ARGDATOS",
            "series": "CCL",
            "data": [{ "source": "ARGDATOS", "series_id": "CCL", "ts": 1747008000000_i64, "value": "1610.8" }],
            "cached": true
        })),
        (status = 500, description = "Serie no soportada, falla de ArgentinaDatos o de la base", body = serde_json::Value)
    ),
    tag = "Macro Series"
)]
pub async fn api_get_argdatos_series(
    State(dc_state): State<DCState>,
    Path(series): Path<String>,
) -> axum::Json<serde_json::Value> {
    handle_request_with_ttl(
        dc_state.sqlx_pool,
        argentinadatos_client::SOURCE,
        &series.to_uppercase(),
        MACRO_CACHE_TTL_DAYS,
        |s| async move { argentinadatos_client::fetch_series(&s).await },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::interest_rate_repository::{self, InterestRatePoint};
    use rust_decimal::Decimal;
    use sqlx::PgPool;
    use yfinance_rs::YfClient;

    fn point(source: &str, series: &str, ts: i64) -> InterestRatePoint {
        InterestRatePoint {
            source: source.to_string(),
            series_id: series.to_string(),
            ts,
            value: Decimal::new(1610, 0),
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
    async fn macro_cache_is_refreshed_after_two_days_but_not_after_one(pool: PgPool) {
        let day_ms = 86_400_000_i64;
        let now = chrono::Utc::now().timestamp_millis();

        // Latest observation 1 day old: still fresh for a 2-day window.
        interest_rate_repository::update_cache(
            pool.clone(),
            "ARGDATOS",
            "CCL",
            vec![point("ARGDATOS", "CCL", now - day_ms)],
        )
        .await
        .unwrap();
        let fresh = handle_request_with_ttl(pool.clone(), "ARGDATOS", "CCL", 2, |_| async {
            panic!("a fresh cache must not invoke the fetcher")
        })
        .await;
        assert_eq!(fresh.0["cached"], true);

        // Latest observation 3 days old: refetched.
        interest_rate_repository::update_cache(
            pool.clone(),
            "ARGDATOS",
            "MEP",
            vec![point("ARGDATOS", "MEP", now - 3 * day_ms)],
        )
        .await
        .unwrap();
        let stale = handle_request_with_ttl(pool.clone(), "ARGDATOS", "MEP", 2, |_| async {
            Ok(vec![point("ARGDATOS", "MEP", now)])
        })
        .await;
        assert_eq!(stale.0["cached"], false);
    }

    #[sqlx::test]
    async fn unsupported_macro_series_reports_an_error_without_touching_the_network(pool: PgPool) {
        let arg = api_get_argdatos_series(State(state(pool)), Path("nope".to_string())).await;
        assert_eq!(arg.0["status"], 500);
    }
}
