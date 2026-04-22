use crate::{
    adverse_selection::AdverseSelectionDetector,
    config::Config,
    execution_mode::ExecutionModeSwitch,
    fill_probability::FillProbabilityModel,
    markout::MarkoutAnalysisEngine,
    metrics::Metrics,
    micro_exit::MicroExitEngine,
    queue_position::QueuePositionEngine,
    symbol_profile::SymbolProfileEngine,
    types::{
        ExecutionMode, FillEvent, FlowSignal, LearningSample, MarkoutSnapshot, MicroExitSignal,
        OrderIntent, OrderType, QueueEstimate, Side, TimingSignal,
    },
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
    let mut symbol_profile = SymbolProfileEngine::new(cfg.symbol.clone());

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

        symbol_profile.observe_intent(&intent);
        prepare_execution(&mut intent, symbol_profile.profile());

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
                    symbol_profile.observe_fill(&fill);
                    metrics.fills_total.with_label_values(&[side_label]).inc();
                    metrics
                        .execution_latency_us
                        .with_label_values(&[execution_mode_label(intent.execution_mode)])
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

                    let exit_fill = immediate_exit_fill(&intent, &fill);
                    if fill_tx.send(fill).await.is_err() {
                        warn!("fill receiver dropped");
                        break;
                    }
                    if let Some(exit_fill) = exit_fill {
                        let exit_sample = learning_sample(&intent, &exit_fill);
                        let _ = learning_tx.try_send(exit_sample);
                        if fill_tx.send(exit_fill).await.is_err() {
                            warn!("exit fill receiver dropped");
                        }
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

fn prepare_execution(intent: &mut OrderIntent, profile: &crate::types::SymbolProfile) {
    let queue = QueuePositionEngine::estimate(intent, profile);
    let fill_class = FillProbabilityModel::classify(intent, &queue);
    let mode = ExecutionModeSwitch::choose(intent, fill_class);

    intent.execution_mode = mode;
    intent.fill_probability = fill_class;
    intent.request.order_type = match mode {
        ExecutionMode::Aggressive | ExecutionMode::Defensive => OrderType::Market,
        ExecutionMode::Passive => OrderType::Limit,
    };
    intent.request.post_only = mode == ExecutionMode::Passive;
    if mode == ExecutionMode::Defensive {
        intent.request.reduce_only = true;
    }
    intent.queue_estimate = QueuePositionEngine::estimate(intent, profile);

    if intent.request.order_type == OrderType::Limit {
        let reference = intent.request.price.unwrap_or(intent.last_price);
        let depth = intent.queue_estimate.placement_depth_bps / 10_000.0;
        let inside = match intent.request.side {
            Side::Buy => reference * (1.0 - depth),
            Side::Sell => reference * (1.0 + depth),
        };
        intent.request.price = Some(inside);
    }
}

async fn paper_submit(
    intent: &OrderIntent,
    child_size: f64,
    started: Instant,
) -> Result<FillEvent, ()> {
    let reference = intent.request.price.unwrap_or(intent.last_price);
    let adverse_selection = AdverseSelectionDetector::pre_fill_score(intent);
    if adverse_selection > 0.78 && !intent.request.reduce_only {
        return Err(());
    }

    let market_pressure = match intent.execution_mode {
        ExecutionMode::Aggressive => intent.urgency * 2.5,
        ExecutionMode::Defensive => intent.urgency * 1.6,
        ExecutionMode::Passive => {
            -0.35 - intent.timing.spread_compression * 0.30
                + (1.0 - intent.queue_estimate.fill_probability) * 0.25
        }
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

    let fill_ratio = match intent.execution_mode {
        ExecutionMode::Aggressive | ExecutionMode::Defensive => 1.0,
        ExecutionMode::Passive if intent.urgency > 0.40 => {
            (0.35 + intent.queue_estimate.fill_probability * 0.45).clamp(0.10, 0.90)
        }
        ExecutionMode::Passive => {
            (0.18 + intent.queue_estimate.fill_probability * 0.38).clamp(0.05, 0.72)
        }
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
    let markout = MarkoutAnalysisEngine::estimate(intent, fill_price, filled_size);
    let mut fill = FillEvent {
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
        queue_estimate: intent.queue_estimate,
        execution_mode: intent.execution_mode,
        micro_exit: MicroExitSignal::default(),
        markout,
        complete: remaining_size <= intent.request.size * 0.001,
    };
    let adverse_post = AdverseSelectionDetector::post_fill_score(intent, &fill);
    fill.micro_exit = MicroExitEngine::evaluate(intent, fill.price, adverse_post);

    info!(
        symbol = intent.request.symbol,
        ?intent.request.side,
        ?intent.execution_mode,
        filled_size,
        remaining_size,
        fill_price,
        actual_slippage_bps,
        queue_position = intent.queue_estimate.queue_position,
        fill_probability = intent.queue_estimate.fill_probability,
        "execution alpha paper fill"
    );
    Ok(fill)
}

fn cancel_replace(intent: &mut OrderIntent, remaining: f64) {
    intent.request.size = remaining;
    intent.request.post_only = false;
    intent.execution_mode = if intent.urgency > 0.45
        || intent.flow.signal == FlowSignal::Exhaustion
        || intent.timing.signal == TimingSignal::Missed
    {
        ExecutionMode::Aggressive
    } else {
        ExecutionMode::Passive
    };
    intent.request.order_type = if intent.execution_mode == ExecutionMode::Aggressive {
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
        markout_100ms: fill.markout.pnl_100ms,
        markout_500ms: fill.markout.pnl_500ms,
        markout_1s: fill.markout.pnl_1s,
        regime: intent.regime.clone(),
    }
}

fn immediate_exit_fill(intent: &OrderIntent, fill: &FillEvent) -> Option<FillEvent> {
    if fill.micro_exit.reduce_ratio <= 0.0 {
        return None;
    }
    let exit_size = fill.filled_size * fill.micro_exit.reduce_ratio.clamp(0.0, 1.0);
    if exit_size <= f64::EPSILON {
        return None;
    }
    let exit_side = fill.side.opposite();
    let side_sign = match exit_side {
        Side::Buy => 1.0,
        Side::Sell => -1.0,
    };
    let slip = (intent.expected_slippage_bps * (1.0 + fill.micro_exit.urgency)).max(0.5);
    let price = fill.price * (1.0 + side_sign * slip / 10_000.0);
    Some(FillEvent {
        symbol: fill.symbol.clone(),
        side: exit_side,
        size: exit_size,
        price,
        requested_price: fill.price,
        filled_size: exit_size,
        remaining_size: 0.0,
        fee: price * exit_size * 0.0004,
        timestamp: fill.timestamp,
        latency_us: fill.latency_us.saturating_add(250),
        expected_slippage_bps: intent.expected_slippage_bps,
        actual_slippage_bps: slip,
        queue_estimate: QueueEstimate::default(),
        execution_mode: ExecutionMode::Defensive,
        micro_exit: fill.micro_exit,
        markout: MarkoutSnapshot::default(),
        complete: true,
    })
}

fn execution_mode_label(mode: ExecutionMode) -> &'static str {
    match mode {
        ExecutionMode::Aggressive => "aggressive",
        ExecutionMode::Passive => "passive",
        ExecutionMode::Defensive => "defensive",
    }
}
