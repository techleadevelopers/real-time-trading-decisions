use crate::types::{FlowSignal, OrderIntent, OrderType, QueueEstimate, SymbolProfile};

#[derive(Clone, Debug, Default)]
pub struct QueuePositionEngine;

impl QueuePositionEngine {
    #[inline]
    pub fn estimate(intent: &OrderIntent, profile: &SymbolProfile) -> QueueEstimate {
        let passive = if intent.request.order_type == OrderType::Limit {
            1.0
        } else {
            0.0
        };
        let spread_pressure =
            (intent.regime.spread / profile.avg_spread_bps.max(0.1)).clamp(0.25, 4.0);
        let flow_discount = match intent.flow.signal {
            FlowSignal::StrongContinuation => 0.72,
            FlowSignal::WeakContinuation => 1.00,
            FlowSignal::Exhaustion => 1.35,
            FlowSignal::ReversalRisk => 1.80,
        };
        let volume_ahead = intent.request.size
            * (1.0 + passive * spread_pressure)
            * flow_discount
            * (1.0 + (1.0 - intent.timing.timing_score).clamp(0.0, 1.0));
        let queue_position = volume_ahead / profile.avg_trade_size.max(0.0001);
        let placement_depth_bps = if intent.request.order_type == OrderType::Limit {
            (0.15 + intent.regime.spread.min(20.0) * 0.06 + queue_position.min(8.0) * 0.04)
                .clamp(0.1, 3.0)
        } else {
            0.0
        };
        let fill_probability = (profile.avg_fill_probability * 0.35
            + intent.timing.timing_score * 0.30
            + intent.flow.continuation_strength * 0.22
            + (1.0 / (1.0 + queue_position)).min(1.0) * 0.13)
            .clamp(0.02, 0.98);
        QueueEstimate {
            queue_position,
            volume_ahead,
            fill_probability,
            placement_depth_bps,
        }
    }
}
