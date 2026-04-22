use crate::types::{Decision, OrderIntent, Scenario, ScenarioType};

pub fn simulate(intent: &OrderIntent) -> Vec<Scenario> {
    let notional = intent.request.size * intent.last_price;
    let edge_bps = (intent.score - 0.5).max(0.0) * 18.0;
    let duration_penalty = (intent.expected_duration_ms as f64 / 1_000.0).min(1.5);
    let regime_noise = intent.regime.volatility * 0.08 + intent.regime.spread * 0.015;

    let continuation_prob = (0.34 + intent.score * 0.38 + intent.urgency * 0.10
        - regime_noise
        - intent.data_latency_ms as f64 * 0.0004)
        .clamp(0.05, 0.82);
    let reversal_prob = (0.18
        + (1.0 - intent.score) * 0.28
        + intent.regime.volatility * 0.06
        + intent.expected_slippage_bps * 0.012)
        .clamp(0.05, 0.75);
    let chop_prob = (1.0 - continuation_prob - reversal_prob).clamp(0.05, 0.75);
    let total = continuation_prob + reversal_prob + chop_prob;

    let continuation_pnl = notional * (edge_bps - intent.expected_slippage_bps) / 10_000.0;
    let reversal_loss_bps = (edge_bps * 0.70 + intent.expected_slippage_bps * 1.8 + 4.0).max(2.0);
    let chop_cost_bps = intent.expected_slippage_bps + 1.0 + duration_penalty;
    let scale_multiplier = if intent.reason == Decision::ScaleIn {
        1.25
    } else {
        1.0
    };

    vec![
        Scenario {
            name: ScenarioType::Continuation,
            probability: continuation_prob / total,
            expected_pnl: continuation_pnl * scale_multiplier,
            risk: notional * (intent.expected_slippage_bps + 1.0) / 10_000.0,
        },
        Scenario {
            name: ScenarioType::Reversal,
            probability: reversal_prob / total,
            expected_pnl: -notional * reversal_loss_bps / 10_000.0,
            risk: notional * reversal_loss_bps / 10_000.0,
        },
        Scenario {
            name: ScenarioType::Chop,
            probability: chop_prob / total,
            expected_pnl: -notional * chop_cost_bps / 10_000.0,
            risk: notional * chop_cost_bps / 10_000.0,
        },
    ]
}
