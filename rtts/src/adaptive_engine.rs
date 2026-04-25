use crate::{
    accounting::edge_validation::{EdgeState, EdgeValidationEngine, EdgeValidationSnapshot},
    config::Config,
    execution_controller::ExecutionControlFeedback,
    learning::LearningState,
    metrics::Metrics,
    types::{
        Decision, Direction, Event, FlowSignal, LearningSample, MarketEvent, MicrostructureFrame,
        ScoredDecision, Side, TimingSignal,
    },
};
use std::{sync::Arc, time::Instant};
use tokio::sync::mpsc::{Receiver, Sender};

pub async fn run(
    cfg: Config,
    mut rx: Receiver<MicrostructureFrame>,
    mut learning_rx: Receiver<LearningSample>,
    mut control_feedback_rx: Receiver<ExecutionControlFeedback>,
    tx: Sender<ScoredDecision>,
    metrics: Arc<Metrics>,
) {
    let mut learning = LearningState::default();
    let mut edge_validation = EdgeValidationEngine::default();
    let mut previous_score = 0.5;
    let mut trading_disabled = false;
    let mut drawdown_pct = 0.0;
    let mut cumulative_realized_pnl = 0.0;

    loop {
        tokio::select! {
            biased;

            Some(sample) = learning_rx.recv() => {
                let regime_label = regime_label(&sample.regime);
                let sample_pnl = sample.pnl;
                let projected_realized_pnl = cumulative_realized_pnl + sample_pnl;
                let projected_drawdown_pct =
                    ((-projected_realized_pnl).max(0.0) / cfg.capital).clamp(0.0, 1.0);
                let snapshot = edge_validation.observe(&sample, projected_drawdown_pct);
                learning.apply_sample(sample);
                cumulative_realized_pnl = projected_realized_pnl;
                drawdown_pct = projected_drawdown_pct;
                metrics.hit_rate.with_label_values(&[regime_label]).set(learning.hit_rate());
                if learning.consecutive_losses() >= cfg.max_consecutive_losses
                    || snapshot.edge_state == EdgeState::Invalid
                    || !snapshot.trading_enabled
                {
                    trading_disabled = true;
                    metrics.rejected_orders_total.with_label_values(&["adaptive_loss_lockout"]).inc();
                } else if snapshot.edge_state == EdgeState::Valid {
                    trading_disabled = false;
                }
            }
            Some(feedback) = control_feedback_rx.recv() => {
                edge_validation.observe_failure(feedback.reason);
                metrics
                    .rejected_orders_total
                    .with_label_values(&["execution_controller_failure"])
                    .inc();
                if feedback.aborted_due_to_decay {
                    trading_disabled = true;
                }
            }
            Some(frame) = rx.recv() => {
                let started = Instant::now();
                let edge_snapshot = edge_validation.snapshot(drawdown_pct);
                let decision = score_frame(&cfg, &learning, &frame, previous_score, trading_disabled, edge_snapshot);
                previous_score = decision.score;
                metrics.decisions_total.with_label_values(&[decision_label(decision.decision)]).inc();
                metrics.stage_latency_us.with_label_values(&["adaptive_decision"]).observe(started.elapsed().as_micros() as f64);

                if tx.try_send(decision.clone()).is_err() {
                    metrics.channel_backpressure_total.with_label_values(&["adaptive_decision"]).inc();
                    if tx.send(decision).await.is_err() {
                        break;
                    }
                }
            }
            else => break,
        }
    }
}

fn score_frame(
    cfg: &Config,
    learning: &LearningState,
    frame: &MicrostructureFrame,
    previous_score: f64,
    trading_disabled: bool,
    edge_snapshot: EdgeValidationSnapshot,
) -> ScoredDecision {
    let weights = learning.weights(&frame.regime);
    let direction = infer_direction(frame);
    let side_factor = match direction {
        Direction::Long => 1.0,
        Direction::Short => -1.0,
        Direction::Flat => 0.0,
    };
    let adversarial_risk = (frame.features.spoofing_risk * 0.45
        + frame.features.liquidity_pull * 0.35
        + frame.features.absorption * 0.20)
        .clamp(0.0, 1.0);
    let liquidity_support = (frame.book.weighted_imbalance * side_factor
        - frame.features.liquidity_shift.min(0.0).abs() * 0.35)
        .clamp(-1.0, 1.0);

    let raw = weights.velocity * frame.features.micro_price_velocity * side_factor
        + weights.orderflow * frame.features.order_flow_delta * side_factor
        + weights.imbalance * frame.features.weighted_imbalance * side_factor
        + weights.liquidity * liquidity_support
        + 0.35 * frame.tape.continuation
        + 0.42 * frame.flow.continuation_strength
        + 0.28 * frame.timing.timing_score
        - weights.spread * (frame.regime.spread / 10.0)
        - weights.adversarial * adversarial_risk
        - flow_penalty(frame.flow.signal)
        - timing_penalty(frame.timing.signal);
    let score = sigmoid(raw).clamp(0.0, 1.0);
    let edge_degrade = match edge_snapshot.edge_state {
        EdgeState::Valid => 1.0,
        EdgeState::Uncertain => 0.65,
        EdgeState::Invalid => 0.0,
    } * match edge_snapshot.competition_state {
        crate::types::CompetitionState::Normal => 1.0,
        crate::types::CompetitionState::Competitive => 0.75,
        crate::types::CompetitionState::Saturated => 0.0,
    };
    let confidence = ((score - 0.5).abs() * 2.0 * (1.0 - adversarial_risk) * edge_degrade)
        .clamp(0.0, 1.0);
    let threshold = ((learning.threshold(&frame.regime)
        + match edge_snapshot.edge_state {
            EdgeState::Valid => 0.0,
            EdgeState::Uncertain => 0.05,
            EdgeState::Invalid => 0.18,
        })
        + match edge_snapshot.competition_state {
            crate::types::CompetitionState::Normal => 0.0,
            crate::types::CompetitionState::Competitive => 0.05,
            crate::types::CompetitionState::Saturated => 0.18,
        })
        .clamp(0.62, 0.94);
    let data_latency_ms = data_age_ms(frame.timestamp);
    let stale_or_slow = frame.stale || data_latency_ms > cfg.max_data_age_ms;
    let event = event_from_direction(direction, score, threshold);
    let expected_duration_ms =
        expected_duration(&frame.regime, confidence, frame.tape.trade_frequency);
    let expected_slippage_bps = estimate_slippage(frame, direction);
    let urgency = urgency(
        score,
        threshold,
        expected_duration_ms,
        frame.tape.volume_burst,
        adversarial_risk,
    ) * edge_degrade
        * (1.0 - edge_snapshot.competition_score * 0.35);

    let decision = if trading_disabled
        || edge_snapshot.edge_state == EdgeState::Invalid
        || edge_snapshot.competition_state == crate::types::CompetitionState::Saturated
    {
        Decision::Ignore
    } else if stale_or_slow {
        Decision::Ignore
    } else if frame.timing.signal == TimingSignal::Missed {
        Decision::Ignore
    } else if frame.flow.signal == FlowSignal::ReversalRisk {
        Decision::Exit
    } else if frame.timing.signal != TimingSignal::Optimal
        && !matches!(frame.flow.signal, FlowSignal::StrongContinuation)
    {
        Decision::Ignore
    } else if adversarial_risk > 0.82 {
        Decision::Exit
    } else if score < 0.38 || previous_score - score > 0.18 {
        Decision::Exit
    } else if score > threshold && score > previous_score + 0.025 && confidence > 0.40 {
        Decision::ScaleIn
    } else if score > threshold && confidence > 0.35 {
        Decision::EnterSmall
    } else {
        Decision::Ignore
    };

    let market = market_from_frame(frame, direction);
    ScoredDecision {
        market,
        event,
        features: frame.features.clone(),
        regime: frame.regime.clone(),
        context: frame.context.clone(),
        flow: frame.flow,
        timing: frame.timing,
        direction,
        confidence,
        continuation_prob: score,
        reversal_prob: (1.0 - score) * (0.5 + adversarial_risk * 0.5),
        score,
        decision,
        expected_duration_ms,
        urgency,
        expected_slippage_bps,
        data_latency_ms,
        adversarial_risk,
        edge_state: edge_snapshot.edge_state,
        edge_regime: edge_snapshot.edge_regime,
        edge_reliability_score: edge_snapshot.edge_reliability_score,
        edge_half_life_samples: edge_snapshot.edge_half_life_samples,
        dynamic_size_multiplier: edge_snapshot.position_size_multiplier,
        competition_state: edge_snapshot.competition_state,
        competition_score: edge_snapshot.competition_score,
        fill_probability: crate::types::FillProbabilityClass::LowFill,
    }
}

fn flow_penalty(signal: FlowSignal) -> f64 {
    match signal {
        FlowSignal::StrongContinuation => 0.0,
        FlowSignal::WeakContinuation => 0.18,
        FlowSignal::Exhaustion => 0.55,
        FlowSignal::ReversalRisk => 1.05,
    }
}

fn timing_penalty(signal: TimingSignal) -> f64 {
    match signal {
        TimingSignal::Optimal => 0.0,
        TimingSignal::Neutral => 0.16,
        TimingSignal::Wait => 0.42,
        TimingSignal::Missed => 0.95,
    }
}

fn infer_direction(frame: &MicrostructureFrame) -> Direction {
    let flow = frame.features.order_flow_delta + frame.features.micro_price_velocity;
    let book = frame.features.weighted_imbalance + frame.book.top_pressure;
    let combined = 0.60 * flow + 0.40 * book;
    if combined > 0.20 {
        Direction::Long
    } else if combined < -0.20 {
        Direction::Short
    } else {
        Direction::Flat
    }
}

fn market_from_frame(frame: &MicrostructureFrame, direction: Direction) -> MarketEvent {
    let price = frame
        .trade
        .map(|trade| trade.price)
        .unwrap_or_else(|| midpoint(frame.book.best_bid, frame.book.best_ask));
    MarketEvent {
        timestamp: frame.timestamp,
        price,
        volume: frame.trade.map(|trade| trade.volume).unwrap_or(0.0),
        side: direction.side().unwrap_or(Side::Buy),
        bid_ask_imbalance: frame.book.imbalance,
        spread: frame.book.spread,
    }
}

fn event_from_direction(direction: Direction, score: f64, threshold: f64) -> Event {
    match direction {
        Direction::Long if score > threshold => Event::PumpDetected,
        Direction::Short if score > threshold => Event::DumpDetected,
        _ => Event::Neutral,
    }
}

fn expected_duration(
    regime: &crate::types::MarketRegime,
    confidence: f64,
    trade_frequency: f64,
) -> u64 {
    let base = if regime.volatility > 2.5 {
        180.0
    } else {
        450.0
    };
    let speed = (trade_frequency / 40.0).clamp(0.5, 2.5);
    (base / speed * (1.15 - confidence * 0.45)).clamp(80.0, 1_500.0) as u64
}

fn estimate_slippage(frame: &MicrostructureFrame, direction: Direction) -> f64 {
    let top_qty = match direction {
        Direction::Long => frame.book.ask_volume,
        Direction::Short => frame.book.bid_volume,
        Direction::Flat => frame.book.bid_volume.min(frame.book.ask_volume),
    };
    let spread_bps = frame.book.spread * 10_000.0;
    let depth_penalty = (1.0 / top_qty.max(0.1)).min(8.0);
    (spread_bps * 0.55 + depth_penalty + frame.features.liquidity_pull * 4.0).clamp(0.1, 50.0)
}

fn urgency(
    score: f64,
    threshold: f64,
    expected_duration_ms: u64,
    burst: f64,
    adversarial: f64,
) -> f64 {
    let edge = (score - threshold).max(0.0);
    (edge * 2.0 + burst.min(4.0) * 0.12 + (500.0 / expected_duration_ms as f64) * 0.20
        - adversarial * 0.35)
        .clamp(0.0, 1.0)
}

#[inline]
fn sigmoid(x: f64) -> f64 {
    if x >= 0.0 {
        1.0 / (1.0 + (-x).exp())
    } else {
        let z = x.exp();
        z / (1.0 + z)
    }
}

#[inline]
fn midpoint(best_bid: f64, best_ask: f64) -> f64 {
    if best_bid > 0.0 && best_ask > 0.0 {
        (best_bid + best_ask) * 0.5
    } else {
        best_bid.max(best_ask)
    }
}

fn data_age_ms(timestamp: u64) -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(timestamp);
    now.saturating_sub(timestamp)
}

fn decision_label(decision: Decision) -> &'static str {
    match decision {
        Decision::Ignore => "ignore",
        Decision::EnterSmall => "enter_small",
        Decision::ScaleIn => "scale_in",
        Decision::Exit => "exit",
    }
}

fn regime_label(regime: &crate::types::MarketRegime) -> &'static str {
    if regime.spread > 10.0 {
        "wide_spread"
    } else if regime.volatility > 2.5 {
        "high_vol"
    } else if regime.trend_strength > 1.5 {
        "trending"
    } else {
        "normal"
    }
}
