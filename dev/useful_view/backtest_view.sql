DROP VIEW IF EXISTS backtest_binance_instruments;

CREATE VIEW backtest_binance_instruments AS
SELECT
    instrument.id AS inst_id,
    instrument.symbol,
    instrument.status,
    instrument.base_asset,
    instrument.quote_asset,
    'binance_' || instrument.symbol_type AS instrument_type,
    CASE
        WHEN instrument.symbol_type = 'SWAP' THEN (
            SELECT (filter.value ->> 'notional')::DOUBLE PRECISION
            FROM jsonb_array_elements(instrument.filters) AS filter(value)
            WHERE filter.value ->> 'filterType' = 'MIN_NOTIONAL'
            LIMIT 1
        )
        ELSE (
            SELECT (filter.value ->> 'minNotional')::DOUBLE PRECISION
            FROM jsonb_array_elements(instrument.filters) AS filter(value)
            WHERE filter.value ->> 'filterType' = 'NOTIONAL'
            LIMIT 1
        )
    END AS min_trade_size,
    (
        SELECT (filter.value ->> 'minQty')::DOUBLE PRECISION
        FROM jsonb_array_elements(instrument.filters) AS filter(value)
        WHERE filter.value ->> 'filterType' = 'LOT_SIZE'
        LIMIT 1
    ) AS min_trade_quantity,
    COALESCE(
        (
            SELECT (filter.value ->> 'tickSize')::DOUBLE PRECISION
            FROM jsonb_array_elements(instrument.filters) AS filter(value)
            WHERE filter.value ->> 'filterType' = 'PRICE_FILTER'
            LIMIT 1
        ),
        0.1
    ) AS tick_size
FROM binance_instruments AS instrument;
