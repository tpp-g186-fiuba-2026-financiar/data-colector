pub mod rava_scrapper_handler;
pub mod rava_structures_responses;

pub const MOCK_CLASIFICACION_JSON: &str = r#"{
        "datos": {
            "BYMA": {"st": "CS", "sst": "M", "text": "C:01"},
            "ALUA": {"st": "CS", "sst": "M", "text": "C:01"},
            "AAPL": {"st": "CD", "sst": "", "text": "C:23"},
            "A3B":  {"st": "CS", "sst": "", "text": ""}
        }
    }"#;

pub const MOCK_REFDATA_JSON: &str = r#"{
        "datos": {
            "arg:BYMA": {"nc": "BYMA", "nl": "Bolsas y Mercados Argentinos", "desc": ""},
            "arg:ALUA": {"nc": "Aluar", "nl": "Aluar Aluminio Argentino", "desc": ""},
            "arg:AAPL": {"nc": "Apple", "nl": "Apple Inc.", "desc": ""},
            "arg:A30C80000J": {"nc": "", "nl": "", "desc": ""}
        }
    }"#;

pub const MOCK_BYMA_HISTORICAL_JSON: &str = r#"{
        "simbolo": "BYMA",
        "datos": [
            {"precio": 1.5, "maximo": 1.50046, "minimo": 1.00031, "apertura": 1.00031, "volumen": 19600550, "fecha": "2017-05-23T00:00:00.000Z", "timestamp": 1495540800},
            {"precio": 1.71, "maximo": 1.77055, "minimo": 1.40043, "apertura": 1.50046, "volumen": 115719480, "fecha": "2017-05-24T00:00:00.000Z", "timestamp": 1495627200}
        ]
    }"#;
