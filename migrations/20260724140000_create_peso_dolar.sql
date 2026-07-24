CREATE TABLE IF NOT EXISTS peso_dolar_cached (
    value_type               VARCHAR(25) NOT NULL,
    ts                   BIGINT NOT NULL,
    
    buy_price            NUMERIC NOT NULL,
    sell_price           NUMERIC NOT NULL,
    
    PRIMARY KEY (value_type, ts),
);

CREATE INDEX IF NOT EXISTS idx_peso_dolar_cached_value_type_ts 
    ON peso_dolar_cached (value_type, ts DESC);