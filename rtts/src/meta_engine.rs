use crate::{
    competition_model,
    config::Config,
    entry_quality, ev_calculator,
    metrics::Metrics,
    scenario_simulator,
    types::{FinalDecision, MetaDecision, OrderIntent},
};
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio::sync::mpsc::{Receiver, Sender};

#[derive(Default)]
struct MetaState {
    recent_executed: VecDeque<bool>,
    recent_adjusted_ev: VecDeque<f64>,
    skipped: u64,
    executed: u64,
}

pub async fn run(
    cfg: Config,
    mut rx: Receiver<OrderIntent>,
    tx: Sender<OrderIntent>,
    metrics: Arc<Metrics>,
) {
    let mut state = MetaState::default();
    while let Some(mut intent) = rx.recv().await {
        let mut meta = evaluate(&cfg, &state, &intent);
        if meta.decision == FinalDecision::Wait {
            tokio::time::sleep(Duration::from_millis(confirm_window_ms(&intent))).await;
            let refreshed = evaluate(&cfg, &state, &intent);
            if refreshed.adjusted_ev < meta.adjusted_ev * 0.85
                || refreshed.entry_quality < meta.entry_quality * 0.92
                || refreshed.competition_score > meta.competition_score + 0.12
            {
                meta = MetaDecision {
                    decision: FinalDecision::Skip,
                    reason: "confirmation_decay",
                    ..refreshed
                };
            } else {
                meta = MetaDecision {
                    decision: FinalDecision::Execute,
                    reason: "confirmed_edge",
                    ..refreshed
                };
            }
        }

        observe(&mut state, &intent, &meta, &metrics);
        intent.meta = Some(meta.clone());
        match meta.decision {
            FinalDecision::Execute => {
                state.executed = state.executed.saturating_add(1);
                if tx.try_send(intent.clone()).is_err() {
                    metrics
                        .channel_backpressure_total
                        .with_label_values(&["meta"])
                        .inc();
                    if tx.send(intent).await.is_err() {
                        break;
                    }
                }
            }
            FinalDecision::Wait | FinalDecision::Skip => {
                state.skipped = state.skipped.saturating_add(1);
                metrics
                    .false_positives_avoided
                    .with_label_values(&[meta.reason])
                    .inc();
            }
        }
    }
}

fn evaluate(cfg: &Config, state: &MetaState, intent: &OrderIntent) -> MetaDecision {
    let scenarios = scenario_simulator::simulate(intent);
    let notional = intent.request.size * intent.last_price;
    let ev = ev_calculator::calculate(
        &scenarios,
        intent.expected_slippage_bps,
        intent.data_latency_ms,
        notional,
    );
    let entry_quality = entry_quality::score(intent);
    let competition_score = competition_model::score(intent);
    let opportunity_rank =
        opportunity_rank(state, ev.adjusted_ev, entry_quality, competition_score);
    let thresholds = thresholds(cfg, state, intent);
    let recent_success = recent_success_rate(state);

    let (decision, reason) = if intent.regime.spread > 18.0 {
        (FinalDecision::Skip, "spread_too_wide")
    } else if intent.regime.volatility > 4.2 && intent.regime.trend_strength < 0.8 {
        (FinalDecision::Skip, "toxic_volatility")
    } else if recent_success < 0.38 && state.recent_executed.len() >= 12 {
        (FinalDecision::Skip, "recent_hit_rate_low")
    } else if ev.worst_case_loss > cfg.capital * cfg.max_risk_pct * 1.4 {
        (FinalDecision::Skip, "worst_case_too_large")
    } else if ev.adjusted_ev <= thresholds.ev {
        (FinalDecision::Skip, "negative_or_weak_ev")
    } else if entry_quality <= thresholds.entry_quality {
        (FinalDecision::Skip, "poor_entry_quality")
    } else if competition_score >= thresholds.competition {
        (FinalDecision::Skip, "opportunity_consumed")
    } else if opportunity_rank < thresholds.opportunity_rank {
        (FinalDecision::Wait, "rank_wait")
    } else if intent.urgency < 0.55 && intent.expected_duration_ms > 120 {
        (FinalDecision::Wait, "needs_confirmation")
    } else {
        (FinalDecision::Execute, "edge_validated")
    };

    MetaDecision {
        decision,
        scenarios,
        ev: ev.ev,
        adjusted_ev: ev.adjusted_ev,
        worst_case_loss: ev.worst_case_loss,
        entry_quality,
        competition_score,
        opportunity_rank,
        reason,
    }
}

struct Thresholds {
    ev: f64,
    entry_quality: f64,
    competition: f64,
    opportunity_rank: f64,
}

fn thresholds(cfg: &Config, state: &MetaState, intent: &OrderIntent) -> Thresholds {
    let dd_pressure = if state.recent_adjusted_ev.iter().rev().take(8).sum::<f64>() < 0.0 {
        0.10
    } else {
        0.0
    };
    let regime_penalty = if intent.regime.spread > 10.0 || intent.regime.volatility > 3.0 {
        0.08
    } else {
        0.0
    };
    Thresholds {
        ev: (cfg.base_order_usd * 0.00015) + dd_pressure,
        entry_quality: (0.58 + regime_penalty + dd_pressure * 0.5).clamp(0.50, 0.82),
        competition: (0.78 - regime_penalty).clamp(0.55, 0.82),
        opportunity_rank: (0.52 + regime_penalty).clamp(0.45, 0.75),
    }
}

fn opportunity_rank(
    state: &MetaState,
    adjusted_ev: f64,
    entry_quality: f64,
    competition: f64,
) -> f64 {
    let pending_quality = state
        .recent_adjusted_ev
        .iter()
        .rev()
        .take(6)
        .copied()
        .fold(0.0, f64::max);
    let relative_ev = if pending_quality > 0.0 {
        (adjusted_ev / pending_quality.max(1e-9)).clamp(0.0, 1.5) / 1.5
    } else {
        0.65
    };
    (0.45 * entry_quality + 0.35 * relative_ev + 0.20 * (1.0 - competition)).clamp(0.0, 1.0)
}

fn recent_success_rate(state: &MetaState) -> f64 {
    if state.recent_executed.is_empty() {
        return 0.50;
    }
    let wins = state.recent_executed.iter().filter(|value| **value).count() as f64;
    wins / state.recent_executed.len() as f64
}

fn observe(state: &mut MetaState, intent: &OrderIntent, meta: &MetaDecision, metrics: &Metrics) {
    state.recent_adjusted_ev.push_back(meta.adjusted_ev);
    if state.recent_adjusted_ev.len() > 64 {
        state.recent_adjusted_ev.pop_front();
    }
    if meta.decision == FinalDecision::Execute {
        state.recent_executed.push_back(meta.adjusted_ev > 0.0);
        if state.recent_executed.len() > 64 {
            state.recent_executed.pop_front();
        }
    }
    metrics
        .meta_decisions_total
        .with_label_values(&[final_label(meta.decision), meta.reason])
        .inc();
    metrics
        .ev_adjusted
        .with_label_values(&[&intent.request.symbol])
        .observe(meta.adjusted_ev);
    metrics
        .entry_quality
        .with_label_values(&[&intent.request.symbol])
        .observe(meta.entry_quality);
    metrics
        .competition_score
        .with_label_values(&[&intent.request.symbol])
        .observe(meta.competition_score);
}

fn confirm_window_ms(intent: &OrderIntent) -> u64 {
    (intent.expected_duration_ms / 8).clamp(3, 35)
}

fn final_label(decision: FinalDecision) -> &'static str {
    match decision {
        FinalDecision::Execute => "execute",
        FinalDecision::Wait => "wait",
        FinalDecision::Skip => "skip",
    }
}
