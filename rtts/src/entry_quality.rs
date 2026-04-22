use crate::types::{OrderIntent, Side};

pub fn score(intent: &OrderIntent) -> f64 {
    let side_sign = match intent.request.side {
        Side::Buy => 1.0,
        Side::Sell => -1.0,
    };
    let timing = (intent.urgency * 0.55 + (1.0 - intent.data_latency_ms as f64 / 250.0) * 0.45)
        .clamp(0.0, 1.0);
    let liquidity_support =
        (0.5 + intent.score * 0.25 - intent.regime.spread * 0.015).clamp(0.0, 1.0);
    let orderflow_alignment = (0.5 + side_sign * (intent.score - 0.5)).clamp(0.0, 1.0);
    let latency_impact = (1.0 - intent.data_latency_ms as f64 / 300.0).clamp(0.0, 1.0);
    let slippage_risk = (1.0
        - intent.expected_slippage_bps / intent.request.max_slippage_bps.max(1.0))
    .clamp(0.0, 1.0);

    (0.25 * timing
        + 0.22 * liquidity_support
        + 0.23 * orderflow_alignment
        + 0.15 * latency_impact
        + 0.15 * slippage_risk)
        .clamp(0.0, 1.0)
}
