use crate::accounting::latency::LatencyDistributionSnapshot;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DistributionMoments {
    pub mean: f64,
    pub variance: f64,
    pub skew: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DecisionQualityReport {
    pub expected_markout: f64,
    pub realized_markout: f64,
    pub edge_error: f64,
    pub edge_error_distribution: DistributionMoments,
    pub edge_reliability_score: f64,
    pub trading_enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExecutionQualityReport {
    pub fill_rate: f64,
    pub slippage: f64,
    pub outbid_rate: f64,
    pub latency_distribution: LatencyDistributionSnapshot,
}

#[derive(Clone, Debug, Default)]
pub struct EdgeReliabilityModel {
    count: u64,
    mean: f64,
    m2: f64,
    m3: f64,
}

impl EdgeReliabilityModel {
    pub fn observe(&mut self, expected_markout: f64, realized_markout: f64) -> DecisionQualityReport {
        let edge_error = expected_markout - realized_markout;
        self.update(edge_error);
        let moments = self.moments();
        let reliability = reliability_score(moments);
        DecisionQualityReport {
            expected_markout,
            realized_markout,
            edge_error,
            edge_error_distribution: moments,
            edge_reliability_score: reliability,
            trading_enabled: reliability >= 0.35,
        }
    }

    pub fn moments(&self) -> DistributionMoments {
        if self.count < 2 {
            return DistributionMoments {
                mean: self.mean,
                variance: 0.0,
                skew: 0.0,
            };
        }
        let variance = self.m2 / (self.count - 1) as f64;
        let skew = if self.count < 3 || self.m2.abs() <= f64::EPSILON {
            0.0
        } else {
            (self.count as f64).sqrt() * self.m3 / self.m2.powf(1.5)
        };
        DistributionMoments {
            mean: self.mean,
            variance,
            skew,
        }
    }

    fn update(&mut self, value: f64) {
        let previous_count = self.count as f64;
        self.count = self.count.saturating_add(1);
        let count = self.count as f64;
        let delta = value - self.mean;
        let delta_n = delta / count;
        let term1 = delta * delta_n * previous_count;

        self.mean += delta_n;
        self.m3 += term1 * delta_n * (count - 2.0) - 3.0 * delta_n * self.m2;
        self.m2 += term1;
    }
}

pub fn execution_quality_report(
    fill_rate: f64,
    slippage: f64,
    outbid_rate: f64,
    latency_distribution: LatencyDistributionSnapshot,
) -> ExecutionQualityReport {
    ExecutionQualityReport {
        fill_rate: fill_rate.clamp(0.0, 1.0),
        slippage,
        outbid_rate: outbid_rate.clamp(0.0, 1.0),
        latency_distribution,
    }
}

fn reliability_score(moments: DistributionMoments) -> f64 {
    let mean_penalty = (moments.mean.abs() / 10.0).clamp(0.0, 1.0);
    let variance_penalty = (moments.variance / 25.0).clamp(0.0, 1.0);
    let skew_penalty = (moments.skew.abs() / 4.0).clamp(0.0, 1.0);
    (1.0 - (0.45 * mean_penalty + 0.40 * variance_penalty + 0.15 * skew_penalty)).clamp(0.0, 1.0)
}
