use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::Path,
    sync::{Arc, OnceLock},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeModelWeights {
    pub version: u32,
    pub generated_at: String,
    pub source: String,
    pub reversal_classifier: ReversalClassifierWeights,
    pub entry_scoring: EntryScoringWeights,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ReversalClassifierWeights {
    pub movement_drop_pct: f64,
    pub movement_velocity: f64,
    pub movement_volume_burst: f64,
    pub intent_exhaustion: f64,
    pub intent_weak_signal: f64,
    pub intent_absorption: f64,
    pub intent_timing: f64,
    pub intent_liquidity_ok: f64,
    pub trigger_observation_bonus: f64,
    pub continuation_directionality: f64,
    pub continuation_momentum: f64,
    pub continuation_imbalance: f64,
    pub continuation_aggression: f64,
    pub ready_bias_threshold: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct EntryScoringWeights {
    pub reversal_probability: f64,
    pub intent_score: f64,
    pub trigger_edge: f64,
    pub reversal_edge: f64,
    pub edge_reliability: f64,
    pub min_enter_score: f64,
    pub min_enter_confidence: f64,
    pub min_wait_probability: f64,
}

impl Default for RuntimeModelWeights {
    fn default() -> Self {
        Self {
            version: 1,
            generated_at: "static-default".to_string(),
            source: "rtts-default".to_string(),
            reversal_classifier: ReversalClassifierWeights {
                movement_drop_pct: 0.45,
                movement_velocity: 0.35,
                movement_volume_burst: 0.20,
                intent_exhaustion: 0.25,
                intent_weak_signal: 0.20,
                intent_absorption: 0.20,
                intent_timing: 0.20,
                intent_liquidity_ok: 0.15,
                trigger_observation_bonus: 0.15,
                continuation_directionality: 0.35,
                continuation_momentum: 0.25,
                continuation_imbalance: 0.20,
                continuation_aggression: 0.20,
                ready_bias_threshold: 0.55,
            },
            entry_scoring: EntryScoringWeights {
                reversal_probability: 0.40,
                intent_score: 0.20,
                trigger_edge: 0.15,
                reversal_edge: 0.10,
                edge_reliability: 0.15,
                min_enter_score: 0.58,
                min_enter_confidence: 0.52,
                min_wait_probability: 0.45,
            },
        }
    }
}

static MODEL_WEIGHTS: OnceLock<RwLock<Arc<RuntimeModelWeights>>> = OnceLock::new();

pub fn initialize(path: &str) -> Result<()> {
    let weights = if path.trim().is_empty() {
        RuntimeModelWeights::default()
    } else {
        load_from_path(path)?
    };
    store().write().clone_from(&Arc::new(weights));
    Ok(())
}

pub fn current() -> Arc<RuntimeModelWeights> {
    store().read().clone()
}

pub fn load_from_path(path: &str) -> Result<RuntimeModelWeights> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read model weights from {}", path))?;
    let parsed: RuntimeModelWeights = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse model weights JSON {}", path))?;
    validate(&parsed).with_context(|| format!("invalid model weights {}", path))?;
    Ok(parsed)
}

pub fn write_default_template(path: &str) -> Result<()> {
    let target = Path::new(path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create weights directory {}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(&RuntimeModelWeights::default())?;
    fs::write(target, body)
        .with_context(|| format!("failed to write model weight template {}", target.display()))?;
    Ok(())
}

fn validate(weights: &RuntimeModelWeights) -> Result<()> {
    if weights.version == 0 {
        anyhow::bail!("version must be >= 1");
    }
    let c = &weights.reversal_classifier;
    for value in [
        c.movement_drop_pct,
        c.movement_velocity,
        c.movement_volume_burst,
        c.intent_exhaustion,
        c.intent_weak_signal,
        c.intent_absorption,
        c.intent_timing,
        c.intent_liquidity_ok,
        c.trigger_observation_bonus,
        c.continuation_directionality,
        c.continuation_momentum,
        c.continuation_imbalance,
        c.continuation_aggression,
    ] {
        if !value.is_finite() || value < 0.0 {
            anyhow::bail!("classifier weights must be finite and non-negative");
        }
    }
    let e = &weights.entry_scoring;
    for value in [
        e.reversal_probability,
        e.intent_score,
        e.trigger_edge,
        e.reversal_edge,
        e.edge_reliability,
        e.min_enter_score,
        e.min_enter_confidence,
        e.min_wait_probability,
    ] {
        if !value.is_finite() || value < 0.0 {
            anyhow::bail!("entry scoring weights must be finite and non-negative");
        }
    }
    Ok(())
}

fn store() -> &'static RwLock<Arc<RuntimeModelWeights>> {
    MODEL_WEIGHTS.get_or_init(|| RwLock::new(Arc::new(RuntimeModelWeights::default())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_weights_validate() {
        validate(&RuntimeModelWeights::default()).expect("default weights must validate");
    }

    #[test]
    fn initialize_empty_path_uses_defaults() {
        initialize("").expect("initialize defaults");
        assert_eq!(current().version, 1);
    }
}
