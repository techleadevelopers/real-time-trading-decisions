use crate::{
    accounting::latency::{latency_impact_score, LatencyDistributions},
    config::Config,
    execution_controller::{ExecutionInstruction, ExecutionStrategy},
    execution_mode::ExecutionModeSwitch,
    fill_probability::FillProbabilityModel,
    metrics::Metrics,
    queue_position::QueuePositionEngine,
    symbol_profile::SymbolProfileEngine,
    types::{ExecutionMode, FillProbabilityClass, OrderIntent, OrderType, Side},
};
use chrono::{TimeZone, Utc};
use reqwest::Client;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tracing::{info, warn};

#[derive(Serialize)]
struct ControlPlaneExecutionRequest {
    idempotency_key: String,
    symbol: String,
    side: String,
    size: f64,
    price: Option<f64>,
    decision: &'static str,
    signal_time: String,
    max_slippage_bps: f64,
    reduce_only: bool,
    request_timestamp: String,
    expected_realized_markout: f64,
    regime_kind: String,
    regime_volatility: f64,
    regime_spread: f64,
    regime_trend_strength: f64,
}

#[derive(Serialize)]
struct CancelRequest<'a> {
    reason: &'a str,
}

#[derive(Serialize)]
struct ReplaceRequest<'a> {
    new_price: f64,
    reason: &'a str,
}

#[derive(Serialize)]
struct SwitchStrategyRequest<'a> {
    strategy: &'a str,
    price: Option<f64>,
    reason: &'a str,
}

pub async fn run(
    cfg: Config,
    mut rx: Receiver<ExecutionInstruction>,
    metrics: Arc<Metrics>,
) {
    let mut symbol_profile = SymbolProfileEngine::new(cfg.symbol.clone());
    let latency_distributions = LatencyDistributions::default();
    let client = Client::new();
    let root = cfg.control_plane_http.trim_end_matches('/').to_string();

    while let Some(instruction) = rx.recv().await {
        match instruction {
            ExecutionInstruction::Submit {
                mut intent,
                idempotency_key,
            } => {
                if intent.data_latency_ms > cfg.max_data_age_ms {
                    metrics
                        .rejected_orders_total
                        .with_label_values(&["stale_before_execute"])
                        .inc();
                    continue;
                }
                if intent
                    .meta
                    .as_ref()
                    .is_some_and(|meta| meta.decision != crate::types::FinalDecision::Execute)
                {
                    metrics
                        .rejected_orders_total
                        .with_label_values(&["meta_not_execute"])
                        .inc();
                    continue;
                }

                let expected_realized_markout =
                    expected_real_markout_after_cost(&intent, &latency_distributions);
                if expected_realized_markout <= execution_threshold(&cfg) {
                    metrics
                        .rejected_orders_total
                        .with_label_values(&["weak_expected_real_markout"])
                        .inc();
                    continue;
                }

                symbol_profile.observe_intent(&intent);
                prepare_execution(&mut intent, symbol_profile.profile());
                let request = ControlPlaneExecutionRequest {
                    idempotency_key,
                    symbol: intent.request.symbol.clone(),
                    side: match intent.request.side {
                        Side::Buy => "BUY".to_string(),
                        Side::Sell => "SELL".to_string(),
                    },
                    size: intent.request.size,
                    price: intent.request.price,
                    decision: "Execute",
                    signal_time: iso8601_utc(intent.timestamp),
                    max_slippage_bps: intent.request.max_slippage_bps,
                    reduce_only: intent.request.reduce_only,
                    request_timestamp: iso8601_utc(now_ms()),
                    expected_realized_markout,
                    regime_kind: format!("{:?}", intent.context.regime),
                    regime_volatility: intent.regime.volatility,
                    regime_spread: intent.regime.spread,
                    regime_trend_strength: intent.regime.trend_strength,
                };
                let side_label = match intent.request.side {
                    Side::Buy => "buy",
                    Side::Sell => "sell",
                };
                metrics.orders_total.with_label_values(&[side_label]).inc();
                post_control_action(
                    &client,
                    &format!("{root}/execution/requests"),
                    &request,
                    &metrics,
                    &intent.request.symbol,
                    "submit",
                )
                .await;
            }
            ExecutionInstruction::Cancel {
                order_id,
                symbol,
                reason,
            } => {
                post_control_action(
                    &client,
                    &format!("{root}/execution/orders/{order_id}/cancel"),
                    &CancelRequest { reason },
                    &metrics,
                    &symbol,
                    "cancel",
                )
                .await;
            }
            ExecutionInstruction::Replace {
                order_id,
                symbol,
                new_price,
                reason,
            } => {
                post_control_action(
                    &client,
                    &format!("{root}/execution/orders/{order_id}/replace"),
                    &ReplaceRequest { new_price, reason },
                    &metrics,
                    &symbol,
                    "replace",
                )
                .await;
            }
            ExecutionInstruction::SwitchStrategy {
                order_id,
                symbol,
                new_strategy,
                price,
                reason,
            } => {
                post_control_action(
                    &client,
                    &format!("{root}/execution/orders/{order_id}/strategy"),
                    &SwitchStrategyRequest {
                        strategy: strategy_label(new_strategy),
                        price,
                        reason,
                    },
                    &metrics,
                    &symbol,
                    "switch_strategy",
                )
                .await;
            }
            ExecutionInstruction::Abort {
                order_id,
                symbol,
                reason,
            } => {
                post_control_action(
                    &client,
                    &format!("{root}/execution/orders/{order_id}/cancel"),
                    &CancelRequest { reason },
                    &metrics,
                    &symbol,
                    "abort",
                )
                .await;
            }
        }
    }
}

fn prepare_execution(intent: &mut OrderIntent, profile: &crate::types::SymbolProfile) {
    let queue = QueuePositionEngine::estimate(intent, profile);
    let fill_class = FillProbabilityModel::classify(intent, &queue);
    let mode = ExecutionModeSwitch::choose(intent, fill_class);

    intent.execution_mode = mode;
    intent.fill_probability = fill_class;
    intent.request.order_type = match mode {
        ExecutionMode::Aggressive | ExecutionMode::Defensive => OrderType::Market,
        ExecutionMode::Passive => OrderType::Limit,
    };
    intent.request.post_only = mode == ExecutionMode::Passive;
    if mode == ExecutionMode::Defensive {
        intent.request.reduce_only = true;
    }
    intent.queue_estimate = QueuePositionEngine::estimate(intent, profile);

    if intent.request.order_type == OrderType::Limit {
        let reference = intent.request.price.unwrap_or(intent.last_price);
        let depth = intent.queue_estimate.placement_depth_bps / 10_000.0;
        let inside = match intent.request.side {
            Side::Buy => reference * (1.0 - depth),
            Side::Sell => reference * (1.0 + depth),
        };
        intent.request.price = Some(inside);
    }
}

fn expected_real_markout_after_cost(intent: &OrderIntent, latency_distributions: &LatencyDistributions) -> f64 {
    let expected_bps = (intent.flow.continuation_strength * 5.0 + intent.timing.timing_score * 3.0
        - intent.expected_slippage_bps
        - intent.regime.spread.max(0.0) * 0.25)
        .max(-10.0);
    let notional = intent.request.size * intent.last_price;
    let snapshot = latency_distributions.snapshot();
    let fill_quality = match intent.fill_probability {
        FillProbabilityClass::HighFill => 1.0,
        FillProbabilityClass::LowFill => 0.4,
    };
    let latency_penalty = latency_impact_score(&snapshot, intent.expected_slippage_bps, fill_quality);
    notional * expected_bps / 10_000.0 - notional * (0.0004 + latency_penalty * 0.00025)
}

fn execution_threshold(cfg: &Config) -> f64 {
    (cfg.base_order_usd * 0.00005).max(0.001)
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn iso8601_utc(timestamp_ms: u64) -> String {
    Utc.timestamp_millis_opt(timestamp_ms as i64)
        .single()
        .unwrap_or_else(Utc::now)
        .to_rfc3339()
}

async fn post_control_action<T: Serialize>(
    client: &Client,
    endpoint: &str,
    payload: &T,
    metrics: &Metrics,
    symbol: &str,
    action: &str,
) {
    match client.post(endpoint).json(payload).send().await {
        Ok(response) if response.status().is_success() => {
            info!(%symbol, action, "execution control action submitted");
        }
        Ok(response) => {
            metrics
                .rejected_orders_total
                .with_label_values(&["control_plane_action_rejected"])
                .inc();
            warn!(status = %response.status(), %symbol, action, "control-plane rejected execution action");
        }
        Err(err) => {
            metrics
                .rejected_orders_total
                .with_label_values(&["control_plane_action_unreachable"])
                .inc();
            warn!(%err, %symbol, action, "failed to submit execution control action");
        }
    }
}

fn strategy_label(strategy: ExecutionStrategy) -> &'static str {
    match strategy {
        ExecutionStrategy::Passive => "PASSIVE",
        ExecutionStrategy::Aggressive => "AGGRESSIVE",
        ExecutionStrategy::Defensive => "DEFENSIVE",
    }
}
