use crate::{
    execution_controller::ExecutionFailureReason,
    types::{CompetitionState, LearningSample, MarketRegime},
};
use std::collections::{HashMap, VecDeque};

const DEFAULT_WINDOW: usize = 128;
const MIN_VALIDATION_SAMPLES: usize = 24;
const NOISE_FLOOR: f64 = 1e-4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeState {
    Valid,
    #[default]
    Uncertain,
    Invalid,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeRegime {
    #[default]
    Stable,
    Decaying,
    Unstable,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdgeValidationSnapshot {
    pub edge_state: EdgeState,
    pub edge_regime: EdgeRegime,
    pub competition_state: CompetitionState,
    pub t_statistic: f64,
    pub ks_statistic: f64,
    pub edge_reliability_score: f64,
    pub sharpe_like: f64,
    pub edge_half_life_samples: f64,
    pub edge_error_mean: f64,
    pub edge_error_variance: f64,
    pub edge_error_skew: f64,
    pub edge_capture_mean: f64,
    pub edge_capture_variance: f64,
    pub negative_capture_streak: usize,
    pub execution_alpha_mean: f64,
    pub execution_alpha_variance: f64,
    pub confidence_interval_low: f64,
    pub confidence_interval_high: f64,
    pub competition_score: f64,
    pub edge_component_mean: f64,
    pub execution_loss_mean: f64,
    pub fees_rebates_mean: f64,
    pub adverse_selection_loss_mean: f64,
    pub regime_reliability: f64,
    pub regime_pnl_mean: f64,
    pub regime_execution_quality: f64,
    pub sample_count: usize,
    pub position_size_multiplier: f64,
    pub trading_enabled: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct RegimeMemory {
    reliability_ewma: f64,
    pnl_ewma: f64,
    execution_quality_ewma: f64,
    capture_ewma: f64,
    sample_count: usize,
}

impl RegimeMemory {
    fn update(&mut self, reliability: f64, pnl: f64, execution_quality: f64, capture: f64) {
        const ALPHA: f64 = 0.08;
        if self.sample_count == 0 {
            self.reliability_ewma = reliability;
            self.pnl_ewma = pnl;
            self.execution_quality_ewma = execution_quality;
            self.capture_ewma = capture;
        } else {
            self.reliability_ewma = ewma(self.reliability_ewma, reliability, ALPHA);
            self.pnl_ewma = ewma(self.pnl_ewma, pnl, ALPHA);
            self.execution_quality_ewma =
                ewma(self.execution_quality_ewma, execution_quality, ALPHA);
            self.capture_ewma = ewma(self.capture_ewma, capture, ALPHA);
        }
        self.sample_count = self.sample_count.saturating_add(1);
    }
}

#[derive(Clone, Debug)]
pub struct EdgeValidationEngine {
    capacity: usize,
    min_samples: usize,
    expected_edges: VecDeque<f64>,
    realized_markouts: VecDeque<f64>,
    realized_pnls: VecDeque<f64>,
    adjusted_returns: VecDeque<f64>,
    edge_errors: VecDeque<f64>,
    capture_ratios: VecDeque<f64>,
    execution_alphas: VecDeque<f64>,
    fill_rates: VecDeque<f64>,
    slippages: VecDeque<f64>,
    outbid_flags: VecDeque<f64>,
    edge_components: VecDeque<f64>,
    execution_losses: VecDeque<f64>,
    fees_rebates: VecDeque<f64>,
    adverse_selection_losses: VecDeque<f64>,
    competition_states: VecDeque<CompetitionState>,
    negative_capture_streak: usize,
    regime_memory: HashMap<&'static str, RegimeMemory>,
    failure_ema: f64,
}

impl Default for EdgeValidationEngine {
    fn default() -> Self {
        Self::new(DEFAULT_WINDOW)
    }
}

impl EdgeValidationEngine {
    pub fn new(window: usize) -> Self {
        let capacity = window.max(32);
        Self {
            capacity,
            min_samples: MIN_VALIDATION_SAMPLES,
            expected_edges: VecDeque::with_capacity(capacity),
            realized_markouts: VecDeque::with_capacity(capacity),
            realized_pnls: VecDeque::with_capacity(capacity),
            adjusted_returns: VecDeque::with_capacity(capacity),
            edge_errors: VecDeque::with_capacity(capacity),
            capture_ratios: VecDeque::with_capacity(capacity),
            execution_alphas: VecDeque::with_capacity(capacity),
            fill_rates: VecDeque::with_capacity(capacity),
            slippages: VecDeque::with_capacity(capacity),
            outbid_flags: VecDeque::with_capacity(capacity),
            edge_components: VecDeque::with_capacity(capacity),
            execution_losses: VecDeque::with_capacity(capacity),
            fees_rebates: VecDeque::with_capacity(capacity),
            adverse_selection_losses: VecDeque::with_capacity(capacity),
            competition_states: VecDeque::with_capacity(capacity),
            negative_capture_streak: 0,
            regime_memory: HashMap::new(),
            failure_ema: 0.0,
        }
    }

    pub fn observe_failure(&mut self, reason: ExecutionFailureReason) {
        let severity = match reason {
            ExecutionFailureReason::QueueTooDeep => 0.35,
            ExecutionFailureReason::Outbid => 0.55,
            ExecutionFailureReason::LatencyTooHigh => 0.65,
            ExecutionFailureReason::CompetitionSpike => 0.80,
            ExecutionFailureReason::LiquidityPull => 0.70,
            ExecutionFailureReason::NoFillTimeout => 0.75,
        };
        self.failure_ema = ewma(self.failure_ema, severity, 0.18);
    }

    pub fn observe(&mut self, sample: &LearningSample, drawdown_pct: f64) -> EdgeValidationSnapshot {
        let expected_edge = filter_noise(sample.expected_markout);
        let realized_markout = filter_noise(sample.realized_markout);
        let realized_pnl = filter_noise(sample.pnl);
        let edge_error = expected_edge - realized_markout;
        let capture_ratio = if expected_edge.abs() > NOISE_FLOOR {
            (realized_pnl / expected_edge).clamp(-5.0, 5.0)
        } else {
            0.0
        };
        let adjusted_return = realized_pnl
            - sample.actual_slippage_bps.abs() * 0.20
            - sample.duration_ms as f64 * 0.0005;

        push_capped(&mut self.expected_edges, expected_edge, self.capacity);
        push_capped(&mut self.realized_markouts, realized_markout, self.capacity);
        push_capped(&mut self.realized_pnls, realized_pnl, self.capacity);
        push_capped(&mut self.adjusted_returns, adjusted_return, self.capacity);
        push_capped(&mut self.edge_errors, edge_error, self.capacity);
        push_capped(&mut self.capture_ratios, capture_ratio, self.capacity);
        push_capped(
            &mut self.execution_alphas,
            filter_noise(sample.execution_alpha),
            self.capacity,
        );
        push_capped(&mut self.fill_rates, sample.fill_ratio.clamp(0.0, 1.0), self.capacity);
        push_capped(
            &mut self.slippages,
            sample.actual_slippage_bps.max(0.0),
            self.capacity,
        );
        push_capped(
            &mut self.outbid_flags,
            if matches!(sample.competition_state, CompetitionState::Competitive | CompetitionState::Saturated) {
                1.0
            } else {
                0.0
            },
            self.capacity,
        );
        push_capped(
            &mut self.edge_components,
            sample.edge_component,
            self.capacity,
        );
        push_capped(
            &mut self.execution_losses,
            sample.execution_loss.max(0.0),
            self.capacity,
        );
        push_capped(
            &mut self.fees_rebates,
            sample.fees_rebates_component,
            self.capacity,
        );
        push_capped(
            &mut self.adverse_selection_losses,
            sample.adverse_selection_loss.max(0.0),
            self.capacity,
        );
        push_state_capped(
            &mut self.competition_states,
            sample.competition_state,
            self.capacity,
        );

        if capture_ratio < -0.05 {
            self.negative_capture_streak = self.negative_capture_streak.saturating_add(1);
        } else {
            self.negative_capture_streak = self.negative_capture_streak.saturating_sub(1);
        }

        let moments = rolling_moments(&self.edge_errors);
        let capture_moments = rolling_moments(&self.capture_ratios);
        let execution_alpha_moments = rolling_moments(&self.execution_alphas);
        let sharpe_like = rolling_sharpe_like(&self.adjusted_returns);
        let t_statistic = one_sample_t_statistic(&self.realized_pnls);
        let (confidence_interval_low, confidence_interval_high) =
            confidence_interval(&self.realized_pnls);
        let ks_statistic = ks_statistic(&self.expected_edges, &self.realized_markouts);
        let competition_score = competition_score(
            &self.expected_edges,
            &self.fill_rates,
            &self.slippages,
            &self.outbid_flags,
        );
        let competition_state =
            classify_competition(competition_score, &self.fill_rates, self.negative_capture_streak);
        let reliability = reliability_score(
            moments.mean,
            moments.variance,
            moments.skew,
            sharpe_like,
            capture_moments.mean + execution_alpha_moments.mean.signum() * execution_alpha_moments.mean.abs().min(1.0) * 0.15,
            competition_score,
            self.failure_ema,
        );
        let edge_regime = classify_regime(
            reliability,
            estimate_half_life(&self.expected_edges, &self.realized_markouts),
            ks_statistic,
            capture_moments.mean,
        );
        let regime_key = regime_bucket(&sample.regime);
        let regime_memory = self.regime_memory.entry(regime_key).or_default();
        let execution_quality =
            execution_quality(sample.fill_ratio, sample.actual_slippage_bps, competition_score);
        regime_memory.update(reliability, realized_pnl, execution_quality, capture_ratio);

        let edge_state = classify_state(
            self.realized_pnls.len(),
            self.min_samples,
            t_statistic,
            ks_statistic,
            moments.mean,
            confidence_interval_low,
            capture_moments.mean,
            self.negative_capture_streak,
            reliability,
        );
        let trading_enabled =
            edge_state != EdgeState::Invalid && competition_state != CompetitionState::Saturated;
        let position_size_multiplier = position_size_multiplier(
            reliability,
            edge_state,
            edge_regime,
            competition_state,
            drawdown_pct,
            regime_memory,
        );

        EdgeValidationSnapshot {
            edge_state,
            edge_regime,
            competition_state,
            t_statistic,
            ks_statistic,
            edge_reliability_score: reliability,
            sharpe_like,
            edge_half_life_samples: estimate_half_life(
                &self.expected_edges,
                &self.realized_markouts,
            ),
            edge_error_mean: moments.mean,
            edge_error_variance: moments.variance,
            edge_error_skew: moments.skew,
            edge_capture_mean: capture_moments.mean,
            edge_capture_variance: capture_moments.variance,
            negative_capture_streak: self.negative_capture_streak,
            execution_alpha_mean: execution_alpha_moments.mean,
            execution_alpha_variance: execution_alpha_moments.variance,
            confidence_interval_low,
            confidence_interval_high,
            competition_score,
            edge_component_mean: mean(&self.edge_components),
            execution_loss_mean: mean(&self.execution_losses),
            fees_rebates_mean: mean(&self.fees_rebates),
            adverse_selection_loss_mean: mean(&self.adverse_selection_losses),
            regime_reliability: regime_memory.reliability_ewma,
            regime_pnl_mean: regime_memory.pnl_ewma,
            regime_execution_quality: regime_memory.execution_quality_ewma,
            sample_count: self.realized_pnls.len(),
            position_size_multiplier,
            trading_enabled,
        }
    }

    pub fn snapshot(&self, drawdown_pct: f64) -> EdgeValidationSnapshot {
        let moments = rolling_moments(&self.edge_errors);
        let capture_moments = rolling_moments(&self.capture_ratios);
        let execution_alpha_moments = rolling_moments(&self.execution_alphas);
        let sharpe_like = rolling_sharpe_like(&self.adjusted_returns);
        let t_statistic = one_sample_t_statistic(&self.realized_pnls);
        let (confidence_interval_low, confidence_interval_high) =
            confidence_interval(&self.realized_pnls);
        let ks_statistic = ks_statistic(&self.expected_edges, &self.realized_markouts);
        let competition_score = competition_score(
            &self.expected_edges,
            &self.fill_rates,
            &self.slippages,
            &self.outbid_flags,
        );
        let competition_state =
            classify_competition(competition_score, &self.fill_rates, self.negative_capture_streak);
        let reliability = reliability_score(
            moments.mean,
            moments.variance,
            moments.skew,
            sharpe_like,
            capture_moments.mean + execution_alpha_moments.mean.signum() * execution_alpha_moments.mean.abs().min(1.0) * 0.15,
            competition_score,
            self.failure_ema,
        );
        let edge_half_life_samples =
            estimate_half_life(&self.expected_edges, &self.realized_markouts);
        let edge_regime = classify_regime(
            reliability,
            edge_half_life_samples,
            ks_statistic,
            capture_moments.mean,
        );
        let regime_memory = self
            .regime_memory
            .values()
            .max_by_key(|memory| memory.sample_count)
            .copied()
            .unwrap_or_default();
        let edge_state = classify_state(
            self.realized_pnls.len(),
            self.min_samples,
            t_statistic,
            ks_statistic,
            moments.mean,
            confidence_interval_low,
            capture_moments.mean,
            self.negative_capture_streak,
            reliability,
        );
        let trading_enabled =
            edge_state != EdgeState::Invalid && competition_state != CompetitionState::Saturated;

        EdgeValidationSnapshot {
            edge_state,
            edge_regime,
            competition_state,
            t_statistic,
            ks_statistic,
            edge_reliability_score: reliability,
            sharpe_like,
            edge_half_life_samples,
            edge_error_mean: moments.mean,
            edge_error_variance: moments.variance,
            edge_error_skew: moments.skew,
            edge_capture_mean: capture_moments.mean,
            edge_capture_variance: capture_moments.variance,
            negative_capture_streak: self.negative_capture_streak,
            execution_alpha_mean: execution_alpha_moments.mean,
            execution_alpha_variance: execution_alpha_moments.variance,
            confidence_interval_low,
            confidence_interval_high,
            competition_score,
            edge_component_mean: mean(&self.edge_components),
            execution_loss_mean: mean(&self.execution_losses),
            fees_rebates_mean: mean(&self.fees_rebates),
            adverse_selection_loss_mean: mean(&self.adverse_selection_losses),
            regime_reliability: regime_memory.reliability_ewma,
            regime_pnl_mean: regime_memory.pnl_ewma,
            regime_execution_quality: regime_memory.execution_quality_ewma,
            sample_count: self.realized_pnls.len(),
            position_size_multiplier: position_size_multiplier(
                reliability,
                edge_state,
                edge_regime,
                competition_state,
                drawdown_pct,
                &regime_memory,
            ),
            trading_enabled,
        }
    }
}

pub fn dynamic_position_size_multiplier(
    edge_reliability_score: f64,
    edge_state: EdgeState,
    edge_regime: EdgeRegime,
    drawdown_pct: f64,
) -> f64 {
    let state_factor = match edge_state {
        EdgeState::Valid => 1.0,
        EdgeState::Uncertain => 0.65,
        EdgeState::Invalid => 0.0,
    };
    let regime_factor = match edge_regime {
        EdgeRegime::Stable => 1.0,
        EdgeRegime::Decaying => 0.75,
        EdgeRegime::Unstable => 0.40,
    };
    let drawdown_factor = (1.0 - drawdown_pct * 1.35).clamp(0.0, 1.0);
    (edge_reliability_score.clamp(0.0, 1.0) * state_factor * regime_factor * drawdown_factor)
        .clamp(0.0, 1.0)
}

fn position_size_multiplier(
    reliability: f64,
    state: EdgeState,
    regime: EdgeRegime,
    competition_state: CompetitionState,
    drawdown_pct: f64,
    regime_memory: &RegimeMemory,
) -> f64 {
    let competition_factor = match competition_state {
        CompetitionState::Normal => 1.0,
        CompetitionState::Competitive => 0.65,
        CompetitionState::Saturated => 0.0,
    };
    let regime_memory_factor = if regime_memory.sample_count >= 8 {
        (0.65
            + regime_memory.reliability_ewma * 0.20
            + regime_memory.execution_quality_ewma * 0.10
            + regime_memory.capture_ewma.max(0.0) * 0.05)
            .clamp(0.35, 1.10)
    } else {
        0.85
    };
    (dynamic_position_size_multiplier(reliability, state, regime, drawdown_pct)
        * competition_factor
        * regime_memory_factor)
        .clamp(0.0, 1.25)
}

fn classify_state(
    sample_count: usize,
    min_samples: usize,
    t_statistic: f64,
    ks_statistic: f64,
    edge_error_mean: f64,
    confidence_interval_low: f64,
    capture_mean: f64,
    negative_capture_streak: usize,
    reliability: f64,
) -> EdgeState {
    if sample_count < min_samples {
        return EdgeState::Uncertain;
    }
    if negative_capture_streak >= 6
        || capture_mean < -0.10
        || confidence_interval_low < 0.0 && t_statistic < 0.0
        || (ks_statistic > 0.72
            && capture_mean < 0.05
            && reliability < 0.50
            && edge_error_mean.abs() > 0.25)
        || reliability < 0.28
    {
        return EdgeState::Invalid;
    }
    if t_statistic > 1.30
        && confidence_interval_low > 0.0
        && (ks_statistic < 0.45
            || (capture_mean > 0.75 && edge_error_mean.abs() < 0.25 && reliability > 0.75))
        && capture_mean > 0.15
        && reliability > 0.55
    {
        return EdgeState::Valid;
    }
    EdgeState::Uncertain
}

fn classify_regime(
    reliability: f64,
    half_life: f64,
    ks_statistic: f64,
    capture_mean: f64,
) -> EdgeRegime {
    if reliability < 0.35 || ks_statistic > 0.70 || capture_mean < 0.0 {
        EdgeRegime::Unstable
    } else if half_life < 16.0 || reliability < 0.60 || ks_statistic > 0.45 {
        EdgeRegime::Decaying
    } else {
        EdgeRegime::Stable
    }
}

fn classify_competition(
    score: f64,
    fill_rates: &VecDeque<f64>,
    negative_capture_streak: usize,
) -> CompetitionState {
    let fill_mean = mean(fill_rates);
    if score >= 0.75 || fill_mean < 0.35 || negative_capture_streak >= 8 {
        CompetitionState::Saturated
    } else if score >= 0.45 || fill_mean < 0.60 {
        CompetitionState::Competitive
    } else {
        CompetitionState::Normal
    }
}

fn competition_score(
    expected_edges: &VecDeque<f64>,
    fill_rates: &VecDeque<f64>,
    slippages: &VecDeque<f64>,
    outbid_flags: &VecDeque<f64>,
) -> f64 {
    let edge_mean = mean(expected_edges).max(0.0);
    let stable_signal = coefficient_of_variation(expected_edges) < 1.0;
    let high_edge_pressure = (edge_mean / 4.0).clamp(0.0, 1.0);
    let fill_penalty = (1.0 - mean(fill_rates)).clamp(0.0, 1.0);
    let outbid_penalty = mean(outbid_flags).clamp(0.0, 1.0);
    let stable_slippage_penalty = if stable_signal {
        (mean(slippages) / 12.0).clamp(0.0, 1.0)
    } else {
        (mean(slippages) / 20.0).clamp(0.0, 1.0)
    };
    (0.25 * high_edge_pressure
        + 0.30 * fill_penalty
        + 0.25 * outbid_penalty
        + 0.20 * stable_slippage_penalty)
        .clamp(0.0, 1.0)
}

fn rolling_sharpe_like(values: &VecDeque<f64>) -> f64 {
    let moments = rolling_moments(values);
    if moments.variance <= f64::EPSILON {
        return 0.0;
    }
    moments.mean / moments.variance.sqrt()
}

#[derive(Clone, Copy, Debug, Default)]
struct Moments {
    mean: f64,
    variance: f64,
    skew: f64,
}

fn rolling_moments(values: &VecDeque<f64>) -> Moments {
    if values.is_empty() {
        return Moments::default();
    }
    let n = values.len() as f64;
    let mean = mean(values);
    let mut m2 = 0.0;
    let mut m3 = 0.0;
    for value in values {
        let delta = *value - mean;
        m2 += delta * delta;
        m3 += delta * delta * delta;
    }
    let variance = if values.len() > 1 { m2 / (n - 1.0) } else { 0.0 };
    let skew = if variance <= f64::EPSILON {
        0.0
    } else {
        (m3 / n) / variance.sqrt().powi(3)
    };
    Moments {
        mean,
        variance,
        skew,
    }
}

fn reliability_score(
    edge_error_mean: f64,
    edge_error_variance: f64,
    edge_error_skew: f64,
    sharpe_like: f64,
    capture_mean: f64,
    competition_score: f64,
    failure_ema: f64,
) -> f64 {
    let mean_penalty = (edge_error_mean.abs() / 10.0).clamp(0.0, 1.0);
    let variance_penalty = (edge_error_variance / 25.0).clamp(0.0, 1.0);
    let skew_penalty = (edge_error_skew.abs() / 4.0).clamp(0.0, 1.0);
    let sharpe_bonus = ((sharpe_like + 1.0) / 3.0).clamp(0.0, 1.0);
    let capture_bonus = ((capture_mean + 0.5) / 1.5).clamp(0.0, 1.0);
    let competition_penalty = competition_score.clamp(0.0, 1.0);
    (0.40 * (1.0 - mean_penalty)
        + 0.18 * (1.0 - variance_penalty)
        + 0.07 * (1.0 - skew_penalty)
        + 0.20 * sharpe_bonus
        + 0.10 * capture_bonus
        - 0.15 * competition_penalty)
        .mul_add(1.0 - failure_ema.clamp(0.0, 0.95) * 0.35, 0.0)
        .clamp(0.0, 1.0)
}

fn one_sample_t_statistic(values: &VecDeque<f64>) -> f64 {
    let moments = rolling_moments(values);
    if values.len() < 2 {
        return 0.0;
    }
    if moments.variance <= f64::EPSILON {
        return if moments.mean > 0.0 {
            10.0
        } else if moments.mean < 0.0 {
            -10.0
        } else {
            0.0
        };
    }
    moments.mean / (moments.variance.sqrt() / (values.len() as f64).sqrt())
}

fn confidence_interval(values: &VecDeque<f64>) -> (f64, f64) {
    let moments = rolling_moments(values);
    if values.is_empty() {
        return (0.0, 0.0);
    }
    let margin = 1.96 * moments.variance.sqrt() / (values.len() as f64).sqrt();
    (moments.mean - margin, moments.mean + margin)
}

fn estimate_half_life(expected: &VecDeque<f64>, realized: &VecDeque<f64>) -> f64 {
    if expected.is_empty() || realized.is_empty() {
        return 0.0;
    }
    let ratio = if mean(expected).abs() > NOISE_FLOOR {
        (mean(realized) / mean(expected)).abs()
    } else {
        0.0
    };
    if ratio <= f64::EPSILON {
        return 0.0;
    }
    let decay = (1.0 - ratio.clamp(0.0, 0.999)).max(0.001);
    (std::f64::consts::LN_2 / decay).clamp(1.0, expected.len() as f64 * 2.0)
}

fn ks_statistic(expected: &VecDeque<f64>, realized: &VecDeque<f64>) -> f64 {
    if expected.is_empty() || realized.is_empty() {
        return 0.0;
    }
    let mut expected_sorted: Vec<f64> = expected.iter().copied().collect();
    let mut realized_sorted: Vec<f64> = realized.iter().copied().collect();
    expected_sorted.sort_by(f64::total_cmp);
    realized_sorted.sort_by(f64::total_cmp);
    let mut i = 0;
    let mut j = 0;
    let mut max_distance: f64 = 0.0;
    while i < expected_sorted.len() && j < realized_sorted.len() {
        let value = expected_sorted[i].min(realized_sorted[j]);
        while i < expected_sorted.len() && expected_sorted[i] <= value {
            i += 1;
        }
        while j < realized_sorted.len() && realized_sorted[j] <= value {
            j += 1;
        }
        let cdf_expected = i as f64 / expected_sorted.len() as f64;
        let cdf_realized = j as f64 / realized_sorted.len() as f64;
        max_distance = max_distance.max((cdf_expected - cdf_realized).abs());
    }
    max_distance
}

fn execution_quality(fill_rate: f64, slippage_bps: f64, competition_score: f64) -> f64 {
    (0.50 * fill_rate.clamp(0.0, 1.0)
        + 0.30 * (1.0 - (slippage_bps.abs() / 15.0).clamp(0.0, 1.0))
        + 0.20 * (1.0 - competition_score.clamp(0.0, 1.0)))
        .clamp(0.0, 1.0)
}

fn regime_bucket(regime: &MarketRegime) -> &'static str {
    if regime.spread > 10.0 {
        "wide_spread"
    } else if regime.volatility > 2.5 {
        "high_vol"
    } else if regime.trend_strength > 1.5 {
        "trending"
    } else {
        "normal"
    }
}

fn coefficient_of_variation(values: &VecDeque<f64>) -> f64 {
    let moments = rolling_moments(values);
    if moments.mean.abs() <= NOISE_FLOOR {
        return f64::INFINITY;
    }
    moments.variance.sqrt() / moments.mean.abs()
}

fn mean(values: &VecDeque<f64>) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn push_capped(queue: &mut VecDeque<f64>, value: f64, capacity: usize) {
    if queue.len() == capacity {
        queue.pop_front();
    }
    queue.push_back(value);
}

fn push_state_capped(queue: &mut VecDeque<CompetitionState>, value: CompetitionState, capacity: usize) {
    if queue.len() == capacity {
        queue.pop_front();
    }
    queue.push_back(value);
}

fn ewma(current: f64, value: f64, alpha: f64) -> f64 {
    current * (1.0 - alpha) + value * alpha
}

fn filter_noise(value: f64) -> f64 {
    if value.abs() <= NOISE_FLOOR {
        0.0
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CompetitionState, Direction, MarketRegime};

    fn sample(
        pnl: f64,
        expected_markout: f64,
        realized_markout: f64,
        fill_ratio: f64,
        slippage: f64,
        competition_state: CompetitionState,
    ) -> LearningSample {
        LearningSample {
            timestamp: 1,
            direction: Direction::Long,
            confidence: 0.7,
            predicted_score: 0.7,
            expected_slippage_bps: 1.0,
            actual_slippage_bps: slippage,
            pnl,
            expected_markout,
            realized_markout,
            execution_alpha: pnl - expected_markout,
            fill_ratio,
            fees_paid: 0.1,
            rebates_received: 0.0,
            funding_cost: 0.0,
            edge_component: expected_markout.max(0.0),
            execution_loss: (expected_markout - pnl).max(0.0),
            fees_rebates_component: -0.1,
            adverse_selection_loss: (-realized_markout).max(0.0),
            edge_capture_ratio: if expected_markout.abs() > 1e-9 {
                pnl / expected_markout
            } else {
                0.0
            },
            competition_state,
            duration_ms: 25,
            entry_quality: 0.8,
            markout_100ms: realized_markout * 0.5,
            markout_500ms: realized_markout,
            markout_1s: realized_markout,
            markout_5s: realized_markout,
            regime: MarketRegime::default(),
        }
    }

    #[test]
    fn edge_becomes_valid_with_consistent_capture() {
        let mut engine = EdgeValidationEngine::new(64);
        let mut snapshot = EdgeValidationSnapshot::default();
        for _ in 0..32 {
            snapshot = engine.observe(
                &sample(1.8, 2.0, 1.9, 0.92, 0.5, CompetitionState::Normal),
                0.0,
            );
        }
        assert_eq!(snapshot.edge_state, EdgeState::Valid);
        assert_eq!(snapshot.competition_state, CompetitionState::Normal);
        assert!(snapshot.edge_capture_mean > 0.5);
        assert!(snapshot.trading_enabled);
    }

    #[test]
    fn persistent_negative_capture_invalidates_edge() {
        let mut engine = EdgeValidationEngine::new(64);
        let mut snapshot = EdgeValidationSnapshot::default();
        for _ in 0..32 {
            snapshot = engine.observe(
                &sample(-1.0, 1.0, -0.8, 0.45, 8.0, CompetitionState::Competitive),
                0.05,
            );
        }
        assert_eq!(snapshot.edge_state, EdgeState::Invalid);
        assert!(snapshot.edge_capture_mean < 0.0);
    }

    #[test]
    fn competitive_conditions_cut_size_and_can_saturate() {
        let mut engine = EdgeValidationEngine::new(64);
        let mut snapshot = EdgeValidationSnapshot::default();
        for _ in 0..32 {
            snapshot = engine.observe(
                &sample(0.1, 2.0, 0.2, 0.20, 12.0, CompetitionState::Saturated),
                0.0,
            );
        }
        assert_eq!(snapshot.competition_state, CompetitionState::Saturated);
        assert_eq!(snapshot.position_size_multiplier, 0.0);
        assert!(!snapshot.trading_enabled);
    }
}
