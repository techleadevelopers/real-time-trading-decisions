use crate::{
    accounting::{
        latency::LatencyBreakdown,
        ledger::LiquidityFlag,
    },
    metrics::Metrics,
    types::{
        ExecutionMode, ExecutionTruth, FillEvent, MarkoutSnapshot, MicroExitSignal, OrderIntent,
        QueueEstimate, Side,
    },
};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{info, warn};

pub async fn run(mut rx: Receiver<OrderIntent>, fill_tx: Sender<FillEvent>, metrics: Arc<Metrics>) {
    while let Some(intent) = rx.recv().await {
        let side_label = match intent.request.side {
            Side::Buy => "buy",
            Side::Sell => "sell",
        };
        metrics.orders_total.with_label_values(&[side_label]).inc();

        let mut attempts = 0;
        loop {
            attempts += 1;
            match paper_submit(&intent).await {
                Ok(fill) => {
                    metrics.fills_total.with_label_values(&[side_label]).inc();
                    if fill_tx.send(fill).await.is_err() {
                        warn!("fill receiver dropped");
                    }
                    break;
                }
                Err(()) if attempts < 3 => {
                    tokio::time::sleep(Duration::from_millis(2 * attempts)).await;
                }
                Err(()) => {
                    warn!(?intent, "paper order failed after retries");
                    break;
                }
            }
        }
    }
}

async fn paper_submit(intent: &OrderIntent) -> Result<FillEvent, ()> {
    let reference = intent.request.price.unwrap_or(intent.last_price);
    let slip_bps = match intent.request.side {
        Side::Buy => 1.2,
        Side::Sell => -1.2,
    };
    let fill_price = reference * (1.0 + slip_bps / 10_000.0);
    let fee = fill_price * intent.request.size * 0.0004;
    info!(
        symbol = intent.request.symbol,
        ?intent.request.side,
        size = intent.request.size,
        price = fill_price,
        reason = ?intent.reason,
        score = intent.score,
        "paper order filled"
    );
    Ok(FillEvent {
        order_id: format!("ord-{}", now_ms()),
        fill_id: format!("fill-{}", now_ms()),
        symbol: intent.request.symbol.clone(),
        side: intent.request.side,
        size: intent.request.size,
        price: fill_price,
        requested_price: reference,
        filled_size: intent.request.size,
        remaining_size: 0.0,
        liquidity_flag: LiquidityFlag::Taker,
        fee,
        fee_asset: "USDT".to_string(),
        rebate_amount: 0.0,
        funding_amount: 0.0,
        timestamp: intent.timestamp,
        latency_us: 0,
        latency_breakdown: LatencyBreakdown {
            decision_latency_us: intent.data_latency_ms.saturating_mul(1_000),
            send_latency_us: 0,
            ack_latency_us: 0,
            first_fill_latency_us: 0,
            full_fill_latency_us: 0,
        },
        expected_markout: 0.0,
        expected_slippage_bps: intent.expected_slippage_bps,
        actual_slippage_bps: slip_bps.abs(),
        queue_estimate: QueueEstimate::default(),
        execution_mode: ExecutionMode::Aggressive,
        micro_exit: MicroExitSignal::default(),
        markout: MarkoutSnapshot::default(),
        complete: true,
        truth: ExecutionTruth {
            request_timestamp: intent.timestamp,
            send_timestamp: now_ms(),
            ack_timestamp: now_ms(),
            exchange_accept_timestamp: now_ms(),
            first_fill_timestamp: now_ms(),
            last_fill_timestamp: now_ms(),
            partial_fill_ratio: 1.0,
            cancel_reason: None,
            reject_reason: None,
            spread_at_execution: intent.regime.spread,
            queue_delay_us: 0,
            simulated: true,
        },
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}
