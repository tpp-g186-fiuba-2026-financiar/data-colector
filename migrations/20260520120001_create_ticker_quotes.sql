CREATE TABLE IF NOT EXISTS ticker_quotes (
    ticker_id      INTEGER NOT NULL REFERENCES tickers(id) ON DELETE CASCADE,
    recorded_at    TIMESTAMPTZ NOT NULL,
    opening_price  DOUBLE PRECISION NOT NULL,
    offered_price  DOUBLE PRECISION NOT NULL,
    PRIMARY KEY (ticker_id, recorded_at)
);

CREATE INDEX IF NOT EXISTS idx_ticker_quotes_recorded_at
    ON ticker_quotes (recorded_at DESC);
