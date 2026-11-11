CREATE OR REPLACE VIEW binance_spot_kline_rolling_1h AS
WITH rolling AS (
    SELECT
        LAST_VALUE(id) OVER w AS id,
        inst_id,
        candle_begin_time,
        FIRST_VALUE(open) OVER w AS open,
        MAX(high) OVER w AS high,
        MIN(low) OVER w AS low,
        LAST_VALUE(close) OVER w AS close,
        SUM(volume) OVER w AS volume,
        SUM(quote_volume) OVER w AS quote_volume,
        (SUM(number_of_trades) OVER w)::BIGINT AS number_of_trades,
        SUM(taker_buy_base_asset_volume) OVER w AS taker_buy_base_asset_volume,
        SUM(taker_buy_quote_asset_volume) OVER w AS taker_buy_quote_asset_volume,
        LAST_VALUE(close_time) OVER w AS close_time,
        MAX(interval) OVER w AS interval,
        FIRST_VALUE(first_trade_id) OVER w AS first_trade_id,
        LAST_VALUE(last_trade_id) OVER w AS last_trade_id,
        LAST_VALUE(batch_timestamp) OVER w AS batch_timestamp,
        COUNT(*) OVER w AS minute_count,
        EXTRACT(MINUTE FROM candle_begin_time)::INTEGER AS minute_index
    FROM binance_spot_kline_history
    WINDOW w AS (
        PARTITION BY inst_id
        ORDER BY candle_begin_time
        RANGE BETWEEN INTERVAL '59 minutes' PRECEDING AND CURRENT ROW
    )
)
SELECT
    id,
    inst_id,
    candle_begin_time - INTERVAL '59 minutes' AS candle_begin_time,
    candle_begin_time + INTERVAL '1 minute' AS window_end,
    open,
    high,
    low,
    close,
    volume,
    quote_volume,
    number_of_trades,
    taker_buy_base_asset_volume,
    taker_buy_quote_asset_volume,
    close_time,
    interval,
    first_trade_id,
    last_trade_id,
    batch_timestamp,
    minute_count,
    minute_index
FROM rolling
WHERE minute_count = 60;

CREATE OR REPLACE VIEW binance_spot_kline_rolling_5m AS
WITH rolling AS (
    SELECT
        LAST_VALUE(id) OVER w AS id,
        inst_id,
        candle_begin_time,
        FIRST_VALUE(open) OVER w AS open,
        MAX(high) OVER w AS high,
        MIN(low) OVER w AS low,
        LAST_VALUE(close) OVER w AS close,
        SUM(volume) OVER w AS volume,
        SUM(quote_volume) OVER w AS quote_volume,
        (SUM(number_of_trades) OVER w)::BIGINT AS number_of_trades,
        SUM(taker_buy_base_asset_volume) OVER w AS taker_buy_base_asset_volume,
        SUM(taker_buy_quote_asset_volume) OVER w AS taker_buy_quote_asset_volume,
        LAST_VALUE(close_time) OVER w AS close_time,
        MAX(interval) OVER w AS interval,
        FIRST_VALUE(first_trade_id) OVER w AS first_trade_id,
        LAST_VALUE(last_trade_id) OVER w AS last_trade_id,
        LAST_VALUE(batch_timestamp) OVER w AS batch_timestamp,
        COUNT(*) OVER w AS minute_count,
        EXTRACT(MINUTE FROM candle_begin_time)::INTEGER % 5 AS minute_index
    FROM binance_spot_kline_history
    WINDOW w AS (
        PARTITION BY inst_id
        ORDER BY candle_begin_time
        RANGE BETWEEN INTERVAL '4 minutes' PRECEDING AND CURRENT ROW
    )
)
SELECT
    id,
    inst_id,
    candle_begin_time - INTERVAL '4 minutes' AS candle_begin_time,
    candle_begin_time + INTERVAL '1 minute' AS window_end,
    open,
    high,
    low,
    close,
    volume,
    quote_volume,
    number_of_trades,
    taker_buy_base_asset_volume,
    taker_buy_quote_asset_volume,
    close_time,
    interval,
    first_trade_id,
    last_trade_id,
    batch_timestamp,
    minute_count,
    minute_index
FROM rolling
WHERE minute_count = 5;

CREATE OR REPLACE VIEW binance_spot_kline_rolling_15m AS
WITH rolling AS (
    SELECT
        LAST_VALUE(id) OVER w AS id,
        inst_id,
        candle_begin_time,
        FIRST_VALUE(open) OVER w AS open,
        MAX(high) OVER w AS high,
        MIN(low) OVER w AS low,
        LAST_VALUE(close) OVER w AS close,
        SUM(volume) OVER w AS volume,
        SUM(quote_volume) OVER w AS quote_volume,
        (SUM(number_of_trades) OVER w)::BIGINT AS number_of_trades,
        SUM(taker_buy_base_asset_volume) OVER w AS taker_buy_base_asset_volume,
        SUM(taker_buy_quote_asset_volume) OVER w AS taker_buy_quote_asset_volume,
        LAST_VALUE(close_time) OVER w AS close_time,
        MAX(interval) OVER w AS interval,
        FIRST_VALUE(first_trade_id) OVER w AS first_trade_id,
        LAST_VALUE(last_trade_id) OVER w AS last_trade_id,
        LAST_VALUE(batch_timestamp) OVER w AS batch_timestamp,
        COUNT(*) OVER w AS minute_count,
        EXTRACT(MINUTE FROM candle_begin_time)::INTEGER % 15 AS minute_index
    FROM binance_spot_kline_history
    WINDOW w AS (
        PARTITION BY inst_id
        ORDER BY candle_begin_time
        RANGE BETWEEN INTERVAL '14 minutes' PRECEDING AND CURRENT ROW
    )
)
SELECT
    id,
    inst_id,
    candle_begin_time - INTERVAL '14 minutes' AS candle_begin_time,
    candle_begin_time + INTERVAL '1 minute' AS window_end,
    open,
    high,
    low,
    close,
    volume,
    quote_volume,
    number_of_trades,
    taker_buy_base_asset_volume,
    taker_buy_quote_asset_volume,
    close_time,
    interval,
    first_trade_id,
    last_trade_id,
    batch_timestamp,
    minute_count,
    minute_index
FROM rolling
WHERE minute_count = 15;

CREATE OR REPLACE VIEW binance_swap_kline_rolling_5m AS
WITH rolling AS (
    SELECT
        LAST_VALUE(id) OVER w AS id,
        inst_id,
        candle_begin_time,
        FIRST_VALUE(open) OVER w AS open,
        MAX(high) OVER w AS high,
        MIN(low) OVER w AS low,
        LAST_VALUE(close) OVER w AS close,
        SUM(volume) OVER w AS volume,
        SUM(quote_volume) OVER w AS quote_volume,
        (SUM(number_of_trades) OVER w)::BIGINT AS number_of_trades,
        SUM(taker_buy_base_asset_volume) OVER w AS taker_buy_base_asset_volume,
        SUM(taker_buy_quote_asset_volume) OVER w AS taker_buy_quote_asset_volume,
        LAST_VALUE(close_time) OVER w AS close_time,
        MAX(interval) OVER w AS interval,
        FIRST_VALUE(first_trade_id) OVER w AS first_trade_id,
        LAST_VALUE(last_trade_id) OVER w AS last_trade_id,
        LAST_VALUE(batch_timestamp) OVER w AS batch_timestamp,
        COUNT(*) OVER w AS minute_count,
        EXTRACT(MINUTE FROM candle_begin_time)::INTEGER % 5 AS minute_index
    FROM binance_swap_kline_history
    WINDOW w AS (
        PARTITION BY inst_id
        ORDER BY candle_begin_time
        RANGE BETWEEN INTERVAL '4 minutes' PRECEDING AND CURRENT ROW
    )
)
SELECT
    id,
    inst_id,
    candle_begin_time - INTERVAL '4 minutes' AS candle_begin_time,
    candle_begin_time + INTERVAL '1 minute' AS window_end,
    open,
    high,
    low,
    close,
    volume,
    quote_volume,
    number_of_trades,
    taker_buy_base_asset_volume,
    taker_buy_quote_asset_volume,
    close_time,
    interval,
    first_trade_id,
    last_trade_id,
    batch_timestamp,
    minute_count,
    minute_index
FROM rolling
WHERE minute_count = 5;

CREATE OR REPLACE VIEW binance_swap_kline_rolling_15m AS
WITH rolling AS (
    SELECT
        LAST_VALUE(id) OVER w AS id,
        inst_id,
        candle_begin_time,
        FIRST_VALUE(open) OVER w AS open,
        MAX(high) OVER w AS high,
        MIN(low) OVER w AS low,
        LAST_VALUE(close) OVER w AS close,
        SUM(volume) OVER w AS volume,
        SUM(quote_volume) OVER w AS quote_volume,
        (SUM(number_of_trades) OVER w)::BIGINT AS number_of_trades,
        SUM(taker_buy_base_asset_volume) OVER w AS taker_buy_base_asset_volume,
        SUM(taker_buy_quote_asset_volume) OVER w AS taker_buy_quote_asset_volume,
        LAST_VALUE(close_time) OVER w AS close_time,
        MAX(interval) OVER w AS interval,
        FIRST_VALUE(first_trade_id) OVER w AS first_trade_id,
        LAST_VALUE(last_trade_id) OVER w AS last_trade_id,
        LAST_VALUE(batch_timestamp) OVER w AS batch_timestamp,
        COUNT(*) OVER w AS minute_count,
        EXTRACT(MINUTE FROM candle_begin_time)::INTEGER % 15 AS minute_index
    FROM binance_swap_kline_history
    WINDOW w AS (
        PARTITION BY inst_id
        ORDER BY candle_begin_time
        RANGE BETWEEN INTERVAL '14 minutes' PRECEDING AND CURRENT ROW
    )
)
SELECT
    id,
    inst_id,
    candle_begin_time - INTERVAL '14 minutes' AS candle_begin_time,
    candle_begin_time + INTERVAL '1 minute' AS window_end,
    open,
    high,
    low,
    close,
    volume,
    quote_volume,
    number_of_trades,
    taker_buy_base_asset_volume,
    taker_buy_quote_asset_volume,
    close_time,
    interval,
    first_trade_id,
    last_trade_id,
    batch_timestamp,
    minute_count,
    minute_index
FROM rolling
WHERE minute_count = 15;

CREATE OR REPLACE VIEW binance_swap_kline_rolling_1h AS
WITH rolling AS (
    SELECT
        LAST_VALUE(id) OVER w AS id,
        inst_id,
        candle_begin_time,
        FIRST_VALUE(open) OVER w AS open,
        MAX(high) OVER w AS high,
        MIN(low) OVER w AS low,
        LAST_VALUE(close) OVER w AS close,
        SUM(volume) OVER w AS volume,
        SUM(quote_volume) OVER w AS quote_volume,
        (SUM(number_of_trades) OVER w)::BIGINT AS number_of_trades,
        SUM(taker_buy_base_asset_volume) OVER w AS taker_buy_base_asset_volume,
        SUM(taker_buy_quote_asset_volume) OVER w AS taker_buy_quote_asset_volume,
        LAST_VALUE(close_time) OVER w AS close_time,
        MAX(interval) OVER w AS interval,
        FIRST_VALUE(first_trade_id) OVER w AS first_trade_id,
        LAST_VALUE(last_trade_id) OVER w AS last_trade_id,
        LAST_VALUE(batch_timestamp) OVER w AS batch_timestamp,
        COUNT(*) OVER w AS minute_count,
        EXTRACT(MINUTE FROM candle_begin_time)::INTEGER AS minute_index
    FROM binance_swap_kline_history
    WINDOW w AS (
        PARTITION BY inst_id
        ORDER BY candle_begin_time
        RANGE BETWEEN INTERVAL '59 minutes' PRECEDING AND CURRENT ROW
    )
)
SELECT
    id,
    inst_id,
    candle_begin_time - INTERVAL '59 minutes' AS candle_begin_time,
    candle_begin_time + INTERVAL '1 minute' AS window_end,
    open,
    high,
    low,
    close,
    volume,
    quote_volume,
    number_of_trades,
    taker_buy_base_asset_volume,
    taker_buy_quote_asset_volume,
    close_time,
    interval,
    first_trade_id,
    last_trade_id,
    batch_timestamp,
    minute_count,
    minute_index
FROM rolling
WHERE minute_count = 60;