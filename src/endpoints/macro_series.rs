use axum::extract::{Path, State};

use crate::endpoints::DCState;
use crate::endpoints::interest_rates::handle_request_with_ttl;
use crate::interest_rates::argentinadatos_client;

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

        interest_rate_repository::update_cache(
            pool.clone(),
            "ARGDATOS",
            "MEP",
            vec![point("ARGDATOS", "MEP", now - 3 * day_ms)],
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE interest_rate_fetches SET fetched_at = now() - interval '3 days' \
             WHERE source = 'ARGDATOS' AND series_id = 'MEP'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let stale = handle_request_with_ttl(pool.clone(), "ARGDATOS", "MEP", 2, |_| async {
            Ok(vec![point("ARGDATOS", "MEP", now)])
        })
        .await;
        assert_eq!(stale.0["cached"], false);
    }

    #[sqlx::test]
    async fn sin_datos_nuevos_no_reescribe_y_queda_fresca(pool: PgPool) {
        let now = chrono::Utc::now().timestamp_millis();
        interest_rate_repository::update_cache(
            pool.clone(),
            "ARGDATOS",
            "CCL",
            vec![point("ARGDATOS", "CCL", now - 3 * 86_400_000)],
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE interest_rate_fetches SET fetched_at = now() - interval '3 days' \
             WHERE source = 'ARGDATOS' AND series_id = 'CCL'",
        )
        .execute(&pool)
        .await
        .unwrap();
        // La fuente todavia no publico nada nuevo (fin de semana): se sirve lo
        // guardado y se anota la consulta, sin reescribir la serie.
        let first = handle_request_with_ttl(pool.clone(), "ARGDATOS", "CCL", 2, |_| async move {
            Ok(vec![point("ARGDATOS", "CCL", now - 3 * 86_400_000)])
        })
        .await;
        assert_eq!(first.0["cached"], true);
        // Ya consultada hoy: el siguiente pedido no vuelve a ir a la fuente.
        let second = handle_request_with_ttl(pool.clone(), "ARGDATOS", "CCL", 2, |_| async {
            panic!("recien consultada: no hay que volver a ir a la fuente")
        })
        .await;
        assert_eq!(second.0["cached"], true);
    }

    #[sqlx::test]
    async fn unsupported_macro_series_reports_an_error_without_touching_the_network(pool: PgPool) {
        let arg = api_get_argdatos_series(State(state(pool)), Path("nope".to_string())).await;
        assert_eq!(arg.0["status"], 500);
    }
}
