use crate::types::{FillEvent, FlowSignal, OrderIntent, Side, TimingSignal};

#[derive(Clone, Debug, Default)]
pub struct AdverseSelectionDetector;

impl AdverseSelectionDetector {
    #[inline]
    pub fn pre_fill_score(intent: &OrderIntent) -> f64 {
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
        (0.42 * flow_risk
            + 0.28 * timing_risk
            + 0.15 * intent.expected_slippage_bps / intent.request.max_slippage_bps.max(1.0)
            + 0.15 * (1.0 - intent.context.stability_score))
            .clamp(0.0, 1.0)
    }

    #[inline]
    pub fn post_fill_score(intent: &OrderIntent, fill: &FillEvent) -> f64 {
        let side = match intent.request.side {
            Side::Buy => 1.0,
            Side::Sell => -1.0,
        };
        let price_against = ((fill.price - fill.requested_price) * side
            / fill.requested_price.max(f64::EPSILON)
            * 10_000.0)
            .max(0.0);
        (Self::pre_fill_score(intent) * 0.55
            + (price_against / intent.request.max_slippage_bps.max(1.0)).clamp(0.0, 1.0) * 0.30
            + (1.0 - intent.context.liquidity_score) * 0.15)
            .clamp(0.0, 1.0)
    }
}
