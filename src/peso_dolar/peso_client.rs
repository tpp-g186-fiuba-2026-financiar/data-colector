const DOLAR_CURRENT_URL: &str = "https://dolarapi.com/v1/dolares";
const DOLAR_HISTORICAL_URL: &str = "https://api.argentinadatos.com/v1/cotizaciones/dolares";

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::PgPool;

use crate::persistence::peso_dolar_repository::{self, PesoDolarPoint};

pub async fn fetch_current() -> Result<Value, String> {
	let client = reqwest::Client::builder()
		.danger_accept_invalid_certs(true)
		.build()
		.map_err(|e| format!("Failed to build HTTP client: {}", e))?;

	let resp = client
		.get(DOLAR_CURRENT_URL)
		.send()
		.await
		.map_err(|e| format!("Failed to call current dolar API: {}", e))?;

	if !resp.status().is_success() {
		return Err(format!("DOLAR current API returned status {}", resp.status()));
	}

	let json: Value = resp
		.json()
		.await
		.map_err(|e| format!("Failed to parse current dolar response: {}", e))?;

	Ok(json)
}

pub async fn fetch_historical_points() -> Result<Vec<PesoDolarPoint>, String> {
	let client = reqwest::Client::builder()
		.danger_accept_invalid_certs(true)
		.build()
		.map_err(|e| format!("Failed to build HTTP client: {}", e))?;

	let resp = client
		.get(DOLAR_HISTORICAL_URL)
		.send()
		.await
		.map_err(|e| format!("Failed to call historical dolar API: {}", e))?;

	if !resp.status().is_success() {
		return Err(format!("DOLAR historical API returned status {}", resp.status()));
	}

	let historical_json: Value = resp
		.json()
		.await
		.map_err(|e| format!("Failed to parse historical dolar response: {}", e))?;

	let mut points: Vec<PesoDolarPoint> = Vec::new();

	if let Value::Array(items) = &historical_json {
		for item in items.iter() {
			if let Value::Object(map) = item {
				let ts_opt: Option<i64> = if let Some(fecha) = map.get("fecha") {
					match fecha {
						Value::String(s) => {
							if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
								Some(date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis())
							} else if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
								Some(dt.with_timezone(&Utc).timestamp_millis())
							} else {
								None
							}
						}
						Value::Number(n) => n.as_i64(),
						_ => None,
					}
				} else {
                    None
                };

				let buy_opt: Option<Decimal> = map
					.get("compra")
					.and_then(|v| match v {
						Value::String(s) => Decimal::from_str_exact(s).ok(),
						Value::Number(n) => Decimal::from_str_exact(&n.to_string()).ok(),
						_ => None,
					});

				let sell_opt: Option<Decimal> = map
					.get("venta")
					.and_then(|v| match v {
						Value::String(s) => Decimal::from_str_exact(s).ok(),
						Value::Number(n) => Decimal::from_str_exact(&n.to_string()).ok(),
						_ => None,
					});
                
                let value_type: Option<String> = map
					.get("casa")
					.and_then(|v| match v {
						Value::String(s) => Some(s.clone()),
						Value::Number(n) => Some(n.to_string()),
						_ => None,
					});    

				if let (Some(ts), Some(buy), Some(sell), Some(value_type)) = (ts_opt, buy_opt, sell_opt, value_type) {
					points.push(PesoDolarPoint {
						value_type: value_type,
						ts,
						buy_price: buy,
						sell_price: sell,
					});
				}
			}
		}
	}

	Ok(points)
}

pub async fn fetch_historical_and_persist(pool: PgPool) -> Result<(Vec<PesoDolarPoint>, bool), String> {
	let points = fetch_historical_points().await?;

    // Veo el timestamp de ayer ya que el valor de hoy no esta disponible aun
	let batch_ts = Utc::today().pred_opt().unwrap().and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis();
    // Uso el oficial para ver si esta en el cache
	let value_type = "oficial".to_string();

	let was_cached = match peso_dolar_repository::is_cached(pool.clone(), value_type.clone(), batch_ts).await {
		Ok(Some((_existing, _last_ts))) => true,
		Ok(None) => {
			if !points.is_empty() {
				if let Err(e) = peso_dolar_repository::update_cache(pool.clone(), value_type, batch_ts, points.clone()).await {
					eprintln!("Failed to persist peso_dolar_cached: {}", e);
				}
			}
			false
		}
		Err(e) => {
			eprintln!("Failed to read peso_dolar_cached: {}", e);
			false
		}
	};

	Ok((points, was_cached))
}