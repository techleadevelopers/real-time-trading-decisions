# Neural Edge Trading

Neural Edge Trading is a hybrid crypto trading research and execution project.

It currently has two distinct layers:

- `backend/`: Python/FastAPI research API for candle-based data collection, baseline features, model training, signal inspection, and backtest endpoints.
- `rtts/`: Rust + Tokio real-time trading system focused on event-driven, microstructure-aware scalp execution.
- `control-plane/`: Go orchestration and real-time control layer for market-data gateway, risk, state, execution routing, REST, and WebSocket operations.

The Rust RTTS is the low-latency decision and execution-intelligence core. It is not candle-based. It consumes trade prints and L2 order book updates, builds microstructure state, evaluates multiple scenarios, and emits execution decisions.

The Go control-plane is the operational brain. It receives execution requests from Rust, applies idempotency and global risk controls, maintains order/position state, and forwards approved orders to the exchange API layer. The current exchange implementation is paper trading and is designed to be replaced by an authenticated exchange client.

The `frontend/` directory is intentionally private/local and ignored by Git. It is not part of the public repository.

---

## Current Architecture

```text
neural-edge-trading/
  backend/              # FastAPI research and model API
  rtts/                 # Rust real-time trading system
  control-plane/        # Go orchestration and execution control service
  docker-compose.yml    # Backend/db/redis local stack
  .env.example          # Environment template
  README.md             # This file
```

Ignored local-only paths:

```text
frontend/
rtts/target/
control-plane/bin/
control-plane/dist/
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
    queue_position.rs    # Queue position, volume-ahead, fill probability estimate
    fill_probability.rs  # High/low fill classification from queue and flow
    execution_mode.rs    # Aggressive/passive/defensive mode switching
    adverse_selection.rs # Pre/post-fill adverse selection scoring
    micro_exit.rs        # Take-profit, fade, adverse-flow, liquidity-collapse exits
    markout.rs           # 100ms/500ms/1s post-entry markout estimates
    symbol_profile.rs    # Per-symbol spread/fill/volatility profile
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
12. `queue_position`, `fill_probability`, and `execution_mode` estimate queue position, volume ahead, fill probability, and switch between aggressive, passive, and defensive execution.
13. `execution_smart` chooses market vs limit only after approval, then simulates partial fills, cancel/replace, slippage, adverse-selection rejection, immediate defensive exits, and learning feedback.
14. `micro_exit` and `markout` evaluate take-profit, momentum fade, adverse flow, liquidity collapse, and 100ms/500ms/1s post-entry quality.
15. `symbol_profile` keeps per-symbol spread, fill probability, volatility, and trade-size estimates to adapt execution.
16. `learning` adjusts thresholds, feature weights, and scaling aggressiveness using lightweight exponential updates from slippage, entry quality, PnL, duration, and markouts.
17. `metrics` exposes latency, EV, entry quality, competition score, skipped/executed decisions, slippage, microtrade PnL, hit rate by regime, scale efficiency, position size, drawdown, and backpressure.

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
- queue position estimate
- fill probability
- execution mode
- adverse selection risk
- symbol-specific spread/fill behavior

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

## Control Plane: Go Operational Layer

The Go `control-plane` service connects research, RTTS decisions, market-data operations, risk controls, and execution routing.

Responsibilities:

- Binance WebSocket market-data gateway for trades and top L2 depth.
- Goroutine/channel event pipeline with bounded buffers.
- Backpressure handling in ingestion and update streaming.
- HTTP execution endpoint for Rust RTTS decisions.
- Idempotency keys per order.
- Pre-trade risk checks: kill switch, circuit breaker, exposure, position limits, stale signal rejection.
- In-memory position and order state.
- Order lifecycle state machine: `NEW -> SENT -> PARTIAL -> FILLED -> CANCELED`.
- REST API and WebSocket live updates.

Structure:

```text
control-plane/
  cmd/control-plane/main.go
  internal/
    api/          # REST and WebSocket gateway
    config/       # env-driven configuration
    domain/       # shared structs
    execution/    # execution gateway and exchange client interface
    marketdata/   # Binance websocket gateway
    pipeline/     # event processing
    risk/         # kill switch, breakers, exposure checks
    state/        # in-memory positions/orders/idempotency
  Dockerfile
  README.md
```

Run:

```powershell
cd control-plane
go mod tidy
go run ./cmd/control-plane
```

Verify:

```powershell
cd control-plane
go test -count=1 ./...
```

API:

```text
GET  /health
GET  /status
GET  /positions
GET  /risk
POST /kill-switch
POST /execution/requests
GET  /ws
```

Example Rust RTTS execution request:

```json
{
  "idempotency_key": "BTCUSDT-1700000000-1",
  "symbol": "BTCUSDT",
  "side": "BUY",
  "size": 0.001,
  "price": 67000.0,
  "decision": "Execute",
  "signal_time": "2026-04-22T12:00:00Z",
  "max_slippage_bps": 3.0,
  "reduce_only": false
}
```

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
