-- Add migration script here
CREATE TABLE bid_offer_historical_prices (
    ticker VARCHAR(20) NOT NULL,
    recorded_at DATE NOT NULL,
    bid NUMERIC(12, 4),
    offered NUMERIC(12, 4),
    PRIMARY KEY (ticker, recorded_at)
);