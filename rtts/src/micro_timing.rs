use crate::types::{Features, MicroTimingState, OrderBookState, TapeState, TimingSignal};

#[derive(Clone, Debug, Default)]
pub struct MicroTimingEngine {
    previous_velocity: f64,
}

impl MicroTimingEngine {
    #[inline]
    pub fn update(
        &mut self,
        features: &Features,
        book: &OrderBookState,
        tape: &TapeState,
    ) -> MicroTimingState {
        let spread_compression = (-features.spread_dynamics).max(0.0).clamp(0.0, 5.0) / 5.0;
        let liquidity_pull = features.liquidity_pull.clamp(0.0, 1.0);
        let trade_burst = (tape.volume_burst / 4.0).clamp(0.0, 1.0);
        let micro_pullback = if self.previous_velocity.signum()
            == features.micro_price_velocity.signum()
            || self.previous_velocity.abs() <= f64::EPSILON
        {
            0.0
        } else {
            (features.micro_price_velocity.abs() / 5.0).clamp(0.0, 1.0)
        };
        let depth_ok = ((book.bid_volume + book.ask_volume) / 10.0).clamp(0.0, 1.0);
        let timing_score = (0.30 * spread_compression
            + 0.22 * trade_burst
            + 0.18 * micro_pullback
            + 0.18 * depth_ok
            + 0.12 * (1.0 - liquidity_pull))
            .clamp(0.0, 1.0);
        let signal = if liquidity_pull > 0.72 || features.micro_price_velocity.abs() > 4.5 {
            TimingSignal::Missed
        } else if timing_score > 0.62 && spread_compression > 0.15 {
            TimingSignal::Optimal
        } else if timing_score < 0.38 || features.spread_dynamics > 1.6 {
            TimingSignal::Wait
        } else {
            TimingSignal::Neutral
        };
        self.previous_velocity = features.micro_price_velocity;
        MicroTimingState {
            signal,
            spread_compression,
            liquidity_pull,
            trade_burst,
            micro_pullback,
            timing_score,
        }
    }
}
