-- Cuando se bajo por ultima vez cada serie. Sin esto la frescura se medía con
-- la fecha del ultimo dato, y las series que se publican con atraso (dolares
-- de ArgentinaDatos, fines de semana) parecian siempre vencidas: se volvian a
-- bajar y reescribir en cada pedido.
CREATE TABLE IF NOT EXISTS interest_rate_fetches (
    source     VARCHAR(10) NOT NULL,
    series_id  VARCHAR(20) NOT NULL,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (source, series_id)
);
