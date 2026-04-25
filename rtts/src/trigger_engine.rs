use crate::{
    config::Config,
    metrics::Metrics,
    types::{
        FlowSignal, MicrostructureFrame, Position, TimingSignal, TriggerSnapshot,
    },
};
use std::{sync::Arc, time::Instant};
use tokio::sync::mpsc::{Receiver, Sender};

#[derive(Clone, Copy, Debug, Default)]
pub struct PercentTriggerState {
    pub local_high: f64,
    pub drop_pct: f64,
    pub in_observation: bool,
}

#[derive(Clone, Debug, Default)]
pub struct TriggerEngine {
    state: PercentTriggerState,
    previous_sell_pressure: f64,
    decay_high_alpha: f64,
    drop_threshold: f64,
    reset_threshold: f64,
}

impl TriggerEngine {
    pub fn new(drop_threshold: f64, reset_threshold: f64) -> Self {
        Self {
            state: PercentTriggerState::default(),
            previous_sell_pressure: 0.0,
            decay_high_alpha: 0.003,
            drop_threshold: drop_threshold.clamp(0.001, 0.10),
            reset_threshold: reset_threshold.clamp(0.002, 0.20),
        }
    }

    pub fn observe(&mut self, frame: &MicrostructureFrame) -> TriggerSnapshot {
        let price = frame
            .trade
            .map(|trade| trade.price)
            .unwrap_or_else(|| mid_price(frame));
        if !price.is_finite() || price <= 0.0 {
            return TriggerSnapshot::default();
        }

        if self.state.local_high <= 0.0 {
            self.state.local_high = price;
        } else {
            self.state.local_high = self
                .state
                .local_high
                .max(price)
                .max(self.state.local_high * (1.0 - self.decay_high_alpha));
        }

        self.state.drop_pct =
            ((self.state.local_high - price) / self.state.local_high.max(f64::EPSILON)).clamp(0.0, 1.0);
        if self.state.drop_pct >= self.drop_threshold {
            self.state.in_observation = true;
        }

        let confirmed = self.state.in_observation && confirm_reversal(frame, self.previous_sell_pressure);
        let should_exit = should_exit(frame, &Position::default());
        let expected_edge = if confirmed {
            derive_expected_edge(frame)
        } else {
            0.0
        };

        if confirmed || self.state.drop_pct < self.drop_threshold * 0.35 {
            self.state.in_observation = false;
        } else if self.state.in_observation && self.state.drop_pct >= self.reset_threshold {
            self.state.in_observation = false;
            self.state.local_high = price;
            self.state.drop_pct = 0.0;
        }

        self.previous_sell_pressure = sell_pressure(frame);

        TriggerSnapshot {
            local_high: self.state.local_high,
            drop_pct: self.state.drop_pct,
            in_observation: self.state.in_observation,
            confirmed,
            expected_edge,
            should_exit,
        }
    }
}

pub async fn run(
    cfg: Config,
    mut rx: Receiver<MicrostructureFrame>,
    tx: Sender<MicrostructureFrame>,
    metrics: Arc<Metrics>,
) {
    let mut engine = TriggerEngine::new(cfg.trigger_drop_pct, cfg.trigger_reset_pct);
    while let Some(mut frame) = rx.recv().await {
        let started = Instant::now();
        frame.trigger = engine.observe(&frame);
        metrics
            .stage_latency_us
            .with_label_values(&["trigger_engine"])
            .observe(started.elapsed().as_micros() as f64);

        if tx.try_send(frame.clone()).is_err() {
            metrics
                .channel_backpressure_total
                .with_label_values(&["trigger_engine"])
                .inc();
            if tx.send(frame).await.is_err() {
                break;
            }
        }
    }
}

pub fn confirm_reversal(frame: &MicrostructureFrame, previous_sell_pressure: f64) -> bool {
    let flow_ok = matches!(
        frame.flow.signal,
        FlowSignal::Exhaustion | FlowSignal::WeakContinuation
    );
    let selling_weakening =
        sell_pressure(frame) < previous_sell_pressure * 0.82 || frame.tape.exhaustion > 0.42;
    let orderbook_ok = frame.book.absorption > 0.28
        || (frame.book.liquidity_pull < 0.28
            && frame.book.top_pressure > -0.12
            && frame.context.stability_score > 0.45);
    let timing_ok = frame.timing.signal == TimingSignal::Optimal;
    flow_ok && selling_weakening && orderbook_ok && timing_ok
}

pub fn should_exit(frame: &MicrostructureFrame, position: &Position) -> bool {
    if !position.is_open() {
        return frame.trigger.should_exit;
    }
    let markout_degrading = frame.features.micro_price_velocity < -0.85
        && frame.flow.continuation_strength < 0.38
        && frame.features.order_flow_delta < -0.45;
    let flow_flip = matches!(
        frame.flow.signal,
        FlowSignal::ReversalRisk | FlowSignal::Exhaustion
    ) && frame.book.weighted_imbalance < -0.20;
    let adverse_selection_rising =
        frame.features.liquidity_pull > 0.48 || frame.book.spoofing_score > 0.42;
    let timing_lost = matches!(frame.timing.signal, TimingSignal::Missed | TimingSignal::Wait);
    let hard_fallback = position.unrealized_pnl < -0.004 * position.avg_price.abs() * position.size.abs();
    markout_degrading || flow_flip || adverse_selection_rising || timing_lost || hard_fallback
}

fn derive_expected_edge(frame: &MicrostructureFrame) -> f64 {
    (frame.flow.exhaustion * 1.6
        + frame.book.absorption * 1.2
        + frame.timing.timing_score
        + frame.context.stability_score * 0.6
        - frame.features.liquidity_pull * 0.9
        - frame.regime.spread * 0.04)
        .max(0.0)
}

fn sell_pressure(frame: &MicrostructureFrame) -> f64 {
    ((-frame.tape.delta).max(0.0)
        + (-frame.features.order_flow_delta).max(0.0) * 0.65
        + (-frame.book.top_pressure).max(0.0) * 2.5)
        .max(0.0)
}

fn mid_price(frame: &MicrostructureFrame) -> f64 {
    if frame.book.best_bid > 0.0 && frame.book.best_ask > 0.0 {
        (frame.book.best_bid + frame.book.best_ask) * 0.5
    } else {
        frame.tape.last_price
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        Features, FlowState, MarketContext, MarketRegime, OrderBookState, RegimeKind, Side,
        TapeState, TimingSignal, TradeEvent,
    };

    fn frame(price: f64) -> MicrostructureFrame {
        MicrostructureFrame {
            timestamp: 1,
            trade: Some(TradeEvent {
                timestamp: 1,
                price,
                volume: 1.0,
                side: Side::Sell,
            }),
            book: OrderBookState {
                best_bid: price - 0.1,
                best_ask: price + 0.1,
                bid_volume: 10.0,
                ask_volume: 8.0,
                imbalance: 0.15,
                liquidity_clusters: Vec::new(),
                top_pressure: 0.05,
                weighted_imbalance: 0.12,
                spread: 0.002,
                spoofing_score: 0.05,
                liquidity_pull: 0.10,
                absorption: 0.40,
            },
            tape: TapeState {
                buy_volume: 4.0,
                sell_volume: 6.0,
                delta: -2.0,
                trade_frequency: 20.0,
                volume_burst: 1.2,
                exhaustion: 0.55,
                continuation: 0.15,
                last_price: price,
            },
            features: Features {
                velocity: 0.0,
                vol_z: 0.0,
                imbalance: 0.1,
                volatility: 0.4,
                spread: 0.2,
                weighted_imbalance: 0.1,
                spread_dynamics: -0.3,
                micro_price_velocity: 0.15,
                trade_clustering: 0.0,
                liquidity_shift: 0.0,
                order_flow_delta: -0.25,
                absorption: 0.40,
                spoofing_risk: 0.05,
                liquidity_pull: 0.10,
            },
            regime: MarketRegime {
                volatility: 0.5,
                spread: 2.0,
                trend_strength: 0.4,
            },
            context: MarketContext {
                regime: RegimeKind::Normal,
                volatility: 0.4,
                liquidity_score: 0.8,
                stability_score: 0.7,
            },
            flow: FlowState {
                signal: FlowSignal::Exhaustion,
                aggressive_ratio: 0.4,
                absorption: 0.4,
                exhaustion: 0.6,
                continuation_strength: 0.25,
            },
            timing: crate::types::MicroTimingState {
                signal: TimingSignal::Optimal,
                spread_compression: 0.4,
                liquidity_pull: 0.1,
                trade_burst: 0.2,
                micro_pullback: 0.15,
                timing_score: 0.75,
            },
            trigger: TriggerSnapshot::default(),
            reversal: crate::types::ReversalSnapshot::default(),
            reversal_classifier: crate::types::ReversalClassifierSnapshot::default(),
            entry_scoring: crate::types::EntryScoringSnapshot::default(),
            stale: false,
        }
    }

    #[test]
    fn drop_detection_correctness() {
        let mut engine = TriggerEngine::new(0.015, 0.025);
        let _ = engine.observe(&frame(100.0));
        let snapshot = engine.observe(&frame(98.4));
        assert!(snapshot.drop_pct >= 0.015);
    }

    #[test]
    fn no_entry_without_confirmation() {
        let mut engine = TriggerEngine::new(0.015, 0.025);
        let _ = engine.observe(&frame(100.0));
        let mut weak = frame(98.4);
        weak.timing.signal = TimingSignal::Neutral;
        let snapshot = engine.observe(&weak);
        assert!(!snapshot.confirmed);
    }

    #[test]
    fn entry_when_all_signals_align() {
        let mut engine = TriggerEngine::new(0.015, 0.025);
        let _ = engine.observe(&frame(100.0));
        engine.previous_sell_pressure = 6.0;
        let snapshot = engine.observe(&frame(98.4));
        assert!(snapshot.confirmed);
        assert!(snapshot.expected_edge > 0.0);
    }

    #[test]
    fn reset_on_continued_drop() {
        let mut engine = TriggerEngine::new(0.015, 0.020);
        let _ = engine.observe(&frame(100.0));
        let _ = engine.observe(&frame(98.4));
        let snapshot = engine.observe(&frame(97.0));
        assert!(!snapshot.in_observation);
    }

    #[test]
    fn exit_on_markout_degradation() {
        let mut degraded = frame(99.0);
        degraded.features.micro_price_velocity = -1.2;
        degraded.features.order_flow_delta = -0.8;
        degraded.flow.continuation_strength = 0.15;
        let position = Position {
            size: 1.0,
            avg_price: 100.0,
            entries: 1,
            confidence: 0.5,
            unrealized_pnl: -1.5,
        };
        assert!(should_exit(&degraded, &position));
    }
}
