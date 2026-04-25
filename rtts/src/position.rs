use crate::{
    accounting::ledger::{AccountingEngine, FillLedgerEntry, LotMatchingMethod},
    config::Config,
    metrics::Metrics,
    types::{
        Decision, Direction, ExecutionMode, FillEvent, FlowSignal,
        OrderIntent, OrderRequest, OrderType, Position, QueueEstimate, RegimeKind, ScoredDecision,
        Side, TimingSignal,
    },
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
    let mut accounting = AccountingEngine::new(LotMatchingMethod::Fifo);
    let mut position = Position::default();
    let mut last_score = 0.0;
    let mut last_price: f64 = 0.0;

    loop {
        tokio::select! {
            biased;

            Some(fill) = fill_rx.recv() => {
                let ledger_entry = FillLedgerEntry::from(&fill);
                let _ = accounting.apply_fill(ledger_entry);
                let mark_price = last_price.max(fill.price);
                accounting.mark_to_market(&fill.symbol, mark_price);
                sync_position_from_accounting(&mut position, &accounting, &fill.symbol);
                metrics.position_size.with_label_values(&[&cfg.symbol]).set(position.size);
                if position.entries > 0 {
                    metrics.scale_efficiency.with_label_values(&[&cfg.symbol]).set(
                        position.unrealized_pnl / position.entries as f64,
                    );
                }
                metrics
                    .drawdown
                    .with_label_values(&[&cfg.symbol])
                    .set((-accounting.state().realized_pnl_total).max(0.0));
                info!(?position, price = fill.price, "paper fill applied");
            }
            Some(signal) = decision_rx.recv() => {
                let started = Instant::now();
                last_price = signal.market.price;
                accounting.mark_to_market(&cfg.symbol, signal.market.price);
                sync_position_from_accounting(&mut position, &accounting, &cfg.symbol);

                let drawdown_pct =
                    ((-accounting.state().realized_pnl_total).max(0.0) / cfg.capital).clamp(0.0, 1.0);
                if let Some(intent) = decide_order(&cfg, &position, &signal, last_score, drawdown_pct) {
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
    _drawdown_pct: f64,
) -> Option<OrderIntent> {
    if should_force_execution_exit(position, signal) {
        let side = position.side()?.opposite();
        return Some(intent(cfg, side, position.size.abs(), true, signal, position));
    }

    if signal.decision == Decision::Exit && position.is_open() {
        let side = position.side()?.opposite();
        let reduce_size = if signal.adversarial_risk > 0.80 {
            position.size.abs()
        } else {
            (position.size.abs() * 0.50).max(
                position
                    .size
                    .abs()
                    .min(quote_to_base(cfg.base_order_usd, signal.market.price)),
            )
        };
        return Some(intent(cfg, side, reduce_size, true, signal, position));
    }

    let side = match signal.direction {
        Direction::Long => Side::Buy,
        Direction::Short => Side::Sell,
        Direction::Flat => return None,
    };

    match signal.decision {
        Decision::EnterSmall if !position.is_open() && allow_micro_entry(signal) => {
            let size = quote_to_base(
                cfg.base_order_usd
                    * signal.dynamic_size_multiplier
                    * 0.35
                    * signal.confidence.max(0.35)
                    * context_size_factor(signal)
                    * flow_size_factor(signal),
                signal.market.price,
            );
            Some(intent(cfg, side, size, false, signal, position))
        }
        Decision::EnterSmall | Decision::ScaleIn
            if can_scale(position, signal, last_score, side, cfg.max_entries) =>
        {
            let regime_boost = if signal.context.regime == RegimeKind::TrendExpansion {
                0.35
            } else {
                0.0
            };
            let entry_multiplier =
                0.55 + position.entries as f64 * 0.22 + signal.confidence * 0.20 + regime_boost;
            let size = quote_to_base(
                cfg.base_order_usd
                    * entry_multiplier
                    * signal.dynamic_size_multiplier,
                signal.market.price,
            );
            Some(intent(cfg, side, size, false, signal, position))
        }
        Decision::Exit if position.is_open() => {
            let side = position.side()?.opposite();
            Some(intent(
                cfg,
                side,
                position.size.abs(),
                true,
                signal,
                position,
            ))
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
        && signal.confidence > 0.45
        && signal.flow.signal == FlowSignal::StrongContinuation
        && signal.timing.signal != TimingSignal::Missed
        && signal.features.order_flow_delta.signum()
            == if desired_side == Side::Buy { 1.0 } else { -1.0 }
        && signal.features.weighted_imbalance.signum()
            == if desired_side == Side::Buy { 1.0 } else { -1.0 }
        && position.unrealized_pnl >= -0.0001
        && signal.adversarial_risk < 0.55
        && signal.context.regime != RegimeKind::NewsShock
        && signal.context.liquidity_score > 0.35
}

fn intent(
    cfg: &Config,
    side: Side,
    size: f64,
    reduce_only: bool,
    signal: &ScoredDecision,
    position: &Position,
) -> OrderIntent {
    let order_type = if signal.urgency > 0.62 || reduce_only {
        OrderType::Market
    } else {
        OrderType::Limit
    };
    let execution_mode = match signal.competition_state {
        crate::types::CompetitionState::Normal => {
            if order_type == OrderType::Market {
                ExecutionMode::Aggressive
            } else {
                ExecutionMode::Passive
            }
        }
        crate::types::CompetitionState::Competitive => ExecutionMode::Passive,
        crate::types::CompetitionState::Saturated => ExecutionMode::Defensive,
    };
    OrderIntent {
        request: OrderRequest {
            symbol: cfg.symbol.clone(),
            side,
            size,
            price: Some(signal.market.price),
            order_type,
            post_only: order_type == OrderType::Limit && !reduce_only,
            reduce_only,
            max_slippage_bps: (signal.expected_slippage_bps * 1.8).max(2.0),
        },
        reason: signal.decision,
        score: signal.score,
        last_price: signal.market.price,
        position_before: position.clone(),
        timestamp: signal.market.timestamp,
        urgency: signal.urgency,
        expected_slippage_bps: signal.expected_slippage_bps,
        expected_duration_ms: signal.expected_duration_ms,
        data_latency_ms: signal.data_latency_ms,
        regime: signal.regime.clone(),
        context: signal.context.clone(),
        flow: signal.flow,
        timing: signal.timing,
        edge_state: signal.edge_state,
        edge_regime: signal.edge_regime,
        edge_reliability_score: signal.edge_reliability_score,
        edge_half_life_samples: signal.edge_half_life_samples,
        edge_capture_mean: signal.edge_capture_mean,
        negative_capture_streak: signal.negative_capture_streak,
        execution_alpha_mean: signal.execution_alpha_mean,
        markout_degradation_score: signal.markout_degradation_score,
        dynamic_size_multiplier: signal.dynamic_size_multiplier,
        competition_state: signal.competition_state,
        competition_score: signal.competition_score,
        trading_enabled: signal.trading_enabled,
        execution_mode,
        queue_estimate: QueueEstimate::default(),
        fill_probability: signal.fill_probability,
        meta: None,
    }
}

fn allow_micro_entry(signal: &ScoredDecision) -> bool {
    signal.trading_enabled
        && signal.edge_state != crate::accounting::edge_validation::EdgeState::Invalid
        &&
    matches!(
        signal.flow.signal,
        FlowSignal::StrongContinuation | FlowSignal::WeakContinuation
    ) && matches!(
        signal.timing.signal,
        TimingSignal::Optimal | TimingSignal::Neutral
    ) && signal.flow.continuation_strength > 0.42
        && signal.timing.timing_score > 0.45
}

fn should_force_execution_exit(position: &Position, signal: &ScoredDecision) -> bool {
    position.is_open()
        && (!signal.trading_enabled
            || signal.competition_state == crate::types::CompetitionState::Saturated
            || signal.negative_capture_streak >= 4
            || signal.edge_capture_mean < -0.05
            || signal.execution_alpha_mean < -1.0
            || signal.markout_degradation_score > 0.85)
}

fn flow_size_factor(signal: &ScoredDecision) -> f64 {
    match signal.flow.signal {
        FlowSignal::StrongContinuation => 1.0,
        FlowSignal::WeakContinuation => 0.65,
        FlowSignal::Exhaustion => 0.25,
        FlowSignal::ReversalRisk => 0.0,
    }
}

fn context_size_factor(signal: &ScoredDecision) -> f64 {
    match signal.context.regime {
        RegimeKind::LowLiquidity => 0.45,
        RegimeKind::HighVolatility => 0.70,
        RegimeKind::TrendExpansion => 1.20,
        RegimeKind::NewsShock => 0.0,
        RegimeKind::Normal => 1.0,
    }
}

#[inline]
fn quote_to_base(quote_size: f64, price: f64) -> f64 {
    quote_size / price.max(f64::EPSILON)
}


fn sync_position_from_accounting(
    position: &mut Position,
    accounting: &AccountingEngine,
    symbol: &str,
) {
    let exposure = accounting.position_exposure(symbol);
    if exposure.net_quantity.abs() <= f64::EPSILON {
        *position = Position::default();
        return;
    }
    if position.is_open()
        && position.size.signum() != exposure.net_quantity.signum()
    {
        warn!("accounting position crossed through flat; resetting local position snapshot");
    }
    position.size = exposure.net_quantity;
    position.avg_price = exposure.avg_entry_price;
    position.entries = exposure.open_lots as u32;
    position.confidence = (0.25 + exposure.open_lots as f64 * 0.10).clamp(0.25, 1.0);
    position.unrealized_pnl = exposure.unrealized_pnl;
}
