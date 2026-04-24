use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ValidationMode {
    Shadow,
    Paper,
    Live,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatisticalTestResult {
    pub t_statistic: f64,
    pub ks_statistic: f64,
    pub passed: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ValidationReport {
    pub comparisons: HashMap<(ValidationMode, ValidationMode), StatisticalTestResult>,
    pub accepted: bool,
}

pub fn validate_modes(
    shadow: &[f64],
    paper: &[f64],
    live: &[f64],
    t_threshold: f64,
    ks_threshold: f64,
) -> ValidationReport {
    let mut comparisons = HashMap::new();
    let pairs = [
        ((ValidationMode::Shadow, ValidationMode::Paper), shadow, paper),
        ((ValidationMode::Shadow, ValidationMode::Live), shadow, live),
        ((ValidationMode::Paper, ValidationMode::Live), paper, live),
    ];
    let mut accepted = true;
    for ((lhs, rhs), a, b) in pairs {
        let result = StatisticalTestResult {
            t_statistic: welch_t_statistic(a, b),
            ks_statistic: ks_statistic(a, b),
            passed: true,
        };
        let passed =
            result.t_statistic.abs() <= t_threshold && result.ks_statistic <= ks_threshold;
        accepted &= passed;
        comparisons.insert(
            (lhs, rhs),
            StatisticalTestResult {
                passed,
                ..result
            },
        );
    }
    ValidationReport {
        comparisons,
        accepted,
    }
}

fn welch_t_statistic(a: &[f64], b: &[f64]) -> f64 {
    if a.len() < 2 || b.len() < 2 {
        return f64::INFINITY;
    }
    let mean_a = mean(a);
    let mean_b = mean(b);
    let var_a = sample_variance(a, mean_a);
    let var_b = sample_variance(b, mean_b);
    let denom = (var_a / a.len() as f64 + var_b / b.len() as f64).sqrt();
    if denom <= f64::EPSILON {
        0.0
    } else {
        (mean_a - mean_b) / denom
    }
}

fn ks_statistic(a: &[f64], b: &[f64]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 1.0;
    }
    let mut lhs = a.to_vec();
    let mut rhs = b.to_vec();
    lhs.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    rhs.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));

    let mut i = 0usize;
    let mut j = 0usize;
    let mut cdf_a = 0.0;
    let mut cdf_b = 0.0;
    let mut max_distance: f64 = 0.0;

    while i < lhs.len() && j < rhs.len() {
        if lhs[i] <= rhs[j] {
            i += 1;
            cdf_a = i as f64 / lhs.len() as f64;
        } else {
            j += 1;
            cdf_b = j as f64 / rhs.len() as f64;
        }
        max_distance = max_distance.max((cdf_a - cdf_b).abs());
    }
    max_distance
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn sample_variance(values: &[f64], mean: f64) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    values
        .iter()
        .map(|value| {
            let delta = *value - mean;
            delta * delta
        })
        .sum::<f64>()
        / (values.len() - 1) as f64
}
