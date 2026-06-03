CREATE TABLE IF NOT EXISTS available_tickers_byma (
    id          SERIAL PRIMARY KEY,
    symbol      TEXT NOT NULL UNIQUE,
    market      TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_history_price_cached_at TIMESTAMP WITH TIME ZONE
);
