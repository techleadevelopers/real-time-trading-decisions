use crate::types::Scenario;

#[derive(Clone, Copy, Debug)]
pub struct EvResult {
    pub ev: f64,
    pub adjusted_ev: f64,
    pub worst_case_loss: f64,
}

pub fn calculate(
    scenarios: &[Scenario],
    expected_slippage_bps: f64,
    latency_ms: u64,
    notional: f64,
) -> EvResult {
    let ev = scenarios
        .iter()
        .map(|scenario| scenario.probability * scenario.expected_pnl)
        .sum::<f64>();
    let worst_case_loss = scenarios
        .iter()
        .map(|scenario| scenario.expected_pnl - scenario.risk)
        .fold(0.0, f64::min)
        .abs();
    let slippage_cost = notional * expected_slippage_bps / 10_000.0;
    let latency_cost = notional * (latency_ms as f64 * 0.002).min(8.0) / 10_000.0;
    EvResult {
        ev,
        adjusted_ev: ev - slippage_cost - latency_cost,
        worst_case_loss,
    }
}
