use crate::types::Features;

#[derive(Clone, Debug)]
pub struct LogisticFilter {
    continuation_weights: [f64; 6],
    reversal_weights: [f64; 6],
}

impl Default for LogisticFilter {
    fn default() -> Self {
        Self {
            continuation_weights: [-0.15, 1.35, 0.45, 0.95, -0.30, -0.65],
            reversal_weights: [-0.25, -1.05, 0.35, -0.85, 0.55, 0.25],
        }
    }
}

impl LogisticFilter {
    #[inline]
    pub fn probabilities(&self, features: &Features) -> (f64, f64) {
        (
            sigmoid(dot(self.continuation_weights, features)),
            sigmoid(dot(self.reversal_weights, features)),
        )
    }
}

#[inline]
fn dot(weights: [f64; 6], features: &Features) -> f64 {
    weights[0]
        + weights[1] * features.velocity
        + weights[2] * features.vol_z
        + weights[3] * features.imbalance
        + weights[4] * features.volatility
        + weights[5] * features.spread
}

#[inline]
fn sigmoid(x: f64) -> f64 {
    if x >= 0.0 {
        1.0 / (1.0 + (-x).exp())
    } else {
        let z = x.exp();
        z / (1.0 + z)
    }
}

