CREATE TABLE IF NOT EXISTS interest_rate_cached (
    source     VARCHAR(10) NOT NULL,
    series_id  VARCHAR(20) NOT NULL,
    ts         BIGINT NOT NULL,
    value      NUMERIC NOT NULL,

    PRIMARY KEY (source, series_id, ts)
);

CREATE INDEX IF NOT EXISTS idx_interest_rate_cached_source_series_ts
    ON interest_rate_cached (source, series_id, ts DESC);
