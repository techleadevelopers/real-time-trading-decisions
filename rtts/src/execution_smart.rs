use crate::{
    config::Config,
    metrics::Metrics,
    types::{FillEvent, FlowSignal, LearningSample, OrderIntent, OrderType, Side, TimingSignal},
};
use std::{sync::Arc, time::Instant};
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{info, warn};

pub async fn run(
    cfg: Config,
    mut rx: Receiver<OrderIntent>,
    fill_tx: Sender<FillEvent>,
    learning_tx: Sender<LearningSample>,
    metrics: Arc<Metrics>,
) {
    while let Some(mut intent) = rx.recv().await {
        let started = Instant::now();
        if intent.data_latency_ms > cfg.max_data_age_ms {
            metrics
                .rejected_orders_total
                .with_label_values(&["stale_before_execute"])
                .inc();
            continue;
        }
        if intent.meta.as_ref().is_some_and(|meta| {
            meta.decision != crate::types::FinalDecision::Execute || meta.adjusted_ev <= 0.0
        }) {
            metrics
                .rejected_orders_total
                .with_label_values(&["meta_not_execute"])
                .inc();
            continue;
        }

        choose_order_type(&mut intent);
        let side_label = match intent.request.side {
            Side::Buy => "buy",
            Side::Sell => "sell",
        };
        metrics.orders_total.with_label_values(&[side_label]).inc();

        let mut remaining = intent.request.size;
        let mut attempts = 0;
        while remaining > intent.request.size * 0.001 && attempts < 3 {
            attempts += 1;
            let child_size = remaining;
            match paper_submit(&intent, child_size, started).await {
                Ok(fill) => {
                    remaining = fill.remaining_size;
                    metrics.fills_total.with_label_values(&[side_label]).inc();
                    metrics
                        .execution_latency_us
                        .with_label_values(&[order_type_label(intent.request.order_type)])
                        .observe(fill.latency_us as f64);
                    metrics
                        .slippage_bps
                        .with_label_values(&[side_label])
                        .observe(fill.actual_slippage_bps);
                    let sample = learning_sample(&intent, &fill);
                    metrics
                        .microtrade_pnl
                        .with_label_values(&[&intent.request.symbol])
                        .observe(sample.pnl);
                    let _ = learning_tx.try_send(sample);
                    if fill_tx.send(fill).await.is_err() {
                        warn!("fill receiver dropped");
                        break;
                    }
                    if remaining <= f64::EPSILON {
                        break;
                    }
                    cancel_replace(&mut intent, remaining);
                }
                Err(()) => {
                    metrics
                        .rejected_orders_total
                        .with_label_values(&["execution_retry"])
                        .inc();
                    cancel_replace(&mut intent, remaining);
                }
            }
        }
    }
}

fn choose_order_type(intent: &mut OrderIntent) {
    if intent.flow.signal == FlowSignal::ReversalRisk
        || intent.timing.signal == TimingSignal::Missed
    {
        intent.request.order_type = OrderType::Market;
        intent.request.post_only = false;
        intent.request.reduce_only = true;
        return;
    }
    if intent.urgency > 0.64
        || intent.expected_duration_ms < 180
        || intent.request.reduce_only
        || intent.flow.signal == FlowSignal::StrongContinuation && intent.timing.timing_score > 0.72
    {
        intent.request.order_type = OrderType::Market;
        intent.request.post_only = false;
        return;
    }
    intent.request.order_type = OrderType::Limit;
    intent.request.post_only = true;
    let reference = intent.request.price.unwrap_or(intent.last_price);
    let inside = match intent.request.side {
        Side::Buy => reference * (1.0 - 0.4 / 10_000.0),
        Side::Sell => reference * (1.0 + 0.4 / 10_000.0),
    };
    intent.request.price = Some(inside);
}

async fn paper_submit(
    intent: &OrderIntent,
    child_size: f64,
    started: Instant,
) -> Result<FillEvent, ()> {
    let reference = intent.request.price.unwrap_or(intent.last_price);
    let adverse_selection = adverse_selection_score(intent);
    if adverse_selection > 0.78 && !intent.request.reduce_only {
        return Err(());
    }
    let market_pressure = if intent.request.order_type == OrderType::Market {
        intent.urgency * 2.5
    } else {
        -0.35 - intent.timing.spread_compression * 0.30
    };
    let side_sign = match intent.request.side {
        Side::Buy => 1.0,
        Side::Sell => -1.0,
    };
    let actual_slippage_bps = (intent.expected_slippage_bps + market_pressure)
        .clamp(-1.0, intent.request.max_slippage_bps);
    if actual_slippage_bps > intent.request.max_slippage_bps {
        return Err(());
    }
    let fill_ratio = match intent.request.order_type {
        OrderType::Market => 1.0,
        OrderType::Limit if intent.urgency > 0.40 => 0.55 + intent.timing.timing_score * 0.20,
        OrderType::Limit => 0.25 + intent.timing.timing_score * 0.25,
    };
    let filled_size = child_size * fill_ratio;
    let remaining_size = (child_size - filled_size).max(0.0);
    let fill_price = reference * (1.0 + side_sign * actual_slippage_bps / 10_000.0);
    let fee = fill_price
        * filled_size
        * if intent.request.order_type == OrderType::Market {
            0.0004
        } else {
            0.0002
        };
    let latency_us = started.elapsed().as_micros() as u64;
    info!(
        symbol = intent.request.symbol,
        ?intent.request.side,
        ?intent.request.order_type,
        filled_size,
        remaining_size,
        fill_price,
        actual_slippage_bps,
        "smart paper fill"
    );
    Ok(FillEvent {
        symbol: intent.request.symbol.clone(),
        side: intent.request.side,
        size: intent.request.size,
        price: fill_price,
        requested_price: reference,
        filled_size,
        remaining_size,
        fee,
        timestamp: intent.timestamp,
        latency_us,
        expected_slippage_bps: intent.expected_slippage_bps,
        actual_slippage_bps,
        complete: remaining_size <= intent.request.size * 0.001,
    })
}

fn cancel_replace(intent: &mut OrderIntent, remaining: f64) {
    intent.request.size = remaining;
    intent.request.post_only = false;
    intent.request.order_type = if intent.urgency > 0.45
        || intent.flow.signal == FlowSignal::Exhaustion
        || intent.timing.signal == TimingSignal::Missed
    {
        OrderType::Market
    } else {
        OrderType::Limit
    };
    if let Some(price) = intent.request.price {
        let adjustment = match intent.request.side {
            Side::Buy => 0.7 / 10_000.0,
            Side::Sell => -0.7 / 10_000.0,
        };
        intent.request.price = Some(price * (1.0 + adjustment));
    }
}

fn learning_sample(intent: &OrderIntent, fill: &FillEvent) -> LearningSample {
    let side_sign = match intent.request.side {
        Side::Buy => 1.0,
        Side::Sell => -1.0,
    };
    let markout = (intent.last_price - fill.price) * fill.filled_size * side_sign;
    LearningSample {
        timestamp: fill.timestamp,
        direction: if intent.request.side == Side::Buy {
            crate::types::Direction::Long
        } else {
            crate::types::Direction::Short
        },
        confidence: intent.score,
        predicted_score: intent.score,
        expected_slippage_bps: fill.expected_slippage_bps,
        actual_slippage_bps: fill.actual_slippage_bps,
        pnl: markout - fill.fee,
        duration_ms: (fill.latency_us / 1_000).max(1),
        entry_quality: intent
            .meta
            .as_ref()
            .map(|meta| meta.entry_quality)
            .unwrap_or(intent.timing.timing_score),
        regime: intent.regime.clone(),
    }
}

fn adverse_selection_score(intent: &OrderIntent) -> f64 {
    let flow_risk = match intent.flow.signal {
        FlowSignal::ReversalRisk => 1.0,
        FlowSignal::Exhaustion => 0.72,
        FlowSignal::WeakContinuation => 0.35,
        FlowSignal::StrongContinuation => 0.0,
    };
    let timing_risk = match intent.timing.signal {
        TimingSignal::Missed => 1.0,
        TimingSignal::Wait => 0.55,
        TimingSignal::Neutral => 0.25,
        TimingSignal::Optimal => 0.0,
    };
    (0.45 * flow_risk
        + 0.30 * timing_risk
        + 0.15 * intent.expected_slippage_bps / intent.request.max_slippage_bps.max(1.0)
        + 0.10 * (1.0 - intent.context.stability_score))
        .clamp(0.0, 1.0)
}

fn order_type_label(order_type: OrderType) -> &'static str {
    match order_type {
        OrderType::Market => "market",
        OrderType::Limit => "limit",
    }
}
