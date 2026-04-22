use crate::types::{FillEvent, OrderIntent, SymbolProfile};

#[derive(Clone, Debug)]
pub struct SymbolProfileEngine {
    profile: SymbolProfile,
}

impl SymbolProfileEngine {
    pub fn new(symbol: String) -> Self {
        Self {
            profile: SymbolProfile {
                symbol,
                ..SymbolProfile::default()
            },
        }
    }

    #[inline]
    pub fn profile(&self) -> &SymbolProfile {
        &self.profile
    }

    #[inline]
    pub fn observe_intent(&mut self, intent: &OrderIntent) {
        self.profile.avg_spread_bps = ema(
            self.profile.avg_spread_bps,
            intent.regime.spread.max(0.1),
            0.02,
        );
        self.profile.volatility_ema = ema(
            self.profile.volatility_ema,
            intent.regime.volatility.max(0.01),
            0.02,
        );
        self.profile.avg_trade_size = ema(
            self.profile.avg_trade_size,
            intent.request.size.max(0.0001),
            0.02,
        );
    }

    #[inline]
    pub fn observe_fill(&mut self, fill: &FillEvent) {
        let complete = if fill.complete { 1.0 } else { 0.0 };
        self.profile.avg_fill_probability = ema(self.profile.avg_fill_probability, complete, 0.06);
        self.profile.avg_trade_size = ema(
            self.profile.avg_trade_size,
            fill.filled_size.max(0.0001),
            0.03,
        );
    }
}

#[inline]
fn ema(previous: f64, value: f64, alpha: f64) -> f64 {
    previous * (1.0 - alpha) + value * alpha
}
