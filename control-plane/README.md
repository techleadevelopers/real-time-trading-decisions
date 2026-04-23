# control-plane

Go orchestration and real-time control layer for the hybrid trading system.

Responsibilities:

- Binance market-data gateway for trades and L2 top depth.
- Goroutine/channel event pipeline with bounded buffers and backpressure drops.
- Execution gateway for Rust RTTS requests over HTTP.
- Idempotency by order key.
- Pre-trade risk validation.
- In-memory position and order state.
- Global kill switch and per-symbol circuit breakers.
- REST API and WebSocket update stream.

Run:

```powershell
cd control-plane
go mod tidy
go run ./cmd/control-plane
```

REST:

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

Environment:

```text
CONTROL_PLANE_ADDR=:8088
CONTROL_PLANE_SYMBOLS=btcusdt,ethusdt
MD_BUFFER=8192
UPDATE_BUFFER=8192
RISK_MAX_EXPOSURE_USD=10000
RISK_MAX_POSITION_USD=2500
RISK_MAX_DAILY_LOSS_USD=250
RISK_MAX_SIGNAL_AGE=500ms
RISK_LATENCY_REJECT_AFTER=150ms
LOG_LEVEL=info
```

The exchange layer is currently `PaperExchange`. Replace `execution.ExchangeClient`
with an authenticated exchange client for production order routing.

