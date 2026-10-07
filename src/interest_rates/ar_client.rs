use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::persistence::interest_rate_repository::InterestRatePoint;

pub const SOURCE: &str = "AR";

const BCRA_BASE_URL: &str = "https://api.bcra.gob.ar/estadisticas/v4.0/Monetarias";

/// Max observations asked per request (the API default is only 1000, ~4 years of
/// daily data). 3000 covers ~12 years of daily series.
const BCRA_LIMIT: u32 = 3000;

/// BCRA API v4.0 nests the observations: `results[].detalle[]` (the v3 shape,
/// a flat `results[]`, was deprecated and now answers HTTP 410).
#[derive(Debug, Deserialize)]
struct BcraResponse {
    results: Vec<BcraVariable>,
}

#[derive(Debug, Deserialize)]
struct BcraVariable {
    detalle: Vec<BcraObservation>,
}

#[derive(Debug, Deserialize)]
struct BcraObservation {
    fecha: String,
    valor: Decimal,
}

pub fn resolve_bcra_variable_id(series: &str) -> Option<u32> {
    match series.to_uppercase().as_str() {
        "TPM" => Some(44),
        "BADLAR" => Some(7),
        "RESERVAS" => Some(1),
        "TC_MAYORISTA" => Some(5),
        "BASE_MONETARIA" => Some(15),
        _ => series.parse::<u32>().ok(),
    }
}

pub async fn fetch_series(series: &str) -> Result<Vec<InterestRatePoint>, String> {
    let variable_id = resolve_bcra_variable_id(series)
        .ok_or_else(|| format!("Unsupported AR interest rate series: {}", series))?;

    let url = format!("{}/{}?limit={}", BCRA_BASE_URL, variable_id, BCRA_LIMIT);

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

    Ok(parse_bcra_response(body, series))
}

/// Converts a raw BCRA API response into our internal `InterestRatePoint`
/// representation, dropping any observation whose date cannot be parsed.
///
/// Extracted from `fetch_series` so the mapping/filtering logic can be
/// exercised directly in tests without any networking.
fn parse_bcra_response(body: BcraResponse, series: &str) -> Vec<InterestRatePoint> {
    let series_id_normalized = series.to_uppercase();

    body.results
        .into_iter()
        .flat_map(|variable| variable.detalle)
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
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_named_numeric_and_invalid_bcra_series() {
        assert_eq!(resolve_bcra_variable_id("tpm"), Some(44));
        assert_eq!(resolve_bcra_variable_id("BADLAR"), Some(7));
        assert_eq!(resolve_bcra_variable_id("reservas"), Some(1));
        assert_eq!(resolve_bcra_variable_id("tc_mayorista"), Some(5));
        assert_eq!(resolve_bcra_variable_id("base_monetaria"), Some(15));
        assert_eq!(resolve_bcra_variable_id("123"), Some(123));
        assert_eq!(resolve_bcra_variable_id("unknown"), None);
    }

    #[tokio::test]
    async fn rejects_unsupported_bcra_series_before_networking() {
        let error = fetch_series("unknown").await.unwrap_err();
        assert!(error.contains("Unsupported AR interest rate series"));
    }

    #[test]
    fn parses_and_filters_bcra_observations() {
        let body = BcraResponse {
            results: vec![BcraVariable {
                detalle: vec![
                    BcraObservation {
                        fecha: "2026-01-15".to_string(),
                        valor: Decimal::new(4250, 2),
                    },
                    BcraObservation {
                        fecha: "not-a-date".to_string(),
                        valor: Decimal::new(1, 0),
                    },
                    BcraObservation {
                        fecha: "2026-02-01".to_string(),
                        valor: Decimal::new(4300, 2),
                    },
                ],
            }],
        };

        let points = parse_bcra_response(body, "tpm");

        assert_eq!(points.len(), 2);
        assert!(points.iter().all(|p| p.source == SOURCE));
        assert!(points.iter().all(|p| p.series_id == "TPM"));
        assert_eq!(points[0].value, Decimal::new(4250, 2));
        assert_eq!(points[1].value, Decimal::new(4300, 2));
        assert!(points[0].ts < points[1].ts);
    }

    #[test]
    fn deserializes_the_nested_bcra_v4_response() {
        // Real shape of GET /estadisticas/v4.0/Monetarias/44 (trimmed).
        let raw = r#"{
            "status": 200,
            "metadata": { "resultset": { "count": 2, "offset": 0, "limit": 3000 } },
            "results": [{
                "idVariable": 44,
                "detalle": [
                    { "fecha": "2026-10-02", "valor": 24.5625 },
                    { "fecha": "2026-10-01", "valor": 24.4375 }
                ]
            }]
        }"#;

        let body: BcraResponse = serde_json::from_str(raw).unwrap();
        let points = parse_bcra_response(body, "TPM");

        assert_eq!(points.len(), 2);
        assert_eq!(points[0].value, Decimal::new(245625, 4));
        assert_eq!(points[1].value, Decimal::new(244375, 4));
    }
}
