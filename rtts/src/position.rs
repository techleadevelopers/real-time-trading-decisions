use crate::{
    config::Config,
    metrics::Metrics,
    types::{Decision, Event, FillEvent, OrderIntent, OrderRequest, Position, ScoredDecision, Side},
};
use std::{sync::Arc, time::Instant};
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{info, warn};

pub async fn run(
    cfg: Config,
    mut decision_rx: Receiver<ScoredDecision>,
    mut fill_rx: Receiver<FillEvent>,
    order_tx: Sender<OrderIntent>,
    metrics: Arc<Metrics>,
) {
    let mut position = Position::default();
    let mut last_score = 0.0;
    let mut last_price = 0.0;

    loop {
        tokio::select! {
            biased;

            Some(fill) = fill_rx.recv() => {
                apply_fill(&mut position, &fill);
                position.update_unrealized(last_price.max(fill.price));
                metrics.position_size.with_label_values(&[&cfg.symbol]).set(position.size);
                info!(?position, price = fill.price, "paper fill applied");
            }
            Some(signal) = decision_rx.recv() => {
                let started = Instant::now();
                last_price = signal.market.price;
                position.update_unrealized(signal.market.price);

                if let Some(intent) = decide_order(&cfg, &position, &signal, last_score) {
                    if order_tx.try_send(intent.clone()).is_err() {
                        metrics.channel_backpressure_total.with_label_values(&["position"]).inc();
                        if order_tx.send(intent).await.is_err() {
                            break;
                        }
                    }
                }

                last_score = signal.score;
                metrics
                    .stage_latency_us
                    .with_label_values(&["position"])
                    .observe(started.elapsed().as_micros() as f64);
            }
            else => break,
        }
    }
}

fn decide_order(
    cfg: &Config,
    position: &Position,
    signal: &ScoredDecision,
    last_score: f64,
) -> Option<OrderIntent> {
    if signal.decision == Decision::Exit && position.is_open() {
        let side = position.side()?.opposite();
        return Some(intent(cfg, side, position.size.abs(), signal, position));
    }

    let side = match signal.event {
        Event::PumpDetected => Side::Buy,
        Event::DumpDetected => Side::Sell,
        Event::Neutral => return None,
    };

    match signal.decision {
        Decision::EnterSmall if !position.is_open() => {
            let size = quote_to_base(cfg.base_order_usd, signal.market.price);
            Some(intent(cfg, side, size, signal, position))
        }
        Decision::EnterSmall | Decision::ScaleIn if can_scale(position, signal, last_score, side, cfg.max_entries) => {
            let entry_multiplier = 1.0 + position.entries as f64 * 0.35;
            let size = quote_to_base(cfg.base_order_usd * entry_multiplier, signal.market.price);
            Some(intent(cfg, side, size, signal, position))
        }
        Decision::Exit if position.is_open() => {
            let side = position.side()?.opposite();
            Some(intent(cfg, side, position.size.abs(), signal, position))
        }
        _ => None,
    }
}

fn can_scale(
    position: &Position,
    signal: &ScoredDecision,
    last_score: f64,
    desired_side: Side,
    max_entries: u32,
) -> bool {
    position.is_open()
        && position.side() == Some(desired_side)
        && position.entries < max_entries
        && signal.score > last_score
        && position.unrealized_pnl >= -0.0001
}

fn intent(
    cfg: &Config,
    side: Side,
    size: f64,
    signal: &ScoredDecision,
    position: &Position,
) -> OrderIntent {
    OrderIntent {
        request: OrderRequest {
            symbol: cfg.symbol.clone(),
            side,
            size,
            price: Some(signal.market.price),
        },
        reason: signal.decision,
        score: signal.score,
        last_price: signal.market.price,
        position_before: position.clone(),
        timestamp: signal.market.timestamp,
    }
}

#[inline]
fn quote_to_base(quote_size: f64, price: f64) -> f64 {
    quote_size / price.max(f64::EPSILON)
}

fn apply_fill(position: &mut Position, fill: &FillEvent) {
    let signed_size = match fill.side {
        Side::Buy => fill.size,
        Side::Sell => -fill.size,
    };
    if !position.is_open() || position.size.signum() == signed_size.signum() {
        let new_size = position.size + signed_size;
        let old_notional = position.avg_price * position.size.abs();
        let fill_notional = fill.price * fill.size;
        let denom = new_size.abs().max(f64::EPSILON);
        position.avg_price = (old_notional + fill_notional) / denom;
        position.size = new_size;
        position.entries = position.entries.saturating_add(1);
        position.confidence = (position.confidence + 0.25).min(1.0);
    } else {
        let remaining = position.size + signed_size;
        if remaining.abs() <= f64::EPSILON {
            *position = Position::default();
        } else if remaining.signum() == position.size.signum() {
            position.size = remaining;
        } else {
            warn!("fill crossed through flat; opening residual position");
            position.size = remaining;
            position.avg_price = fill.price;
            position.entries = 1;
            position.confidence = 0.25;
        }
    }
}

