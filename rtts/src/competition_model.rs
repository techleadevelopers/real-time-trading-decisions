use crate::types::{OrderIntent, OrderType, RegimeKind};

pub fn score(intent: &OrderIntent) -> f64 {
    let urgency_crowding = intent.urgency * 0.32;
    let short_duration = (1.0 - intent.expected_duration_ms as f64 / 800.0).clamp(0.0, 1.0) * 0.24;
    let spread_signal = (1.0 - intent.regime.spread / 12.0).clamp(0.0, 1.0) * 0.18;
    let latency = (intent.data_latency_ms as f64 / 250.0).clamp(0.0, 1.0) * 0.18;
    let taker_penalty = if intent.request.order_type == OrderType::Market {
        0.08
    } else {
        0.0
    };
    let regime_pressure = match intent.context.regime {
        RegimeKind::NewsShock => 0.35,
        RegimeKind::TrendExpansion => 0.14,
        RegimeKind::HighVolatility => 0.10,
        RegimeKind::LowLiquidity => 0.18,
        RegimeKind::Normal => 0.0,
    };
    (urgency_crowding + short_duration + spread_signal + latency + taker_penalty + regime_pressure)
        .clamp(0.0, 1.0)
}
