use crate::{
    model_weights,
    metrics::Metrics,
    types::{FlowSignal, MicrostructureFrame, ReversalClassifierSnapshot, TimingSignal},
};
use std::{sync::Arc, time::Instant};
use tokio::sync::mpsc::{Receiver, Sender};

#[derive(Clone, Debug, Default)]
pub struct ReversalClassifier;

impl ReversalClassifier {
    #[inline]
    pub fn classify(frame: &MicrostructureFrame) -> ReversalClassifierSnapshot {
        let weights = model_weights::current();
        let params = weights.reversal_classifier;
        let movement_score = movement_score(frame, params);
        let intent_score = intent_score(frame, params);
        let trigger_bias = if frame.trigger.in_observation {
            frame.trigger.drop_pct.clamp(0.0, 0.04) / 0.04
        } else {
            0.0
        };
        let reversal_probability =
            (0.30 * movement_score + 0.55 * intent_score + params.trigger_observation_bonus * trigger_bias)
                .clamp(0.0, 1.0);
        let continuation_probability = continuation_score(frame, params);
        let chop_probability = (1.0 - reversal_probability.max(continuation_probability))
            .clamp(0.0, 1.0);

        ReversalClassifierSnapshot {
            reversal_probability,
            continuation_probability,
            chop_probability,
            intent_score,
            movement_score,
            ready_long: reversal_probability > continuation_probability
                && reversal_probability > chop_probability
                && reversal_probability > params.ready_bias_threshold
                && frame.features.micro_price_velocity <= 0.25
                && frame.tape.delta <= 0.0,
            ready_short: reversal_probability > continuation_probability
                && reversal_probability > chop_probability
                && reversal_probability > params.ready_bias_threshold
                && frame.features.micro_price_velocity >= -0.25
                && frame.tape.delta >= 0.0,
        }
    }
}

pub async fn run(
    mut rx: Receiver<MicrostructureFrame>,
    tx: Sender<MicrostructureFrame>,
    metrics: Arc<Metrics>,
) {
    while let Some(mut frame) = rx.recv().await {
        let started = Instant::now();
        frame.reversal_classifier = ReversalClassifier::classify(&frame);
        metrics
            .stage_latency_us
            .with_label_values(&["reversal_classifier"])
            .observe(started.elapsed().as_micros() as f64);
        if tx.try_send(frame.clone()).is_err() {
            metrics
                .channel_backpressure_total
                .with_label_values(&["reversal_classifier"])
                .inc();
            if tx.send(frame).await.is_err() {
                break;
            }
        }
    }
}

fn movement_score(
    frame: &MicrostructureFrame,
    weights: model_weights::ReversalClassifierWeights,
) -> f64 {
    let drop = frame.trigger.drop_pct.clamp(0.0, 0.04) / 0.04;
    let velocity = frame.features.micro_price_velocity.abs().clamp(0.0, 2.0) / 2.0;
    let volume = frame.tape.volume_burst.clamp(0.0, 4.0) / 4.0;
    (weights.movement_drop_pct * drop
        + weights.movement_velocity * velocity
        + weights.movement_volume_burst * volume)
        .clamp(0.0, 1.0)
}

fn intent_score(
    frame: &MicrostructureFrame,
    weights: model_weights::ReversalClassifierWeights,
) -> f64 {
    let exhaustion = frame.flow.exhaustion.clamp(0.0, 1.0);
    let weak_or_exhausted = matches!(
        frame.flow.signal,
        FlowSignal::Exhaustion | FlowSignal::WeakContinuation
    ) as u8 as f64;
    let absorption = frame.book.absorption.clamp(0.0, 1.0);
    let timing = if frame.timing.signal == TimingSignal::Optimal {
        frame.timing.timing_score.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let liquidity_ok = (1.0 - frame.features.liquidity_pull.clamp(0.0, 1.0))
        * frame.context.stability_score.clamp(0.0, 1.0);
    (weights.intent_exhaustion * exhaustion
        + weights.intent_weak_signal * weak_or_exhausted
        + weights.intent_absorption * absorption
        + weights.intent_timing * timing
        + weights.intent_liquidity_ok * liquidity_ok)
        .clamp(0.0, 1.0)
}

fn continuation_score(
    frame: &MicrostructureFrame,
    weights: model_weights::ReversalClassifierWeights,
) -> f64 {
    let directionality = frame.flow.continuation_strength.clamp(0.0, 1.0);
    let momentum = frame.features.micro_price_velocity.abs().clamp(0.0, 2.0) / 2.0;
    let imbalance = frame.book.weighted_imbalance.abs().clamp(0.0, 1.0);
    let aggression = frame.flow.aggressive_ratio.clamp(0.0, 1.0);
    (weights.continuation_directionality * directionality
        + weights.continuation_momentum * momentum
        + weights.continuation_imbalance * imbalance
        + weights.continuation_aggression * aggression)
        .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        EntryScoringSnapshot, Features, FlowState, MarketContext, MarketRegime, MicroTimingState,
        OrderBookState, RegimeKind, ReversalSnapshot, Side, TapeState, TradeEvent, TriggerSnapshot,
    };

    fn frame() -> MicrostructureFrame {
        MicrostructureFrame {
            timestamp: 1,
            trade: Some(TradeEvent {
                timestamp: 1,
                price: 100.0,
                volume: 1.0,
                side: Side::Sell,
            }),
            book: OrderBookState {
                best_bid: 99.9,
                best_ask: 100.1,
                bid_volume: 10.0,
                ask_volume: 8.0,
                imbalance: 0.1,
                liquidity_clusters: vec![],
                top_pressure: 0.05,
                weighted_imbalance: 0.10,
                spread: 0.002,
                spoofing_score: 0.05,
                liquidity_pull: 0.12,
                absorption: 0.45,
            },
            tape: TapeState {
                buy_volume: 4.0,
                sell_volume: 6.0,
                delta: -3.5,
                trade_frequency: 25.0,
                volume_burst: 2.2,
                exhaustion: 0.82,
                continuation: 0.15,
                last_price: 100.0,
            },
            features: Features {
                velocity: 0.0,
                vol_z: 0.0,
                imbalance: 0.1,
                volatility: 0.6,
                spread: 0.2,
                weighted_imbalance: 0.10,
                spread_dynamics: -0.2,
                micro_price_velocity: -0.32,
                trade_clustering: 0.0,
                liquidity_shift: 0.0,
                order_flow_delta: -0.55,
                absorption: 0.45,
                spoofing_risk: 0.05,
                liquidity_pull: 0.12,
            },
            regime: MarketRegime {
                volatility: 0.8,
                spread: 2.0,
                trend_strength: 0.4,
            },
            context: MarketContext {
                regime: RegimeKind::Normal,
                volatility: 0.8,
                liquidity_score: 0.8,
                stability_score: 0.75,
            },
            flow: FlowState {
                signal: FlowSignal::Exhaustion,
                aggressive_ratio: 0.4,
                absorption: 0.45,
                exhaustion: 0.88,
                continuation_strength: 0.18,
            },
            timing: MicroTimingState {
                signal: TimingSignal::Optimal,
                spread_compression: 0.3,
                liquidity_pull: 0.1,
                trade_burst: 0.2,
                micro_pullback: 0.15,
                timing_score: 0.84,
            },
            trigger: TriggerSnapshot {
                local_high: 101.5,
                drop_pct: 0.028,
                in_observation: true,
                confirmed: false,
                expected_edge: 0.0,
                should_exit: false,
            },
            reversal: ReversalSnapshot::default(),
            reversal_classifier: ReversalClassifierSnapshot::default(),
            entry_scoring: EntryScoringSnapshot::default(),
            stale: false,
        }
    }

    #[test]
    fn classifier_prefers_reversal_when_context_exhausts() {
        let snapshot = ReversalClassifier::classify(&frame());
        assert!(snapshot.reversal_probability > snapshot.continuation_probability);
        assert!(snapshot.ready_long);
    }
}
