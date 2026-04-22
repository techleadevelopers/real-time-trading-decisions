use crate::{
    metrics::Metrics,
    types::{FillEvent, OrderIntent, Side},
};
use std::{sync::Arc, time::Duration};
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
        symbol: intent.request.symbol.clone(),
        side: intent.request.side,
        size: intent.request.size,
        price: fill_price,
        requested_price: reference,
        filled_size: intent.request.size,
        remaining_size: 0.0,
        fee,
        timestamp: intent.timestamp,
        latency_us: 0,
        expected_slippage_bps: intent.expected_slippage_bps,
        actual_slippage_bps: slip_bps.abs(),
        complete: true,
    })
}
