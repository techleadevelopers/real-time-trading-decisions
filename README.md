# Neural Edge Trading

Neural Edge Trading is a hybrid crypto trading research and execution project.

It currently has two distinct layers:

- `backend/`: Python/FastAPI research API for candle-based data collection, baseline features, model training, signal inspection, and backtest endpoints.
- `rtts/`: Rust + Tokio real-time trading system focused on event-driven, microstructure-aware scalp execution.

The Rust RTTS is the execution-oriented core. It is not candle-based. It consumes trade prints and L2 order book updates, builds microstructure state, evaluates multiple scenarios, and only sends paper orders after risk and meta-decision validation.

The `frontend/` directory is intentionally private/local and ignored by Git. It is not part of the public repository.

---

## Current Architecture

```text
neural-edge-trading/
  backend/              # FastAPI research and model API
  rtts/                 # Rust real-time trading system
  docker-compose.yml    # Backend/db/redis local stack
  .env.example          # Environment template
  README.md             # This file
```

Ignored local-only paths:

```text
frontend/
rtts/target/
```

These are intentionally not pushed to GitHub.

---

## Backend: Python Research API

The Python backend remains useful for slower research workflows:

- candle collection from BingX/Binance
- baseline feature calculation
- deterministic Short Sniper rules
- logistic regression baseline model
- model train/predict endpoints
- backtest endpoint stubs
- health/data/model/regime routers

Main files:

```text
backend/
  app.py
  routers/
    health.py
    data.py
    model.py
    backtest.py
    regime.py
  services/
    collector.py
    features.py
    models.py
    rules.py
    market_stream.py
    metrics.py
```

Primary endpoints:

```text
GET  /health/
GET  /data/candles?symbol=NEARUSDT&interval=1m&limit=300
GET  /data/signals?symbol=NEARUSDT&interval=1m&limit=300
POST /model/train?symbol=NEARUSDT&interval=1m&limit=500
POST /model/predict?symbol=NEARUSDT&interval=1m&limit=500
```

Run backend stack:

```powershell
Copy-Item .env.example .env
docker-compose up --build
```

Backend docs:

```text
http://localhost:8000/docs
```

---

## RTTS: Rust Real-Time Trading System

The Rust crate is a production-oriented skeleton for a microstructure-aware scalp engine. It is designed around bounded Tokio channels, deterministic state transitions, and paper execution by default.

It shifts the execution path from:

```text
signal -> execute
```

to:

```text
market updates
-> microstructure state
-> adaptive decision
-> position/risk
-> multi-scenario meta-decision
-> smart paper execution
```

### RTTS Structure

```text
rtts/
  Cargo.toml
  src/
    main.rs              # CLI, tracing, pipeline start
    config.rs            # Runtime config, latency, and risk knobs
    ingestion.rs         # Binance websocket or deterministic mock feed
    orderbook.rs         # Delta L2 book, walls, spoof/pull/absorption
    tape.rs              # Aggressive flow, delta, bursts, exhaustion
    flow_intelligence.rs # O(1) continuation/exhaustion/reversal flow state
    micro_timing.rs      # Spread compression, liquidity pull, bursts, pullbacks
    context_engine.rs    # O(1) market context and regime classification
    microstructure.rs    # Normalized features and regime output
    adaptive_engine.rs   # Dynamic scoring and adversarial defense
    scenario_simulator.rs # Continuation/reversal/chop estimates
    ev_calculator.rs     # Slippage/latency-adjusted expected value
    entry_quality.rs     # Timing/liquidity/orderflow entry score
    competition_model.rs # Opportunity crowding and consumed-edge risk
    meta_engine.rs       # Final judge: execute, wait, or skip
    learning.rs          # Exponential online threshold/weight updates
    position.rs          # One evolving position with scale/decay logic
    risk.rs              # Hard risk, stale-data, DD, kill-switch checks
    execution_smart.rs   # Market/limit choice, partial fill, replace
    metrics.rs           # Prometheus text endpoint
    pipeline.rs          # Bounded mpsc wiring
    types.rs             # Shared domain structs
```

### RTTS Pipeline

1. `ingestion` emits raw `TradeEvent` and `BookDelta` updates.
2. `orderbook` maintains L2 bid/ask depth by price tick and computes depth, top pressure, weighted imbalance, liquidity clusters, spoofing, liquidity pulls, and absorption.
3. `tape` tracks aggressive buy/sell volume, delta, trade frequency, bursts, exhaustion, and continuation.
4. `flow_intelligence` classifies flow as strong continuation, weak continuation, exhaustion, or reversal risk.
5. `micro_timing` scores spread compression, liquidity pull, trade bursts, and micro pullbacks to decide whether entry timing is optimal, neutral, waiting, or missed.
6. `context_engine` classifies regimes: `Normal`, `HighVolatility`, `NewsShock`, `LowLiquidity`, and `TrendExpansion`.
7. `microstructure` normalizes features online and emits numeric market regime values plus compact `MarketContext`, flow, and timing state.
8. `adaptive_engine` produces direction, confidence, urgency, expected duration, and pre-trade slippage, while filtering missed timing and reversal-risk flow.
9. `position` treats entries and scale-ins as one evolving position. It opens micro size first, scales only on confirmed flow/timing/liquidity, reduces size in low liquidity, and allows more scale in trend expansion.
10. `risk` rejects stale, over-budget, over-risk, and abnormal orders before meta evaluation.
11. `meta_engine` is the final judge. It simulates continuation/reversal/chop, computes adjusted EV, scores entry quality, estimates competition, waits for confirmation when needed, and returns `Execute`, `Wait`, or `Skip`.
12. `execution_smart` chooses market vs limit only after approval, then simulates partial fills, cancel/replace, slippage, adverse-selection rejection, and learning feedback.
13. `learning` adjusts thresholds, feature weights, and scaling aggressiveness using lightweight exponential updates from slippage, entry quality, PnL, and duration.
14. `metrics` exposes latency, EV, entry quality, competition score, skipped/executed decisions, slippage, microtrade PnL, hit rate by regime, scale efficiency, position size, drawdown, and backpressure.

---

## Running RTTS

Mock feed:

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

Useful RTTS environment variables:

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

Validation:

```powershell
cd rtts
cargo fmt
cargo check
cargo test
```

---

## Core Trading Behavior

The RTTS does not execute just because a signal exists.

Before an order reaches execution, the system checks:

- microstructure direction
- orderflow alignment
- L2 depth and liquidity support
- volatility/spread regime
- stale data and latency
- risk budget
- scenario EV
- worst-case loss
- entry quality
- competition/crowding risk
- recent execution quality

Final decision:

```rust
enum FinalDecision {
    Execute,
    Wait,
    Skip,
}
```

`Skip` is a first-class decision. The system is designed to avoid overtrading.

---

## Market Context Regimes

The context engine is deterministic and hot-path safe. It uses no external APIs and no NLP.

Detected regimes:

- `Normal`
- `HighVolatility`
- `NewsShock`
- `LowLiquidity`
- `TrendExpansion`

Inputs:

- volatility spikes
- spread widening
- orderbook depth collapse
- trade velocity bursts
- sudden imbalance shifts
- liquidity pulls

Behavior:

- `NewsShock`: block execution.
- `LowLiquidity`: block new risk in meta engine and reduce size if risk-reducing logic is needed.
- `HighVolatility`: tighten thresholds.
- `TrendExpansion`: allow stronger scaling only when orderflow and liquidity support agree.
- `Normal`: use standard thresholds.

---

## Safety And Reality Check

This repository is educational and experimental. Crypto is highly volatile. This is not investment advice.

The Rust RTTS is still paper-execution focused. Real exchange execution needs:

- authenticated persistent order sessions
- client order IDs and idempotency
- exchange ACK/cancel/replace reconciliation
- queue position modeling
- self-trade prevention
- real markout analysis
- per-symbol calibration
- production kill switches

This system still loses to institutional HFT firms on:

- colocation
- direct/private market data
- hardware timestamping
- kernel bypass networking
- queue modeling
- venue-specific execution infrastructure

The goal here is not to pretend to be colocated HFT. The goal is to enforce better decision quality, reduce false positives, control execution risk, and avoid trading when the edge is not statistically validated.
