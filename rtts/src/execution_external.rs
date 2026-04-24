use crate::{
    accounting::{
        latency::LatencyBreakdown,
        ledger::LiquidityFlag,
    },
    config::Config,
    metrics::Metrics,
    types::{ExecutionMode, ExecutionTruth, FillEvent, MarkoutSnapshot, MicroExitSignal, QueueEstimate, Side},
};
use futures_util::StreamExt;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::connect_async;
use tracing::{info, warn};

#[derive(Debug, Deserialize)]
struct WsUpdate {
    #[serde(rename = "type")]
    kind: String,
    data: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct ControlPlaneExecutionUpdate {
    order: ControlPlaneOrder,
    ledger: Option<ControlPlaneLedgerEntry>,
    execution: Option<ControlPlaneExecutionEvent>,
    request_timestamp_ms: i64,
    send_timestamp_ms: i64,
    ack_timestamp_ms: i64,
    first_fill_timestamp_ms: i64,
    last_fill_timestamp_ms: i64,
    expected_realized_markout: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ControlPlaneOrder {
    id: String,
    symbol: String,
    side: String,
    size: f64,
    price: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ControlPlaneLedgerEntry {
    order_id: String,
    fill_id: String,
    symbol: String,
    side: String,
    price: f64,
    quantity: f64,
    liquidity_flag: String,
    fee_amount: f64,
    fee_asset: String,
    rebate_amount: f64,
    funding_amount: f64,
    event_time_unix_ms: i64,
}

#[derive(Debug, Deserialize)]
struct ControlPlaneExecutionEvent {
    slippage_real: f64,
    partial_fill_ratio: f64,
    latency_breakdown: ControlPlaneLatencyBreakdown,
}

#[derive(Debug, Deserialize)]
struct ControlPlaneLatencyBreakdown {
    decision_latency: u64,
    send_latency: u64,
    ack_latency: u64,
    first_fill_latency: u64,
    full_fill_latency: u64,
}

pub async fn run(
    cfg: Config,
    fill_tx: Sender<FillEvent>,
    truth_fill_tx: Sender<FillEvent>,
    metrics: Arc<Metrics>,
) {
    let ws_url = cfg.control_plane_ws.clone();
    loop {
        match connect_async(&ws_url).await {
            Ok((mut stream, _)) => {
                info!(url = %ws_url, "connected to control-plane execution feed");
                while let Some(message) = stream.next().await {
                    match message {
                        Ok(msg) if msg.is_text() => {
                            if let Ok(update) = serde_json::from_str::<WsUpdate>(msg.to_text().unwrap_or_default()) {
                                handle_update(update, &fill_tx, &truth_fill_tx, &metrics).await;
                            }
                        }
                        Ok(_) => {}
                        Err(err) => {
                            warn!(%err, "execution feed websocket error");
                            break;
                        }
                    }
                }
            }
            Err(err) => warn!(%err, url = %ws_url, "failed to connect to control-plane execution feed"),
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

async fn handle_update(
    update: WsUpdate,
    fill_tx: &Sender<FillEvent>,
    truth_fill_tx: &Sender<FillEvent>,
    metrics: &Metrics,
) {
    if update.kind != "execution_update" {
        return;
    }
    let Ok(payload) = serde_json::from_value::<ControlPlaneExecutionUpdate>(update.data) else {
        return;
    };
    let (Some(ledger), Some(execution)) = (payload.ledger, payload.execution) else {
        return;
    };

    let side = parse_side(&ledger.side);
    let fill = FillEvent {
        order_id: ledger.order_id,
        fill_id: ledger.fill_id,
        symbol: ledger.symbol.clone(),
        side,
        size: payload.order.size,
        price: ledger.price,
        requested_price: payload.order.price.unwrap_or(ledger.price),
        filled_size: ledger.quantity,
        remaining_size: (payload.order.size - ledger.quantity).max(0.0),
        liquidity_flag: parse_liquidity_flag(&ledger.liquidity_flag),
        fee: ledger.fee_amount,
        fee_asset: ledger.fee_asset,
        rebate_amount: ledger.rebate_amount,
        funding_amount: ledger.funding_amount,
        timestamp: non_negative_ms(ledger.event_time_unix_ms),
        latency_us: (execution.latency_breakdown.full_fill_latency / 1_000) as u64,
        latency_breakdown: LatencyBreakdown {
            decision_latency_us: execution.latency_breakdown.decision_latency / 1_000,
            send_latency_us: execution.latency_breakdown.send_latency / 1_000,
            ack_latency_us: execution.latency_breakdown.ack_latency / 1_000,
            first_fill_latency_us: execution.latency_breakdown.first_fill_latency / 1_000,
            full_fill_latency_us: execution.latency_breakdown.full_fill_latency / 1_000,
        },
        expected_markout: payload.expected_realized_markout.unwrap_or_default(),
        expected_slippage_bps: 0.0,
        actual_slippage_bps: execution.slippage_real,
        queue_estimate: QueueEstimate::default(),
        execution_mode: infer_execution_mode(payload.order.price),
        micro_exit: MicroExitSignal::default(),
        markout: MarkoutSnapshot::default(),
        complete: execution.partial_fill_ratio >= 0.999,
        truth: ExecutionTruth {
            request_timestamp: non_negative_ms(payload.request_timestamp_ms),
            send_timestamp: non_negative_ms(payload.send_timestamp_ms),
            ack_timestamp: non_negative_ms(payload.ack_timestamp_ms),
            exchange_accept_timestamp: non_negative_ms(payload.ack_timestamp_ms),
            first_fill_timestamp: non_negative_ms(payload.first_fill_timestamp_ms),
            last_fill_timestamp: non_negative_ms(payload.last_fill_timestamp_ms),
            partial_fill_ratio: execution.partial_fill_ratio,
            cancel_reason: None,
            reject_reason: None,
            spread_at_execution: 0.0,
            queue_delay_us: (execution.latency_breakdown.first_fill_latency / 1_000) as u64,
            simulated: false,
        },
    };

    metrics.fills_total.with_label_values(&[side_label(side)]).inc();
    metrics
        .execution_latency_us
        .with_label_values(&["external"])
        .observe(fill.latency_breakdown.full_fill_latency_us as f64);
    metrics
        .slippage_bps
        .with_label_values(&[side_label(side)])
        .observe(fill.actual_slippage_bps);

    let _ = truth_fill_tx.try_send(fill.clone());
    if fill_tx.try_send(fill).is_err() {
        metrics
            .channel_backpressure_total
            .with_label_values(&["execution_external"])
            .inc();
    }
}

fn parse_side(raw: &str) -> Side {
    match raw {
        "SELL" => Side::Sell,
        _ => Side::Buy,
    }
}

fn parse_liquidity_flag(raw: &str) -> LiquidityFlag {
    match raw {
        "MAKER" => LiquidityFlag::Maker,
        _ => LiquidityFlag::Taker,
    }
}

fn infer_execution_mode(price: Option<f64>) -> ExecutionMode {
    if price.is_some() {
        ExecutionMode::Passive
    } else {
        ExecutionMode::Aggressive
    }
}

fn side_label(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

fn non_negative_ms(value: i64) -> u64 {
    value.max(0) as u64
}
