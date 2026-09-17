use yfinance_rs::{Range, Ticker, YfClient};

use crate::persistence::interest_rate_repository::InterestRatePoint;

pub const SOURCE: &str = "US";

pub fn resolve_yahoo_symbol(series: &str) -> Option<&'static str> {
    match series.to_uppercase().as_str() {
        "IRX" | "^IRX" => Some("^IRX"),
        "FVX" | "^FVX" => Some("^FVX"),
        "TNX" | "^TNX" => Some("^TNX"),
        "TYX" | "^TYX" => Some("^TYX"),
        _ => None,
    }
}

pub async fn fetch_series(series: &str) -> Result<Vec<InterestRatePoint>, String> {
    let client = YfClient::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
        .build()
        .map_err(|e| format!("Failed to create YF client: {}", e))?;

    fetch_series_with_client(series, &client).await
}

/// Does the actual symbol resolution, history fetch and mapping to
/// `InterestRatePoint`s against a caller-provided `YfClient`.
///
/// Split out from `fetch_series` so tests can point the client's chart API
/// base URL at a mock server instead of hitting Yahoo Finance for real.
async fn fetch_series_with_client(
    series: &str,
    client: &YfClient,
) -> Result<Vec<InterestRatePoint>, String> {
    let yahoo_symbol = resolve_yahoo_symbol(series)
        .ok_or_else(|| format!("Unsupported US interest rate series: {}", series))?;

    let ticker = Ticker::new(client, yahoo_symbol.to_string());

    let candles = ticker
        .history(Some(Range::Y10), Some(yfinance_rs::Interval::D1), false)
        .await
        .map_err(|e| format!("{}", e))?;

    let normalized_series = yahoo_symbol.trim_start_matches('^').to_string();

    let points: Vec<InterestRatePoint> = candles
        .into_iter()
        .map(|candle| InterestRatePoint {
            source: SOURCE.to_string(),
            series_id: normalized_series.clone(),
            ts: candle.ts.timestamp_millis(),
            value: candle.close.amount(),
        })
        .collect();

    Ok(points)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn resolves_all_supported_yahoo_symbols_case_insensitively() {
        assert_eq!(resolve_yahoo_symbol("irx"), Some("^IRX"));
        assert_eq!(resolve_yahoo_symbol("^FVX"), Some("^FVX"));
        assert_eq!(resolve_yahoo_symbol("tnx"), Some("^TNX"));
        assert_eq!(resolve_yahoo_symbol("TYX"), Some("^TYX"));
        assert_eq!(resolve_yahoo_symbol("unknown"), None);
    }

    #[tokio::test]
    async fn rejects_unsupported_yahoo_series_before_networking() {
        let error = fetch_series("unknown").await.unwrap_err();
        assert!(error.contains("Unsupported US interest rate series"));
    }

    fn build_chart_json(symbol: &str, closes: &[f64]) -> String {
        let base_ts: i64 = 1_700_000_000;
        let timestamps: Vec<i64> = (0..closes.len())
            .map(|i| base_ts + (i as i64) * 86_400)
            .collect();
        let volume: Vec<u64> = (0..closes.len()).map(|i| 1_000 + i as u64).collect();

        serde_json::json!({
            "chart": {
                "error": serde_json::Value::Null,
                "result": [{
                    "meta": {
                        "currency": "USD",
                        "symbol": symbol,
                        "timezone": "America/New_York",
                        "gmtoffset": -14_400,
                    },
                    "timestamp": timestamps,
                    "indicators": {
                        "quote": [{
                            "open": closes,
                            "high": closes,
                            "low": closes,
                            "close": closes,
                            "volume": volume,
                        }],
                        "adjclose": [{ "adjclose": closes }],
                    },
                }],
            }
        })
        .to_string()
    }

    async fn mocked_client(server: &MockServer) -> YfClient {
        YfClient::builder()
            .user_agent("coverage-test")
            .base_chart(url::Url::parse(&format!("{}/v8/finance/chart/", server.uri())).unwrap())
            .build()
            .expect("failed to build YfClient with mocked chart base")
    }

    #[tokio::test]
    async fn fetches_and_normalizes_points_for_a_caret_prefixed_symbol() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v8/finance/chart/^TNX"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                build_chart_json("^TNX", &[4.1, 4.2, 4.3]),
                "application/json",
            ))
            .mount(&server)
            .await;

        let client = mocked_client(&server).await;
        let points = fetch_series_with_client("tnx", &client).await.unwrap();

        assert_eq!(points.len(), 3);
        assert!(points.iter().all(|p| p.source == SOURCE));
        // The leading '^' must be stripped from the normalized series id.
        assert!(points.iter().all(|p| p.series_id == "TNX"));
    }

    #[tokio::test]
    async fn reports_yahoo_errors_as_a_string() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v8/finance/chart/^IRX"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = mocked_client(&server).await;
        let error = fetch_series_with_client("irx", &client).await.unwrap_err();
        assert!(!error.is_empty());
    }
}
