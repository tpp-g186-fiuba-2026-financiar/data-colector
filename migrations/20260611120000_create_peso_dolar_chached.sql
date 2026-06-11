CREATE TABLE IF NOT EXISTS peso_dolar_cached (
    origin               VARCHAR(25) NOT NULL,
    ts                   BIGINT NOT NULL,
    
    buy_price            NUMERIC NOT NULL,
    sell_price           NUMERIC NOT NULL,
    
    PRIMARY KEY (origin, ts),
);

CREATE INDEX IF NOT EXISTS idx_peso_dolar_cached_origin_ts 
    ON peso_dolar_cached (origin, ts DESC);