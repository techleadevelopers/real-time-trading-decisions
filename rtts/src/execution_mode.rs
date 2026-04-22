use crate::types::{
    ExecutionMode, FillProbabilityClass, FlowSignal, OrderIntent, RegimeKind, TimingSignal,
};

#[derive(Clone, Debug, Default)]
pub struct ExecutionModeSwitch;

impl ExecutionModeSwitch {
    #[inline]
    pub fn choose(intent: &OrderIntent, fill: FillProbabilityClass) -> ExecutionMode {
        if intent.request.reduce_only
            || intent.context.regime == RegimeKind::NewsShock
            || intent.flow.signal == FlowSignal::ReversalRisk
            || intent.timing.signal == TimingSignal::Missed
        {
            return ExecutionMode::Defensive;
        }
        if intent.urgency > 0.70
            || intent.expected_duration_ms < 160
            || (fill == FillProbabilityClass::LowFill
                && intent.flow.signal == FlowSignal::StrongContinuation)
        {
            ExecutionMode::Aggressive
        } else {
            ExecutionMode::Passive
        }
    }
}
