use axum::{
	extract::{Path, State},
	http::StatusCode,
};
use serde::Serialize;
use utoipa::ToSchema;

use crate::endpoints::DCState;

#[derive(Serialize, ToSchema)]
pub struct TickerSectorResponse {
	/// HTTP status code returned by the endpoint
	pub status: u16,
	pub ticker: String,
	pub sector: Option<String>,
}

#[utoipa::path(
	get,
	path = "/ticker/sector/{ticker_name}",
	params(
		("ticker_name" = String, Path, description = "Ticker symbol, for example GGAL")
	),
	responses(
		(status = 200, description = "Sector assigned to the ticker", body = TickerSectorResponse, example = json!({
			"status": 200,
			"ticker": "GGAL",
			"sector": "Finanzas"
		})),
		(status = 404, description = "Ticker sector not found", body = TickerSectorResponse),
		(status = 500, description = "Database error while fetching ticker sector", body = TickerSectorResponse)
	),
	tag = "Tickers"
)]
pub async fn api_get_ticker_sector(
	State(dc_state): State<DCState>,
	Path(ticker_name): Path<String>,
) -> axum::Json<serde_json::Value> {
	let ticker = ticker_name.trim().to_uppercase();
	let result = sqlx::query_scalar::<_, String>(
		"SELECT sector FROM ticker_sector WHERE ticker = $1",
	)
	.bind(&ticker)
	.fetch_optional(&dc_state.sqlx_pool)
	.await;

	let (status, sector) = match result {
		Ok(Some(sector)) => (StatusCode::OK, Some(sector)),
		Ok(None) => (StatusCode::NOT_FOUND, None),
		Err(error) => {
			eprintln!("Failed to fetch sector for ticker {}: {}", ticker, error);
			return axum::Json(serde_json::json!({
				"status": StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
				"ticker": ticker,
				"sector": null
			}));
		}
	};

	axum::Json(serde_json::json!({
		"status": status.as_u16(),
		"ticker": ticker,
		"sector": sector
	}))
}

#[cfg(test)]
mod tests {
	use super::*;
	use axum::extract::State;
	use sqlx::PgPool;
	use yfinance_rs::YfClient;

	fn build_test_state(pool: PgPool) -> DCState {
		DCState {
			sqlx_pool: pool,
			yf_client: YfClient::builder()
				.user_agent("coverage-test")
				.build()
				.unwrap(),
		}
	}

	#[sqlx::test]
	async fn gets_ticker_sector(pool: PgPool) {
		sqlx::query("INSERT INTO ticker_sector (ticker, sector) VALUES ('GGAL', 'Finanzas')")
			.execute(&pool)
			.await
			.unwrap();

		let response = api_get_ticker_sector(
			State(build_test_state(pool)),
			Path("ggal".to_string()),
		)
		.await;

		assert_eq!(response.0["status"], 200);
		assert_eq!(response.0["ticker"], "GGAL");
		assert_eq!(response.0["sector"], "Finanzas");
	}

	#[sqlx::test]
	async fn returns_not_found_for_unknown_ticker(pool: PgPool) {
		let response = api_get_ticker_sector(
			State(build_test_state(pool)),
			Path("UNKNOWN".to_string()),
		)
		.await;

		assert_eq!(response.0["status"], 404);
		assert_eq!(response.0["ticker"], "UNKNOWN");
		assert!(response.0["sector"].is_null());
	}
}
