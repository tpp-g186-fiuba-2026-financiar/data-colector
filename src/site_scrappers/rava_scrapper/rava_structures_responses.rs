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
