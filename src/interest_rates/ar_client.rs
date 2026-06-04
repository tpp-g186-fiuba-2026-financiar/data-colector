use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::persistence::interest_rate_repository::InterestRatePoint;

pub const SOURCE: &str = "AR";

const BCRA_BASE_URL: &str = "https://api.bcra.gob.ar/estadisticas/v3.0/Monetarias";

#[derive(Debug, Deserialize)]
struct BcraResponse {
    results: Vec<BcraObservation>,
}

#[derive(Debug, Deserialize)]
struct BcraObservation {
    fecha: String,
    valor: Decimal,
}

pub fn resolve_bcra_variable_id(series: &str) -> Option<u32> {
    match series.to_uppercase().as_str() {
        "TPM" => Some(6),
        "BADLAR" => Some(7),
        _ => series.parse::<u32>().ok(),
    }
}

pub async fn fetch_series(series: &str) -> Result<Vec<InterestRatePoint>, String> {
    let variable_id = resolve_bcra_variable_id(series)
        .ok_or_else(|| format!("Unsupported AR interest rate series: {}", series))?;

    let url = format!("{}/{}", BCRA_BASE_URL, variable_id);

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to call BCRA API: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("BCRA API returned status {}", response.status()));
    }

    let body: BcraResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse BCRA response: {}", e))?;

    let series_id_normalized = series.to_uppercase();

    let points: Vec<InterestRatePoint> = body
        .results
        .into_iter()
        .filter_map(|obs| {
            let date = NaiveDate::parse_from_str(&obs.fecha, "%Y-%m-%d").ok()?;
            let ts = date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis();
            Some(InterestRatePoint {
                source: SOURCE.to_string(),
                series_id: series_id_normalized.clone(),
                ts,
                value: obs.valor,
            })
        })
        .collect();

    Ok(points)
}
