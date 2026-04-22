# Scalp Sniper RTTS

Production-oriented Rust skeleton for an event-driven crypto scalp engine. It is intentionally not candle based: every decision is derived from trade and L2 order book events flowing through bounded async channels.

## Structure

```text
rtts/
  Cargo.toml
  src/
    main.rs          # CLI, tracing, pipeline start
    config.rs        # runtime config and risk knobs
    ingestion.rs     # Binance websocket or deterministic mock feed
    event_engine.rs  # millisecond sliding-window pump/dump detection
    features.rs      # velocity, volume z-score, imbalance, vol, spread
    decision.rs      # direction-aware score and model filter
    model.rs         # lightweight logistic filter
    position.rs      # one evolving position with scale-in logic
    risk.rs          # hard trade risk, DD, kill-switch checks
    execution.rs     # non-blocking paper execution with retry/slippage
    metrics.rs       # Prometheus text endpoint
    pipeline.rs      # bounded mpsc wiring
    types.rs         # shared domain structs
```

## Run

Mock feed, useful for local validation:

```powershell
cd rtts
cargo run -- --exchange mock --symbol BTCUSDT
```

Binance websocket feed:

```powershell
cd rtts
cargo run -- --exchange binance --symbol BTCUSDT
```

Metrics:

```text
http://127.0.0.1:9898/metrics
```

Useful knobs:

```powershell
$env:RTTS_CHANNEL_CAP="4096"
$env:RTTS_WINDOW_MS="500"
$env:RTTS_MAX_RISK_PCT="0.005"
$env:RTTS_DAILY_DD_PCT="0.02"
$env:RTTS_BASE_ORDER_USD="25"
$env:RTTS_MAX_ENTRIES="4"
$env:RTTS_STOP_LOSS_BPS="25"
```

## Pipeline

1. `ingestion` emits compact `MarketEvent` structs from trades plus latest L2 spread/imbalance.
2. `event_engine` detects pump/dump/neutral in a millisecond sliding window.
3. `features` computes microstructure features without heap-heavy per-event allocations.
4. `decision` produces a continuous score in `[0, 1]` and applies a simple logistic filter.
5. `position` treats entries and scale-ins as one evolving position, never separate trades.
6. `risk` rejects orders that breach risk, drawdown, notional, or kill-switch constraints.
7. `execution` paper-fills orders with controlled slippage and sends fills back to `position`.
8. `metrics` exposes latency histograms, counters, position size, drawdown, and backpressure.

## Safety Notes

This crate defaults to paper execution. Real execution should be added behind the `execution` module boundary with persistent authenticated exchange sessions, exchange-native order acknowledgements, idempotent client order IDs, and reconciliation before any capital is exposed.

