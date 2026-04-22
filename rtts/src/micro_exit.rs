use crate::types::{FlowSignal, MicroExitReason, MicroExitSignal, OrderIntent, Side};

#[derive(Clone, Debug, Default)]
pub struct MicroExitEngine;

impl MicroExitEngine {
    #[inline]
    pub fn evaluate(intent: &OrderIntent, fill_price: f64, adverse_score: f64) -> MicroExitSignal {
        if intent.context.liquidity_score < 0.28 {
            return MicroExitSignal {
                reason: MicroExitReason::LiquidityCollapse,
                reduce_ratio: 1.0,
                urgency: 1.0,
            };
        }
        if adverse_score > 0.74 || intent.flow.signal == FlowSignal::ReversalRisk {
            return MicroExitSignal {
                reason: MicroExitReason::AdverseFlow,
                reduce_ratio: 1.0,
                urgency: 0.95,
            };
        }
        if intent.flow.signal == FlowSignal::Exhaustion {
            return MicroExitSignal {
                reason: MicroExitReason::MomentumFade,
                reduce_ratio: 0.50,
                urgency: 0.70,
            };
        }
        let side = match intent.request.side {
            Side::Buy => 1.0,
            Side::Sell => -1.0,
        };
        let favorable_bps =
            (intent.last_price - fill_price) * side / fill_price.max(f64::EPSILON) * 10_000.0;
        if favorable_bps > intent.expected_slippage_bps + 2.0 {
            return MicroExitSignal {
                reason: MicroExitReason::TakeProfit,
                reduce_ratio: 0.35,
                urgency: 0.45,
            };
        }
        MicroExitSignal::default()
    }
}
