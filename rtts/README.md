# Scalp Sniper RTTS

Production-oriented Rust skeleton for a microstructure-aware crypto scalp engine. It is intentionally not candle based: every decision is derived from raw trade prints and L2 book deltas flowing through bounded async channels.

## Structure

```text
rtts/
  Cargo.toml
  src/
    main.rs             # CLI, tracing, pipeline start
    config.rs           # runtime config, latency, and risk knobs
    ingestion.rs        # Binance websocket or deterministic mock feed
    orderbook.rs        # delta L2 book, walls, spoof/pull/absorption
    tape.rs             # aggressive flow, delta, bursts, exhaustion
    flow_intelligence.rs # O(1) continuation/exhaustion/reversal flow state
    micro_timing.rs     # spread compression, pulls, bursts, pullbacks
    context_engine.rs   # O(1) market context and regime classification
    microstructure.rs   # normalized features and regime classification
    adaptive_engine.rs  # dynamic scoring and adversarial defense
    scenario_simulator.rs # continuation/reversal/chop outcome estimates
    ev_calculator.rs    # slippage/latency-adjusted expected value
    entry_quality.rs    # timing/liquidity/orderflow execution score
    competition_model.rs # opportunity crowding and consumed-edge risk
    meta_engine.rs      # final judge: execute, wait, or skip
    learning.rs         # exponential online threshold/weight updates
    position.rs         # one evolving position with scale/decay logic
    risk.rs             # hard risk, stale-data, DD, kill-switch checks
    execution_smart.rs  # market/limit choice, partial fill, replace
    metrics.rs          # Prometheus text endpoint
    pipeline.rs         # bounded mpsc wiring
    types.rs            # shared domain structs
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
$env:RTTS_MAX_DATA_AGE_MS="250"
$env:RTTS_MAX_DECISION_LATENCY_US="1500"
$env:RTTS_MAX_EXECUTION_LATENCY_US="8000"
$env:RTTS_MAX_CONSECUTIVE_LOSSES="3"
```

## Pipeline

1. `ingestion` emits raw `TradeEvent` and `BookDelta` updates. Binance depth updates are applied as deltas; mock mode generates deterministic book/trade pressure.
2. `orderbook` maintains bid/ask depth by price tick and computes total depth, top pressure, weighted imbalance, liquidity clusters, spoofing, liquidity pulls, and absorption.
3. `tape` tracks aggressive buy/sell volume, delta, trade frequency, volume bursts, exhaustion, and continuation.
4. `flow_intelligence` classifies flow as strong continuation, weak continuation, exhaustion, or reversal risk.
5. `micro_timing` scores spread compression, liquidity pulls, trade bursts, and micro pullbacks to decide whether timing is optimal, neutral, waiting, or missed.
6. `context_engine` classifies `Normal`, `HighVolatility`, `NewsShock`, `LowLiquidity`, and `TrendExpansion` from volatility spikes, spread widening, depth collapse, trade velocity bursts, and imbalance shifts.
7. `microstructure` normalizes features online and emits numeric `MarketRegime`, compact `MarketContext`, flow state, and timing state.
8. `adaptive_engine` uses dynamic weights driven by regime, spread, recent hit rate, flow, and timing; it outputs direction, confidence, urgency, expected duration, and pre-trade slippage.
9. `position` treats entries and scale-ins as one evolving position. It opens micro size first, scales only on confirmed flow/timing/liquidity, reduces size in low liquidity, and allows more scale in trend expansion.
10. `risk` rejects stale, over-budget, over-risk, and abnormal orders before meta evaluation.
11. `meta_engine` is the final judge. It blocks news-shock context, tightens thresholds in unstable regimes, simulates continuation/reversal/chop, computes slippage/latency-adjusted EV, scores entry quality, estimates competition, waits for confirmation when needed, then returns execute, wait, or skip.
12. `execution_smart` chooses market vs limit only after the meta decision approves the order, then simulates queue/partial fills, cancels/replaces, records slippage, rejects adverse selection, and feeds learning samples back.
13. `learning` adjusts thresholds, feature weights, and scaling aggressiveness with exponential updates from slippage, entry quality, PnL, and duration.
14. `metrics` exposes stage/execution latency, adjusted EV, entry quality, competition score, skipped/executed decisions, slippage, microtrade PnL, hit rate by regime, scale efficiency, position size, drawdown, and backpressure.

## Safety Notes

This crate defaults to paper execution. Real execution should be added behind the execution module boundary with persistent authenticated exchange sessions, exchange-native order acknowledgements, idempotent client order IDs, and reconciliation before any capital is exposed.

## Reality Check

This is a professional architecture skeleton, not a colocated HFT stack. It still loses to serious firms on exchange proximity, private market data, queue position modeling, hardware timestamping, kernel bypass networking, and production OMS/reconciliation depth. Treat the adaptive model as a control layer for paper/forward testing until execution telemetry proves the edge survives fees, queue loss, adverse selection, and real exchange throttles.
