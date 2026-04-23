use crate::types::{MarkoutSnapshot, OrderIntent, Side};

#[derive(Clone, Debug, Default)]
pub struct MarkoutAnalysisEngine;

impl MarkoutAnalysisEngine {
    #[inline]
    pub fn estimate(intent: &OrderIntent, fill_price: f64, filled_size: f64) -> MarkoutSnapshot {
        let side = match intent.request.side {
            Side::Buy => 1.0,
            Side::Sell => -1.0,
        };
        let quality = intent.flow.continuation_strength * 0.45
            + intent.timing.timing_score * 0.35
            + intent.context.stability_score * 0.20
            - intent.expected_slippage_bps / 50.0;
        let bps_100 = (quality - 0.45) * 2.0;
        let bps_500 = (quality - 0.42) * 4.5;
        let bps_1s = (quality - 0.40) * 7.0;
        MarkoutSnapshot {
            pnl_100ms: markout(fill_price, filled_size, side, bps_100),
            pnl_500ms: markout(fill_price, filled_size, side, bps_500),
            pnl_1s: markout(fill_price, filled_size, side, bps_1s),
            pnl_5s: markout(fill_price, filled_size, side, bps_1s * 1.8),
        }
    }
}

#[inline]
fn markout(price: f64, size: f64, side: f64, bps: f64) -> f64 {
    let future = price * (1.0 + side * bps / 10_000.0);
    (future - price) * size * side
}
