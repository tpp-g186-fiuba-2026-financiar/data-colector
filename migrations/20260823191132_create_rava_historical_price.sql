CREATE TABLE rava_ticker_history (
    ticker VARCHAR(50),
    fecha DATE,
    precio NUMERIC(10, 4),
    maximo NUMERIC(10, 4),
    minimo NUMERIC(10, 4),
    apertura NUMERIC(10, 4),
    volumen BIGINT,
    timestamp BIGINT,
    PRIMARY KEY (ticker, fecha)
);