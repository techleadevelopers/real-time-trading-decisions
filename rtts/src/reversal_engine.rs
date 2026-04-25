use crate::{
    metrics::Metrics,
    types::{
        Direction, FlowSignal, LearningSample, MicrostructureFrame, ReversalSnapshot,
        ReversalState, Side, TimingSignal,
    },
};
use std::{sync::Arc, time::Instant};
use tokio::sync::mpsc::{Receiver, Sender};

const DEFAULT_SHORT_THRESHOLD_MS: u64 = 10_000;
const DEFAULT_MAX_MOVE_PCT: f64 = 0.0035;

#[derive(Clone, Copy, Debug)]
pub struct ReversalContext {
    pub state: ReversalState,
    pub activation_time: u64,
    pub last_exit_price: f64,
    pub last_direction: Side,
}

impl Default for ReversalContext {
    fn default() -> Self {
        Self {
            state: ReversalState::Idle,
            activation_time: 0,
            last_exit_price: 0.0,
            last_direction: Side::Buy,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReversalEngine {
    context: ReversalContext,
    observation_timeout_ms: u64,
    max_price_move_pct: f64,
    previous_buy_pressure: f64,
    previous_sell_pressure: f64,
    last_market_price: f64,
    last_market_ts: u64,
}

impl Default for ReversalEngine {
    fn default() -> Self {
        Self::new(DEFAULT_SHORT_THRESHOLD_MS)
    }
}

impl ReversalEngine {
    pub fn new(observation_timeout_ms: u64) -> Self {
        Self {
            context: ReversalContext::default(),
            observation_timeout_ms,
            max_price_move_pct: DEFAULT_MAX_MOVE_PCT,
            previous_buy_pressure: 0.0,
            previous_sell_pressure: 0.0,
            last_market_price: 0.0,
            last_market_ts: 0,
        }
    }

    pub fn observe_learning(&mut self, sample: &LearningSample, frame: Option<&MicrostructureFrame>) {
        let regime_is_trend = frame
            .map(|value| value.context.regime == crate::types::RegimeKind::TrendExpansion)
            .unwrap_or(false);
        if sample.pnl <= 0.0 || sample.duration_ms >= DEFAULT_SHORT_THRESHOLD_MS || regime_is_trend {
            return;
        }
        let Some(last_direction) = sample.direction.side() else {
            return;
        };
        let Some(price) = frame.map(market_price).filter(|value| *value > 0.0).or({
            if self.last_market_price > 0.0 {
                Some(self.last_market_price)
            } else {
                None
            }
        }) else {
            return;
        };
        self.context = ReversalContext {
            state: match last_direction {
                Side::Buy => ReversalState::ObserveLongToShort,
                Side::Sell => ReversalState::ObserveShortToLong,
            },
            activation_time: frame.map(|value| value.timestamp).unwrap_or(sample.timestamp),
            last_exit_price: price,
            last_direction,
        };
    }

    pub fn observe_frame(&mut self, frame: &MicrostructureFrame) -> ReversalSnapshot {
        self.last_market_price = market_price(frame);
        self.last_market_ts = frame.timestamp;
        let snapshot = match self.context.state {
            ReversalState::Idle => ReversalSnapshot::default(),
            ReversalState::ObserveLongToShort => self.observe_long_to_short(frame),
            ReversalState::ObserveShortToLong => self.observe_short_to_long(frame),
        };
        self.previous_buy_pressure = buy_pressure(frame);
        self.previous_sell_pressure = sell_pressure(frame);
        snapshot
    }

    fn observe_long_to_short(&mut self, frame: &MicrostructureFrame) -> ReversalSnapshot {
        if self.expired(frame) || !allow_flip(&frame.context) || new_dominant_trend(frame, Direction::Long) {
            self.reset();
            return ReversalSnapshot::default();
        }
        let confirmed = matches!(
            frame.flow.signal,
            FlowSignal::Exhaustion | FlowSignal::WeakContinuation
        ) && buy_pressure(frame) < self.previous_buy_pressure * 0.82
            && (frame.book.absorption > 0.30 || frame.book.top_pressure < 0.0)
            && frame.timing.signal == TimingSignal::Optimal
            && frame.context.regime != crate::types::RegimeKind::TrendExpansion
            && frame.features.spread_dynamics <= 0.65
            && frame.regime.volatility < 3.0;
        let snapshot = ReversalSnapshot {
            state: self.context.state,
            active: true,
            confirmed,
            direction: Direction::Short,
            confidence: reversal_confidence(frame, confirmed),
            expected_edge: if confirmed { reversal_edge(frame) } else { 0.0 },
        };
        if confirmed {
            self.reset();
        }
        snapshot
    }

    fn observe_short_to_long(&mut self, frame: &MicrostructureFrame) -> ReversalSnapshot {
        if self.expired(frame) || !allow_flip(&frame.context) || new_dominant_trend(frame, Direction::Short) {
            self.reset();
            return ReversalSnapshot::default();
        }
        let confirmed = matches!(
            frame.flow.signal,
            FlowSignal::Exhaustion | FlowSignal::WeakContinuation
        ) && sell_pressure(frame) < self.previous_sell_pressure * 0.82
            && (frame.book.absorption > 0.30 || frame.book.top_pressure > 0.0)
            && frame.timing.signal == TimingSignal::Optimal
            && frame.context.regime != crate::types::RegimeKind::TrendExpansion
            && frame.features.spread_dynamics <= 0.65
            && frame.regime.volatility < 3.0;
        let snapshot = ReversalSnapshot {
            state: self.context.state,
            active: true,
            confirmed,
            direction: Direction::Long,
            confidence: reversal_confidence(frame, confirmed),
            expected_edge: if confirmed { reversal_edge(frame) } else { 0.0 },
        };
        if confirmed {
            self.reset();
        }
        snapshot
    }

    fn expired(&self, frame: &MicrostructureFrame) -> bool {
        if self.context.state == ReversalState::Idle {
            return false;
        }
        if frame.timestamp.saturating_sub(self.context.activation_time) > self.observation_timeout_ms {
            return true;
        }
        let price = market_price(frame);
        let move_pct =
            ((price - self.context.last_exit_price).abs() / self.context.last_exit_price.max(f64::EPSILON))
                .clamp(0.0, 1.0);
        move_pct > self.max_price_move_pct
    }

    fn reset(&mut self) {
        self.context.state = ReversalState::Idle;
        self.context.activation_time = 0;
        self.context.last_exit_price = 0.0;
    }
}

pub async fn run(
    mut frame_rx: Receiver<MicrostructureFrame>,
    mut learning_rx: Receiver<LearningSample>,
    tx: Sender<MicrostructureFrame>,
    metrics: Arc<Metrics>,
) {
    let mut engine = ReversalEngine::default();
    let mut latest_frame: Option<MicrostructureFrame> = None;
    loop {
        tokio::select! {
            Some(sample) = learning_rx.recv() => {
                engine.observe_learning(&sample, latest_frame.as_ref());
            }
            Some(mut frame) = frame_rx.recv() => {
                let started = Instant::now();
                frame.reversal = engine.observe_frame(&frame);
                latest_frame = Some(frame.clone());
                metrics.stage_latency_us.with_label_values(&["reversal_engine"]).observe(started.elapsed().as_micros() as f64);
                if tx.try_send(frame.clone()).is_err() {
                    metrics.channel_backpressure_total.with_label_values(&["reversal_engine"]).inc();
                    if tx.send(frame).await.is_err() {
                        break;
                    }
                }
            }
            else => break,
        }
    }
}

pub fn allow_flip(context: &crate::types::MarketContext) -> bool {
    context.regime != crate::types::RegimeKind::TrendExpansion
        && context.regime != crate::types::RegimeKind::HighVolatility
        && context.stability_score > 0.42
        && context.liquidity_score > 0.38
}

fn new_dominant_trend(frame: &MicrostructureFrame, blocked_direction: Direction) -> bool {
    match blocked_direction {
        Direction::Long => {
            frame.regime.trend_strength > 1.2
                && frame.features.micro_price_velocity > 0.8
                && frame.features.order_flow_delta > 0.4
        }
        Direction::Short => {
            frame.regime.trend_strength > 1.2
                && frame.features.micro_price_velocity < -0.8
                && frame.features.order_flow_delta < -0.4
        }
        Direction::Flat => false,
    }
}

fn reversal_confidence(frame: &MicrostructureFrame, confirmed: bool) -> f64 {
    if !confirmed {
        return 0.0;
    }
    (0.35 * frame.flow.exhaustion
        + 0.30 * frame.book.absorption
        + 0.20 * frame.timing.timing_score
        + 0.15 * frame.context.stability_score)
        .clamp(0.0, 1.0)
}

fn reversal_edge(frame: &MicrostructureFrame) -> f64 {
    (frame.flow.exhaustion * 1.4
        + frame.book.absorption
        + frame.timing.timing_score * 0.8
        - frame.regime.spread * 0.05
        - frame.features.liquidity_pull * 0.75)
        .max(0.0)
}

fn market_price(frame: &MicrostructureFrame) -> f64 {
    frame.trade.map(|trade| trade.price).unwrap_or_else(|| {
        if frame.book.best_bid > 0.0 && frame.book.best_ask > 0.0 {
            (frame.book.best_bid + frame.book.best_ask) * 0.5
        } else {
            frame.tape.last_price
        }
    })
}

fn buy_pressure(frame: &MicrostructureFrame) -> f64 {
    (frame.tape.delta.max(0.0)
        + frame.features.order_flow_delta.max(0.0) * 0.65
        + frame.book.top_pressure.max(0.0) * 2.5)
        .max(0.0)
}

fn sell_pressure(frame: &MicrostructureFrame) -> f64 {
    ((-frame.tape.delta).max(0.0)
        + (-frame.features.order_flow_delta).max(0.0) * 0.65
        + (-frame.book.top_pressure).max(0.0) * 2.5)
        .max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        CompetitionState, Features, FlowState, MarketContext, MarketRegime, MicroTimingState,
        OrderBookState, RegimeKind, Side, TapeState, TimingSignal, TradeEvent, TriggerSnapshot,
    };

    fn frame(price: f64) -> MicrostructureFrame {
        MicrostructureFrame {
            timestamp: 1_000,
            trade: Some(TradeEvent {
                timestamp: 1_000,
                price,
                volume: 1.0,
                side: Side::Buy,
            }),
            book: OrderBookState {
                best_bid: price - 0.1,
                best_ask: price + 0.1,
                bid_volume: 12.0,
                ask_volume: 9.0,
                imbalance: 0.1,
                liquidity_clusters: Vec::new(),
                top_pressure: 0.08,
                weighted_imbalance: 0.12,
                spread: 0.002,
                spoofing_score: 0.05,
                liquidity_pull: 0.08,
                absorption: 0.45,
            },
            tape: TapeState {
                buy_volume: 6.0,
                sell_volume: 4.0,
                delta: 2.0,
                trade_frequency: 18.0,
                volume_burst: 1.1,
                exhaustion: 0.58,
                continuation: 0.20,
                last_price: price,
            },
            features: Features {
                velocity: 0.0,
                vol_z: 0.0,
                imbalance: 0.1,
                volatility: 0.6,
                spread: 0.2,
                weighted_imbalance: 0.12,
                spread_dynamics: -0.1,
                micro_price_velocity: -0.10,
                trade_clustering: 0.0,
                liquidity_shift: 0.0,
                order_flow_delta: 0.15,
                absorption: 0.45,
                spoofing_risk: 0.05,
                liquidity_pull: 0.08,
            },
            regime: MarketRegime {
                volatility: 0.7,
                spread: 2.0,
                trend_strength: 0.5,
            },
            context: MarketContext {
                regime: RegimeKind::Normal,
                volatility: 0.7,
                liquidity_score: 0.8,
                stability_score: 0.75,
            },
            flow: FlowState {
                signal: FlowSignal::Exhaustion,
                aggressive_ratio: 0.35,
                absorption: 0.45,
                exhaustion: 0.62,
                continuation_strength: 0.28,
            },
            timing: MicroTimingState {
                signal: TimingSignal::Optimal,
                spread_compression: 0.4,
                liquidity_pull: 0.1,
                trade_burst: 0.2,
                micro_pullback: 0.2,
                timing_score: 0.74,
            },
            trigger: TriggerSnapshot::default(),
            reversal: ReversalSnapshot::default(),
            reversal_classifier: crate::types::ReversalClassifierSnapshot::default(),
            entry_scoring: crate::types::EntryScoringSnapshot::default(),
            stale: false,
        }
    }

    fn sample(pnl: f64, duration_ms: u64, direction: Direction) -> LearningSample {
        LearningSample {
            timestamp: 1_000,
            direction,
            confidence: 0.7,
            predicted_score: 0.7,
            expected_slippage_bps: 1.0,
            actual_slippage_bps: 0.5,
            pnl,
            expected_markout: 1.0,
            realized_markout: 1.2,
            execution_alpha: 0.2,
            fill_ratio: 1.0,
            fees_paid: 0.0,
            rebates_received: 0.0,
            funding_cost: 0.0,
            edge_component: 1.0,
            execution_loss: 0.0,
            fees_rebates_component: 0.0,
            adverse_selection_loss: 0.0,
            edge_capture_ratio: 1.2,
            competition_state: CompetitionState::Normal,
            duration_ms,
            entry_quality: 0.8,
            markout_100ms: 0.5,
            markout_500ms: 1.0,
            markout_1s: 1.0,
            markout_5s: 1.0,
            regime: MarketRegime::default(),
        }
    }

    #[test]
    fn activates_only_after_profitable_trade() {
        let mut engine = ReversalEngine::default();
        let frame = frame(100.0);
        engine.observe_learning(&sample(1.0, 500, Direction::Long), Some(&frame));
        assert_eq!(engine.context.state, ReversalState::ObserveLongToShort);
        let mut engine = ReversalEngine::default();
        engine.observe_learning(&sample(-1.0, 500, Direction::Long), Some(&frame));
        assert_eq!(engine.context.state, ReversalState::Idle);
    }

    #[test]
    fn does_not_trigger_in_trend_expansion() {
        let mut engine = ReversalEngine::default();
        let mut frame = frame(100.0);
        frame.context.regime = RegimeKind::TrendExpansion;
        engine.observe_learning(&sample(1.0, 500, Direction::Long), Some(&frame));
        assert_eq!(engine.context.state, ReversalState::Idle);
    }

    #[test]
    fn does_not_flip_without_confirmation() {
        let mut engine = ReversalEngine::default();
        let base = frame(100.0);
        engine.observe_learning(&sample(1.0, 500, Direction::Long), Some(&base));
        let mut weak = frame(99.9);
        weak.timing.signal = TimingSignal::Neutral;
        let snapshot = engine.observe_frame(&weak);
        assert!(!snapshot.confirmed);
    }

    #[test]
    fn flips_correctly_when_all_signals_align() {
        let mut engine = ReversalEngine::default();
        let base = frame(100.0);
        engine.observe_learning(&sample(1.0, 500, Direction::Long), Some(&base));
        engine.previous_buy_pressure = 6.0;
        let snapshot = engine.observe_frame(&frame(99.95));
        assert!(snapshot.confirmed);
        assert_eq!(snapshot.direction, Direction::Short);
    }

    #[test]
    fn resets_properly_on_timeout() {
        let mut engine = ReversalEngine::new(500);
        let base = frame(100.0);
        engine.observe_learning(&sample(1.0, 500, Direction::Long), Some(&base));
        let mut late = frame(100.1);
        late.timestamp = 2_000;
        let snapshot = engine.observe_frame(&late);
        assert!(!snapshot.active);
        assert_eq!(engine.context.state, ReversalState::Idle);
    }
}
