# Data Collector — Endpoints

Documentación viva de la API HTTP del `data-collector`. Esta lista se va
actualizando a mano; la fuente de verdad ejecutable es el Swagger UI que se
levanta junto con el servicio.

## Swagger UI

Una vez corriendo el servicio (por defecto en el puerto definido en `API_PORT`):

- **Swagger UI:** [http://localhost:${API_PORT}/swagger](http://localhost:8080/swagger)
- **OpenAPI JSON:** [http://localhost:${API_PORT}/swagger-endpoints.json](http://localhost:8080/swagger-endpoints.json)

> Reemplazar `8080` por el puerto real configurado en `.env` (`API_PORT`).
> En el `docker-compose.yml` del repo el puerto está mapeado al host, así que
> esos links también funcionan desde la máquina local.

## Tabla resumen

| Método | Ruta | Tag | Handler | Descripción |
|---|---|---|---|---|
| `GET` | `/` | General | [`root::root_check`](src/endpoints/root.rs) | Mensaje de bienvenida |
| `GET` / `POST` | `/health` | General | [`health::health_check`](src/endpoints/health.rs) | Healthcheck con ping a Postgres |
| `POST` | `/available-tickers` | Tickers | [`available_tickers::api_get_available_tickers`](src/endpoints/available_tickers.rs) | Devuelve todos los símbolos cacheados (BYMA + commodities GOLD/OIL) |
| `POST` | `/historical-data/{ticker}` | Historical Data | [`historical_data::api_get_historical_data`](src/endpoints/historical_data.rs) | OHLCV histórico desde Yahoo Finance (cacheado 5 días) |

## Detalle por endpoint

### `GET /`
- **Respuesta 200:** `text/plain` con `"Welcome to the Data Collector API! This is the root endpoint."`

### `GET|POST /health`
- **200:** Postgres responde `SELECT 1` correctamente.
- **503:** Falla la conexión a Postgres.
- **Body:**
  ```json
  { "status": 200, "message": "Database connection is healthy and its working!" }
  ```

### `POST /available-tickers`
- **200:** Lista de tickers cacheados.
  ```json
  { "status": 200, "message": { "tickers": ["GGAL", "YPF", "GOLD", "OIL"] } }
  ```
- **404:** Tabla `available_tickers_byma` vacía.
- **500:** Error de DB.

### `POST /historical-data/{ticker}`
- **Param path** `ticker`: símbolo "limpio" (sin sufijo). Para BYMA: `GGAL`, `YPF`, etc.
  Para commodities: `GOLD` (`GC=F` en Yahoo) u `OIL` (`CL=F` en Yahoo).
- **200:** Vector de candles + flag `cached`.
  ```json
  {
    "status": 200,
    "data": [
      {
        "ticker": "GGAL",
        "ts": 1747008000000,
        "volume": 12345,
        "open_amount": "100.50",
        "high_amount": "105.00",
        "low_amount": "99.75",
        "close_amount": "104.20",
        "close_unadj_amount": "104.20"
      }
    ],
    "cached": true
  }
  ```
- **500:** Falla de Yahoo Finance o de la base.
- Si el cache está vacío o tiene más de 5 días, dispara fetch a Yahoo Finance y
  persiste en `ticker_history_data_cached_yf` en background.

## Background jobs (no son endpoints HTTP)

Estos no se exponen vía API pero corren en `tokio::spawn` al arrancar el binario:

- `BymaTickersPersistor::persist_available_tickers` — refresca el catálogo de
  tickers desde BYMA cada 60 min.
- `BymaTickerHistoricalDataPersistor::persist_historical_price_tickers` —
  refresca histórico de tickers BYMA en lotes de 8 cada 2–4 min.
- `CommoditiesHistoricalPersistor::persist_commodities_historical` — refresca
  histórico de `GOLD` (`GC=F`) y `OIL` (`CL=F`) cada 6 h.
