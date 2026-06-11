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
    let yahoo_symbol = resolve_yahoo_symbol(series)
        .ok_or_else(|| format!("Unsupported US interest rate series: {}", series))?;

    let client = YfClient::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
        .build()
        .map_err(|e| format!("Failed to create YF client: {}", e))?;

    let ticker = Ticker::new(&client, yahoo_symbol.to_string());

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
