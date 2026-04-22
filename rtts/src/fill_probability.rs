use crate::types::{FillProbabilityClass, FlowSignal, OrderIntent, QueueEstimate};

#[derive(Clone, Debug, Default)]
pub struct FillProbabilityModel;

impl FillProbabilityModel {
    #[inline]
    pub fn classify(intent: &OrderIntent, queue: &QueueEstimate) -> FillProbabilityClass {
        let pressure = match intent.flow.signal {
            FlowSignal::StrongContinuation => 0.18,
            FlowSignal::WeakContinuation => 0.0,
            FlowSignal::Exhaustion => -0.18,
            FlowSignal::ReversalRisk => -0.35,
        };
        let score = queue.fill_probability + pressure + intent.timing.trade_burst * 0.10
            - (intent.regime.spread / 25.0).clamp(0.0, 0.30)
            - (queue.queue_position / 12.0).clamp(0.0, 0.25);
        if score > 0.58 {
            FillProbabilityClass::HighFill
        } else {
            FillProbabilityClass::LowFill
        }
    }
}
