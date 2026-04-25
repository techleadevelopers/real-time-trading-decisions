# Neural Edge Trading

Neural Edge Trading is a hybrid crypto trading research and execution project.

It currently has two distinct layers:

- `backend/`: Python/FastAPI research API for candle-based data collection, baseline features, model training, signal inspection, and backtest endpoints.
- `rtts/`: Rust + Tokio real-time trading system focused on event-driven, microstructure-aware scalp execution.
- `control-plane/`: Go orchestration and real-time control layer for market-data gateway, risk, state, execution routing, REST, and WebSocket operations.

The Rust RTTS is the low-latency decision and execution-intelligence core. It is not candle-based. It consumes trade prints and L2 order book updates, builds microstructure state, evaluates multiple scenarios, and emits execution decisions.

The Go control-plane is the operational brain and the authoritative execution layer. It receives execution requests from Rust, applies idempotency and global risk controls, owns the order lifecycle, emits exchange-derived execution events and fill-ledger entries, maintains reconciliation state, synchronizes exchange-side account state, and forwards approved orders to the exchange API layer.

The `frontend/` directory is intentionally private/local and ignored by Git. It is not part of the public repository.

---

## Current Architecture

```text
neural-edge-trading/
  backend/              # FastAPI research and model API
  rtts/                 # Rust real-time trading system
  control-plane/        # Go orchestration, BingX execution, and execution control service
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

The Rust crate is a production-oriented microstructure decision engine. It is designed around bounded Tokio channels, deterministic state transitions, external execution truth, ledger-based accounting, continuous statistical edge validation, and competition-aware edge capture control.

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
-> control-plane execution request
-> external execution event feed
-> accounting truth
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
    execution_smart.rs   # Execution request preparation + control-plane submission
    execution_external.rs # Control-plane WebSocket execution event consumer
    accounting/          # Ledger, latency distributions, edge validation, quality, validation
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
8. `adaptive_engine` produces direction, confidence, urgency, expected duration, and pre-trade slippage, while filtering missed timing and reversal-risk flow and degrading decisions when the statistical edge is uncertain, invalid, competitive, or saturated.
9. `position` consumes externally sourced fills and synchronizes the local position snapshot from accounting truth instead of deriving truth from local execution simulation.
10. `risk` rejects stale, over-budget, over-risk, and abnormal orders before meta evaluation.
11. `meta_engine` is the final judge. It simulates continuation/reversal/chop, computes adjusted EV, scores entry quality, estimates competition, waits for confirmation when needed, and returns `Execute`, `Wait`, or `Skip`. It now explicitly avoids saturated edge-crowding states and degrades under competitive conditions.
12. `queue_position`, `fill_probability`, and `execution_mode` estimate queue position, volume ahead, fill probability, and preferred aggressiveness before any order request leaves Rust.
13. `execution_smart` prepares the execution request and sends it to the Go control-plane. It no longer generates fills or acts as execution truth.
14. `execution_external` consumes authoritative `execution_update` events from the control-plane WebSocket feed and forwards external fills into position/accounting/truth processing.
15. `accounting` computes lot-based realized PnL from fill-ledger entries only. It supports partial fills, mixed maker/taker fees, rebates, funding fields, and unrealized PnL as derived state.
16. `execution_truth` measures realized markout, slippage, fill quality, edge capture ratio, adverse selection, and per-trade PnL decomposition from external fills only. It feeds online learning with real outcomes and exposes `edge_component`, `execution_loss`, `fees/rebates`, and `adverse_selection_loss`.
17. `accounting::edge_validation` runs rolling t-tests on realized PnL, KS-tests on expected vs realized edge distributions, tracks edge error moments, edge capture efficiency, confidence intervals, Sharpe-like adjusted returns, and edge half-life, classifies `VALID/UNCERTAIN/INVALID`, classifies competition as `NORMAL/COMPETITIVE/SATURATED`, and computes a dynamic capital multiplier.
18. `micro_exit` and `markout` evaluate take-profit, momentum fade, adverse flow, liquidity collapse, and 100ms/500ms/1s post-entry quality, but these are not accounting truth.
19. `symbol_profile` keeps per-symbol spread, fill probability, volatility, and trade-size estimates to adapt execution.
20. `learning` adjusts thresholds, feature weights, and scaling aggressiveness using execution outcomes and post-trade quality samples.
21. `accounting::edge_validation` also stores regime-aware memory of reliability, PnL, execution quality, and capture quality so thresholds, sizing, and aggressiveness adapt by regime instead of using one global edge assumption.
22. Anti-overfitting guards are enforced online: minimum sample size before validation, confidence intervals, noise filtering, and slow EWMA decay factors to avoid reacting to small-sample noise.
23. `metrics` exposes latency, EV, entry quality, competition score, skipped/executed decisions, slippage, microtrade PnL, hit rate by regime, scale efficiency, position size, drawdown, controller efficiency, cancel/replace intensity, and backpressure.

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
$env:RTTS_CONTROL_PLANE_HTTP="http://127.0.0.1:8088"
$env:RTTS_CONTROL_PLANE_WS="ws://127.0.0.1:8088/ws"
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
- rolling statistical edge validity
- edge reliability and decay state
- edge capture efficiency versus expected edge
- per-trade PnL decomposition quality
- competition regime: `NORMAL`, `COMPETITIVE`, or `SATURATED`
- regime-specific edge memory and execution quality
- minimum statistical sample size and confidence interval stability
- current drawdown-adjusted capital multiplier

After approval, Rust submits an execution request to the control-plane. The RTTS does not manufacture fills locally. Order acknowledgements, partial fills, final fills, and cancel states come back from the control-plane as external execution events.

The execution controller layer now actively manages live orders after submission:

- re-evaluates queue position, fill probability, elapsed time, and competition on each update
- issues `Cancel`, `Replace`, `SwitchStrategy`, or `Abort`
- aborts orders when edge half-life is exceeded
- feeds execution failures back into adaptive edge validation

The statistical edge engine continuously tests whether trading is justified at all:

- `VALID`: trading allowed under normal risk limits
- `UNCERTAIN`: trading degraded with smaller size and tighter thresholds
- `INVALID`: trading halted until real execution evidence improves

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
- In-memory position, order, fill-ledger, account, and reconciliation state with durability-ready hooks.
- Authoritative order lifecycle state machine: `NEW -> SENT -> ACK -> PARTIAL -> FILLED/CANCELED`.
- Authoritative execution events and fill-ledger entries emitted from the exchange layer.
- Reconciliation check: `sum(exchange fills) == sum(accounting ledger)`.
- BingX authenticated REST trading and authenticated WebSocket execution updates.
- Boot-time reconciliation of open orders, positions, recent fills, and account state before trading starts.
- Self-trade prevention by symbol and `client_order_id` idempotency.
- `GET /account/state` for available balance, margin, leverage, and unrealized PnL.
- REST API and WebSocket live updates.

Structure:

```text
control-plane/
  cmd/control-plane/main.go
  internal/
    api/          # REST and WebSocket gateway
    config/       # env-driven configuration
    domain/       # shared structs
    execution/    # execution gateway plus paper and BingX exchange clients
    marketdata/   # Binance websocket gateway
    pipeline/     # event processing
    risk/         # kill switch, breakers, exposure checks
    state/        # in-memory positions/orders/ledger/reconciliation
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
GET  /execution/ledger
GET  /execution/events
GET  /execution/reconciliation
GET  /account/state
POST /kill-switch
POST /execution/requests
POST /execution/orders/{id}/cancel
POST /execution/orders/{id}/replace
POST /execution/orders/{id}/strategy
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
  "reduce_only": false,
  "expected_realized_markout": 1.25
}
```

Useful control-plane environment variables:

```text
EXECUTION_EXCHANGE=bingx
BINGX_API_KEY=...
BINGX_SECRET_KEY=...
BINGX_BASE_URL=https://open-api.bingx.com
BINGX_WS_URL=wss://open-api-swap.bingx.com
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

The execution source of truth is now control-plane driven rather than RTTS-simulated. BingX integration is now wired into the control-plane with authenticated REST submission, authenticated user-stream execution updates, exchange-side account sync, and boot reconciliation.

- RTTS submits requests
- control-plane owns order lifecycle
- BingX exchange layer emits fills
- fill-ledger drives accounting
- reconciliation verifies ledger consistency

Remaining production work still includes:

- durable storage for orders, fills, and ledger state
- venue-specific fee/funding calibration
- queue position modeling
- production kill switches
- exchange-specific position/leverage calibration under real margin settings
- shadow/live soak testing before enabling real capital

This system still loses to institutional HFT firms on:

- colocation
- direct/private market data
- hardware timestamping
- kernel bypass networking
- queue modeling
- venue-specific execution infrastructure

The goal here is not to pretend to be colocated HFT. The goal is to enforce better decision quality, reduce false positives, control execution risk, and avoid trading when the edge is not statistically validated.
