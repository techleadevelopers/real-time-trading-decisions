use crate::types::{LearningSample, MarketRegime};

#[derive(Clone, Debug)]
pub struct AdaptiveWeights {
    pub velocity: f64,
    pub orderflow: f64,
    pub imbalance: f64,
    pub liquidity: f64,
    pub spread: f64,
    pub adversarial: f64,
}

impl Default for AdaptiveWeights {
    fn default() -> Self {
        Self {
            velocity: 1.10,
            orderflow: 0.95,
            imbalance: 0.85,
            liquidity: 0.65,
            spread: 0.75,
            adversarial: 1.15,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LearningState {
    weights: AdaptiveWeights,
    hit_rate: f64,
    slippage_error_bps: f64,
    threshold: f64,
    consecutive_losses: u32,
    samples: u64,
}

impl Default for LearningState {
    fn default() -> Self {
        Self {
            weights: AdaptiveWeights::default(),
            hit_rate: 0.50,
            slippage_error_bps: 0.0,
            threshold: 0.70,
            consecutive_losses: 0,
            samples: 0,
        }
    }
}

impl LearningState {
    pub fn weights(&self, regime: &MarketRegime) -> AdaptiveWeights {
        let mut weights = self.weights.clone();
        if regime.volatility > 2.0 {
            weights.velocity *= 0.85;
            weights.liquidity *= 1.20;
            weights.adversarial *= 1.25;
        }
        if regime.spread > 8.0 {
            weights.spread *= 1.50;
            weights.orderflow *= 0.85;
        }
        if self.hit_rate < 0.48 {
            weights.adversarial *= 1.20;
            weights.spread *= 1.15;
        }
        weights
    }

    pub fn threshold(&self, regime: &MarketRegime) -> f64 {
        let regime_penalty = if regime.spread > 10.0 || regime.volatility > 3.0 {
            0.05
        } else {
            0.0
        };
        (self.threshold + regime_penalty + self.slippage_error_bps.max(0.0) * 0.002)
            .clamp(0.62, 0.86)
    }

    pub fn hit_rate(&self) -> f64 {
        self.hit_rate
    }

    pub fn consecutive_losses(&self) -> u32 {
        self.consecutive_losses
    }

    pub fn apply_sample(&mut self, sample: LearningSample) {
        self.samples = self.samples.saturating_add(1);
        let won = sample.pnl > 0.0;
        let reward = if won { 1.0 } else { -1.0 };
        let alpha = 0.04;
        self.hit_rate = (1.0 - alpha) * self.hit_rate + alpha * f64::from(won);
        self.slippage_error_bps = (1.0 - alpha) * self.slippage_error_bps
            + alpha * (sample.actual_slippage_bps - sample.expected_slippage_bps);
        if won {
            self.consecutive_losses = 0;
            self.threshold = (self.threshold - 0.003).max(0.66);
        } else {
            self.consecutive_losses = self.consecutive_losses.saturating_add(1);
            self.threshold = (self.threshold + 0.012).min(0.86);
        }

        let size = 0.015 * reward * sample.confidence.clamp(0.1, 1.0);
        self.weights.velocity = adjust(self.weights.velocity, size);
        self.weights.orderflow = adjust(self.weights.orderflow, size * 0.8);
        self.weights.imbalance = adjust(self.weights.imbalance, size * 0.6);
        if sample.actual_slippage_bps > sample.expected_slippage_bps + 2.0 {
            self.weights.spread = adjust(self.weights.spread, 0.02);
            self.weights.liquidity = adjust(self.weights.liquidity, 0.015);
        }
        if sample.regime.volatility > 3.0 && !won {
            self.weights.adversarial = adjust(self.weights.adversarial, 0.025);
        }
    }
}

#[inline]
fn adjust(value: f64, delta: f64) -> f64 {
    (value + delta).clamp(0.25, 2.50)
}
