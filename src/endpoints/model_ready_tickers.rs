use crate::endpoints::DCState;
use axum::{extract::State, http::StatusCode};

const MIN_MODEL_HISTORY_ROWS: i64 = 100;

/// Tickers con suficientes ruedas cacheadas para construir las ventanas y el
/// backtest de los modelos. No alcanza con que el simbolo exista en BYMA.
pub async fn api_get_model_ready_tickers(
    State(dc_state): State<DCState>,
) -> axum::Json<serde_json::Value> {
    let result = sqlx::query_scalar::<_, String>(
        r#"
        SELECT ticker
        FROM ticker_history_data_cached_yf
        GROUP BY ticker
        HAVING COUNT(*) >= $1
        ORDER BY ticker
        "#,
    )
    .bind(MIN_MODEL_HISTORY_ROWS)
    .fetch_all(&dc_state.sqlx_pool)
    .await;

    match result {
        Ok(tickers) => axum::Json(serde_json::json!({
            "status": StatusCode::OK.as_u16(),
            "message": { "tickers": tickers }
        })),
        Err(error) => {
            eprintln!("Failed to list model-ready tickers: {}", error);
            axum::Json(serde_json::json!({
                "status": StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                "message": { "error": "Failed to list model-ready tickers" }
            }))
        }
    }
}
