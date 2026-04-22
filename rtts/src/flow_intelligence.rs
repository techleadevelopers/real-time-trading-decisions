use crate::types::{Direction, Features, FlowSignal, FlowState, OrderBookState, TapeState};

#[derive(Clone, Debug, Default)]
pub struct FlowIntelligence {
    previous_delta: f64,
    previous_continuation: f64,
}

impl FlowIntelligence {
    #[inline]
    pub fn update(
        &mut self,
        features: &Features,
        book: &OrderBookState,
        tape: &TapeState,
        direction: Direction,
    ) -> FlowState {
        let total = (tape.buy_volume + tape.sell_volume).max(f64::EPSILON);
        let aggressive_ratio = (tape.delta.abs() / total).clamp(0.0, 1.0);
        let side = match direction {
            Direction::Long => 1.0,
            Direction::Short => -1.0,
            Direction::Flat => tape.delta.signum(),
        };
        let aligned_flow = (features.order_flow_delta * side).max(0.0).clamp(0.0, 5.0) / 5.0;
        let aligned_book = (book.weighted_imbalance * side).max(0.0).clamp(0.0, 1.0);
        let continuation_strength =
            (0.42 * aggressive_ratio + 0.32 * aligned_flow + 0.26 * aligned_book).clamp(0.0, 1.0);
        let delta_decay = (self.previous_delta.abs() - tape.delta.abs()).max(0.0)
            / self.previous_delta.abs().max(1.0);
        let exhaustion =
            (tape.exhaustion.max(delta_decay) + book.absorption * 0.35).clamp(0.0, 1.0);
        let reversal_pressure =
            ((features.order_flow_delta.signum() != side.signum()) as u8 as f64 * aggressive_ratio
                + features.liquidity_pull * 0.45
                + features.spoofing_risk * 0.35)
                .clamp(0.0, 1.0);

        let signal = if reversal_pressure > 0.62 || exhaustion > 0.74 {
            FlowSignal::ReversalRisk
        } else if exhaustion > 0.55 || continuation_strength < self.previous_continuation * 0.72 {
            FlowSignal::Exhaustion
        } else if continuation_strength > 0.68 && aggressive_ratio > 0.35 {
            FlowSignal::StrongContinuation
        } else {
            FlowSignal::WeakContinuation
        };

        self.previous_delta = tape.delta;
        self.previous_continuation = continuation_strength;
        FlowState {
            signal,
            aggressive_ratio,
            absorption: book.absorption,
            exhaustion,
            continuation_strength,
        }
    }
}
