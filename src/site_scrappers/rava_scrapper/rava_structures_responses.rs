use std::collections::HashMap;

use serde::{Deserialize, Deserializer};

#[derive(Deserialize, Debug)]
pub struct RavaClasificacionResponse {
    pub datos: HashMap<String, ItemData>,
}

#[derive(Deserialize, Debug)]
pub struct RavaRefDataResponse {
    pub datos: HashMap<String, ItemDescriptionData>,
}

#[derive(Deserialize, Debug)]
pub struct ItemData {
    pub st: String,
    pub sst: String,
    pub text: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ItemDescriptionData {
    #[serde(rename = "nc")]
    pub nombre_corto: String,
    #[serde(rename = "nl")]
    pub nombre_largo: String,
    #[serde(rename = "desc", deserialize_with = "description_deserialize")]
    // it can be nul so we use Option
    pub descripcion: Option<String>,
}

fn description_deserialize<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Option<String> = Option::deserialize(deserializer)?;

    // Lets remove the \n \r from the string if it is not None
    if value.is_none() {
        return Ok(None);
    }

    let value = value.unwrap();
    let value = value
        .replace("\n", "")
        .replace("\r", "")
        .replace("\t", "")
        .trim()
        .to_string();

    Ok(Some(value))
}

#[derive(Deserialize, Debug, Clone)]
pub struct RavaHistoricalResponse {
    pub simbolo: String,
    pub datos: Vec<PriceData>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PriceData {
    #[serde(default)]
    pub precio: f64,
    #[serde(default)]
    pub maximo: f64,
    #[serde(default)]
    pub minimo: f64,
    #[serde(default)]
    pub apertura: f64,
    #[serde(default)]
    pub volumen: f64,

    #[serde(deserialize_with = "date_only")]
    pub fecha: String,

    pub timestamp: i64,
}

// Custom deserialization function to strip the time part
fn date_only<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let full_string = String::deserialize(deserializer)?;

    // Split at 'T' to keep only "YYYY-MM-DD"
    let date_only = full_string.split('T').next().unwrap_or(&full_string);

    Ok(date_only.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        ItemDescriptionData, RavaClasificacionResponse, RavaHistoricalResponse, RavaRefDataResponse,
    };

    const CLASIFICACION_JSON: &str = r#"{
        "datos": {
            "BYMA": {"st": "CS", "sst": "M", "text": "C:01"},
            "ALUA": {"st": "CS", "sst": "M", "text": "C:01"},
            "AAPL": {"st": "CD", "sst": "", "text": "C:23"},
            "A3B":  {"st": "CS", "sst": "", "text": ""}
        }
    }"#;

    const REFDATA_JSON: &str = r#"{
        "datos": {
            "arg:BYMA": {"nc": "BYMA", "nl": "Bolsas y Mercados Argentinos", "desc": ""},
            "arg:ALUA": {"nc": "Aluar", "nl": "Aluar Aluminio Argentino", "desc": ""},
            "arg:AAPL": {"nc": "Apple", "nl": "Apple Inc.", "desc": ""},
            "arg:A30C80000J": {"nc": "", "nl": "", "desc": ""}
        }
    }"#;

    const BYMA_HISTORICAL_JSON: &str = r#"{
        "simbolo": "BYMA",
        "datos": [
            {"precio": 1.5, "maximo": 1.50046, "minimo": 1.00031, "apertura": 1.00031, "volumen": 19600550, "fecha": "2017-05-23T00:00:00.000Z", "timestamp": 1495540800},
            {"precio": 1.71, "maximo": 1.77055, "minimo": 1.40043, "apertura": 1.50046, "volumen": 115719480, "fecha": "2017-05-24T00:00:00.000Z", "timestamp": 1495627200}
        ]
    }"#;

    #[test]
    fn test_byma_historical_response_deserialization_works() {
        let response: RavaHistoricalResponse = serde_json::from_str(BYMA_HISTORICAL_JSON).unwrap();
        assert_eq!(response.simbolo, "BYMA");
        assert_eq!(response.datos.len(), 2);
    }

    #[test]
    fn test_date_only_deserialization() {
        let historical_response: RavaHistoricalResponse =
            serde_json::from_str(BYMA_HISTORICAL_JSON).unwrap();
        let first_price_data = &historical_response.datos[0];
        assert_eq!(first_price_data.fecha, "2017-05-23");

        assert_eq!(first_price_data.timestamp, 1495540800);
    }

    #[test]
    fn test_get_only_merval_tickers() {
        
        let response: RavaClasificacionResponse = serde_json::from_str(CLASIFICACION_JSON).unwrap();
        
        let merval_tickers: Vec<String> = response
            .datos
            .iter()
            .filter(|(_, item_data)| item_data.st == "CS" && item_data.sst == "M")
            .map(|(ticker, _)| ticker.clone())
            .collect();

        assert_eq!(merval_tickers.len(), 2);
        assert!(merval_tickers.contains(&"BYMA".to_string()));
        assert!(merval_tickers.contains(&"ALUA".to_string()));
    }

    #[test]
    fn test_classification_and_ref_data_tickers_from_byma() {
        let classification_response: RavaClasificacionResponse =
            serde_json::from_str(CLASIFICACION_JSON).unwrap();
        let ref_data_response: RavaRefDataResponse = serde_json::from_str(REFDATA_JSON).unwrap();

        // Get only MERVAL tickers 
        let merval_tickers: Vec<String> = classification_response
            .datos
            .iter()
            .filter(|(_, item_data)| item_data.sst == "M" && item_data.st == "CS")
            .map(|(ticker, _)| ticker.clone())
            .collect();

        let fixed_ref_data_tickers: HashMap<String, ItemDescriptionData> = ref_data_response
            .datos
            .into_iter()
            .map(|(key, value)| {
                let fixed_key = key.trim_start_matches("arg:").to_string();
                (fixed_key, value)
            })
            .collect();

        let merval_reference_data_tickers_hash: HashMap<String, ItemDescriptionData> =
            fixed_ref_data_tickers
                .into_iter()
                .filter(|(ticker, _)| merval_tickers.contains(ticker))
                .collect();

        assert_eq!(merval_reference_data_tickers_hash.len(), 2);
        assert!(merval_reference_data_tickers_hash.contains_key("BYMA"));
        assert!(merval_reference_data_tickers_hash.contains_key("ALUA"));
    }

    #[test]
    fn test_classification_ref_data_tickers_as_cedears_or_non_merval_item() {
        // This should return APPL as a non-Merval ticker


        let classification_response: RavaClasificacionResponse =
            serde_json::from_str(CLASIFICACION_JSON).unwrap();
        let ref_data_response: RavaRefDataResponse = serde_json::from_str(REFDATA_JSON).unwrap();

        let cedears_tickers: Vec<String> = classification_response
            .datos
            .iter()
            .filter(|(_, item_data)| item_data.sst != "M")
            .map(|(ticker, _)| ticker.clone())
            .collect();

        let fixed_ref_data_tickers: HashMap<String, ItemDescriptionData> = ref_data_response
            .datos
            .into_iter()
            .map(|(key, value)| {
                let fixed_key = key.trim_start_matches("arg:").to_string();
                (fixed_key, value)
            })
            .collect();

        let cedears_reference_data_tickers_hash: HashMap<String, ItemDescriptionData> =
            fixed_ref_data_tickers
                .into_iter()
                .filter(|(ticker, _)| cedears_tickers.contains(ticker))
                .collect();

        assert_eq!(cedears_reference_data_tickers_hash.len(), 1);
        assert!(cedears_reference_data_tickers_hash.contains_key("AAPL"));
    }
}
