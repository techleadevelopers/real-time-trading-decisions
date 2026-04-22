use crate::types::{Features, MarketContext, OrderBookState, RegimeKind, TapeState};

#[derive(Clone, Debug)]
pub struct ContextEngine {
    baseline_depth: f64,
    previous_imbalance: f64,
    initialized: bool,
}

impl Default for ContextEngine {
    fn default() -> Self {
        Self {
            baseline_depth: 1.0,
            previous_imbalance: 0.0,
            initialized: false,
        }
    }
}

impl ContextEngine {
    pub fn update(
        &mut self,
        features: &Features,
        book: &OrderBookState,
        tape: &TapeState,
    ) -> MarketContext {
        let depth = (book.bid_volume + book.ask_volume).max(0.0);
        if !self.initialized {
            self.baseline_depth = depth.max(1.0);
            self.previous_imbalance = book.weighted_imbalance;
            self.initialized = true;
        }

        self.baseline_depth = 0.985 * self.baseline_depth + 0.015 * depth.max(1.0);
        let depth_ratio = (depth / self.baseline_depth.max(1.0)).clamp(0.0, 2.0);
        let liquidity_score = (0.65 * depth_ratio.min(1.0)
            + 0.20 * (1.0 - book.liquidity_pull)
            + 0.15 * (1.0 - (book.spread * 10_000.0 / 20.0).clamp(0.0, 1.0)))
        .clamp(0.0, 1.0);

        let imbalance_shift = (book.weighted_imbalance - self.previous_imbalance).abs();
        self.previous_imbalance = book.weighted_imbalance;

        let volatility = features
            .volatility
            .abs()
            .max(features.micro_price_velocity.abs() * 0.25);
        let spread_widening = features.spread_dynamics.max(0.0).clamp(0.0, 5.0) / 5.0;
        let velocity_burst = (tape.trade_frequency / 80.0).clamp(0.0, 1.0);
        let depth_collapse = (1.0 - depth_ratio).clamp(0.0, 1.0);
        let instability = (0.30 * (volatility / 5.0).clamp(0.0, 1.0)
            + 0.22 * spread_widening
            + 0.22 * depth_collapse
            + 0.16 * velocity_burst
            + 0.10 * imbalance_shift.clamp(0.0, 1.0))
        .clamp(0.0, 1.0);
        let stability_score = 1.0 - instability;

        let regime = classify_regime(
            volatility,
            book.spread * 10_000.0,
            depth_ratio,
            tape.trade_frequency,
            imbalance_shift,
            features,
            stability_score,
        );

        MarketContext {
            regime,
            volatility,
            liquidity_score,
            stability_score,
        }
    }
}

fn classify_regime(
    volatility: f64,
    spread_bps: f64,
    depth_ratio: f64,
    trade_frequency: f64,
    imbalance_shift: f64,
    features: &Features,
    stability_score: f64,
) -> RegimeKind {
    let shock = volatility > 3.8
        && spread_bps > 14.0
        && depth_ratio < 0.45
        && (trade_frequency > 70.0 || imbalance_shift > 0.65);
    if shock || stability_score < 0.22 {
        return RegimeKind::NewsShock;
    }
    if depth_ratio < 0.38 || spread_bps > 18.0 || features.liquidity_pull > 0.70 {
        return RegimeKind::LowLiquidity;
    }
    if volatility > 3.0 || trade_frequency > 90.0 {
        return RegimeKind::HighVolatility;
    }
    if features.micro_price_velocity.abs() > 1.25
        && features.order_flow_delta.signum() == features.micro_price_velocity.signum()
        && features.weighted_imbalance.signum() == features.micro_price_velocity.signum()
        && spread_bps < 10.0
        && depth_ratio > 0.55
    {
        return RegimeKind::TrendExpansion;
    }
    RegimeKind::Normal
}
