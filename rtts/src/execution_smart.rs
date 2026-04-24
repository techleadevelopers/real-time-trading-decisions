use crate::{
    accounting::{
        interfaces::ExecutionSummary,
        latency::{latency_impact_score, LatencyBreakdown, LatencyDistributions},
        ledger::{FillLedgerEntry, LiquidityFlag},
    },
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
        ExecutionMode, ExecutionTruth, FillEvent, FlowSignal, MarkoutSnapshot, MicroExitSignal,
        OrderIntent, OrderType, QueueEstimate, Side, TimingSignal,
    },
};
use std::{
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{info, warn};

pub async fn run(
    cfg: Config,
    mut rx: Receiver<OrderIntent>,
    fill_tx: Sender<FillEvent>,
    truth_fill_tx: Sender<FillEvent>,
    metrics: Arc<Metrics>,
) {
    let mut symbol_profile = SymbolProfileEngine::new(cfg.symbol.clone());
    let mut latency_distributions = LatencyDistributions::default();

    while let Some(mut intent) = rx.recv().await {
        let started = Instant::now();
        if intent.data_latency_ms > cfg.max_data_age_ms {
            metrics
                .rejected_orders_total
                .with_label_values(&["stale_before_execute"])
                .inc();
            continue;
        }
        if intent
            .meta
            .as_ref()
            .is_some_and(|meta| meta.decision != crate::types::FinalDecision::Execute)
        {
            metrics
                .rejected_orders_total
                .with_label_values(&["meta_not_execute"])
                .inc();
            continue;
        }
        if expected_real_markout_after_cost(&intent, &latency_distributions) <= execution_threshold(&cfg) {
            metrics
                .rejected_orders_total
                .with_label_values(&["weak_expected_real_markout"])
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
            match paper_submit(&intent, child_size, started, &latency_distributions).await {
                Ok(fill) => {
                    remaining = fill.remaining_size;
                    symbol_profile.observe_fill(&fill);
                    latency_distributions.record(fill.latency_breakdown);
                    metrics.fills_total.with_label_values(&[side_label]).inc();
                    metrics
                        .execution_latency_us
                        .with_label_values(&[execution_mode_label(intent.execution_mode)])
                        .observe(fill.latency_breakdown.full_fill_latency_us as f64);
                    metrics
                        .slippage_bps
                        .with_label_values(&[side_label])
                        .observe(fill.actual_slippage_bps);
                    let _summary = execution_summary(&fill);

                    let exit_fill = immediate_exit_fill(&intent, &fill);
                    let _ = truth_fill_tx.try_send(fill.clone());
                    if fill_tx.send(fill).await.is_err() {
                        warn!("fill receiver dropped");
                        break;
                    }
                    if let Some(exit_fill) = exit_fill {
                        let _ = truth_fill_tx.try_send(exit_fill.clone());
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
    latency_distributions: &LatencyDistributions,
) -> Result<FillEvent, ()> {
    let send_timestamp = now_ms();
    let reference = intent.request.price.unwrap_or(intent.last_price);
    let order_id = new_event_id("ord");
    let fill_id = new_event_id("fill");
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
    let fill_timestamp = now_ms();
    let latency_breakdown = LatencyBreakdown {
        decision_latency_us: intent.data_latency_ms.saturating_mul(1_000),
        send_latency_us: send_timestamp.saturating_sub(intent.timestamp).saturating_mul(1_000),
        ack_latency_us: 150,
        first_fill_latency_us: latency_us,
        full_fill_latency_us: latency_us,
    };
    let liquidity_flag = if intent.request.order_type == OrderType::Limit {
        LiquidityFlag::Maker
    } else {
        LiquidityFlag::Taker
    };
    let rebate_amount = if liquidity_flag == LiquidityFlag::Maker {
        fill_price * filled_size * 0.00005
    } else {
        0.0
    };
    let funding_amount = 0.0;
    let truth = ExecutionTruth {
        request_timestamp: intent.timestamp,
        send_timestamp,
        ack_timestamp: send_timestamp,
        exchange_accept_timestamp: send_timestamp,
        first_fill_timestamp: fill_timestamp,
        last_fill_timestamp: fill_timestamp,
        partial_fill_ratio: if intent.request.size > 0.0 {
            (filled_size / intent.request.size).clamp(0.0, 1.0)
        } else {
            0.0
        },
        cancel_reason: None,
        reject_reason: None,
        spread_at_execution: intent.regime.spread,
        queue_delay_us: latency_us,
        simulated: true,
    };
    let mut fill = FillEvent {
        order_id,
        fill_id,
        symbol: intent.request.symbol.clone(),
        side: intent.request.side,
        size: intent.request.size,
        price: fill_price,
        requested_price: reference,
        filled_size,
        remaining_size,
        liquidity_flag,
        fee,
        fee_asset: "USDT".to_string(),
        rebate_amount,
        funding_amount,
        timestamp: intent.timestamp,
        latency_us,
        latency_breakdown,
        expected_markout: expected_real_markout_after_cost(intent, latency_distributions),
        expected_slippage_bps: intent.expected_slippage_bps,
        actual_slippage_bps,
        queue_estimate: intent.queue_estimate,
        execution_mode: intent.execution_mode,
        micro_exit: MicroExitSignal::default(),
        markout,
        complete: remaining_size <= intent.request.size * 0.001,
        truth,
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
        order_id: fill.order_id.clone(),
        fill_id: new_event_id("fill"),
        symbol: fill.symbol.clone(),
        side: exit_side,
        size: exit_size,
        price,
        requested_price: fill.price,
        filled_size: exit_size,
        remaining_size: 0.0,
        liquidity_flag: LiquidityFlag::Taker,
        fee: price * exit_size * 0.0004,
        fee_asset: "USDT".to_string(),
        rebate_amount: 0.0,
        funding_amount: 0.0,
        timestamp: fill.timestamp,
        latency_us: fill.latency_us.saturating_add(250),
        latency_breakdown: LatencyBreakdown {
            decision_latency_us: fill.latency_breakdown.decision_latency_us,
            send_latency_us: 100,
            ack_latency_us: 150,
            first_fill_latency_us: fill.latency_breakdown.first_fill_latency_us.saturating_add(250),
            full_fill_latency_us: fill.latency_breakdown.full_fill_latency_us.saturating_add(250),
        },
        expected_markout: 0.0,
        expected_slippage_bps: intent.expected_slippage_bps,
        actual_slippage_bps: slip,
        queue_estimate: QueueEstimate::default(),
        execution_mode: ExecutionMode::Defensive,
        micro_exit: fill.micro_exit,
        markout: MarkoutSnapshot::default(),
        complete: true,
        truth: ExecutionTruth {
            request_timestamp: fill.truth.request_timestamp,
            send_timestamp: now_ms(),
            ack_timestamp: now_ms(),
            exchange_accept_timestamp: now_ms(),
            first_fill_timestamp: now_ms(),
            last_fill_timestamp: now_ms(),
            partial_fill_ratio: 1.0,
            cancel_reason: None,
            reject_reason: None,
            spread_at_execution: intent.regime.spread,
            queue_delay_us: fill.latency_us.saturating_add(250),
            simulated: true,
        },
    })
}

fn expected_real_markout_after_cost(intent: &OrderIntent, latency_distributions: &LatencyDistributions) -> f64 {
    let expected_bps = (intent.flow.continuation_strength * 5.0 + intent.timing.timing_score * 3.0
        - intent.expected_slippage_bps
        - intent.regime.spread.max(0.0) * 0.25)
        .max(-10.0);
    let notional = intent.request.size * intent.last_price;
    let snapshot = latency_distributions.snapshot();
    let fill_quality = intent.queue_estimate.fill_probability.clamp(0.0, 1.0);
    let latency_penalty = latency_impact_score(&snapshot, intent.expected_slippage_bps, fill_quality);
    notional * expected_bps / 10_000.0 - notional * (0.0004 + latency_penalty * 0.00025)
}

fn execution_threshold(cfg: &Config) -> f64 {
    (cfg.base_order_usd * 0.00005).max(0.001)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn execution_mode_label(mode: ExecutionMode) -> &'static str {
    match mode {
        ExecutionMode::Aggressive => "aggressive",
        ExecutionMode::Passive => "passive",
        ExecutionMode::Defensive => "defensive",
    }
}

fn execution_summary(fill: &FillEvent) -> ExecutionSummary {
    let ledger_fill = FillLedgerEntry::from(fill);
    ExecutionSummary {
        fill_ratio: if fill.size > 0.0 {
            (fill.filled_size / fill.size).clamp(0.0, 1.0)
        } else {
            0.0
        },
        slippage: fill.actual_slippage_bps,
        latency: fill.latency_breakdown,
        fills: vec![ledger_fill],
    }
}

fn new_event_id(prefix: &str) -> String {
    format!("{prefix}-{}", now_ms())
}
