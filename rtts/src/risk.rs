use crate::{config::Config, metrics::Metrics, types::OrderIntent};
use std::{sync::Arc, time::Instant};
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::warn;

pub async fn run(
    cfg: Config,
    mut rx: Receiver<OrderIntent>,
    tx: Sender<OrderIntent>,
    metrics: Arc<Metrics>,
) {
    let mut realized_pnl = 0.0;
    let mut kill_switch = false;

    while let Some(intent) = rx.recv().await {
        let started = Instant::now();
        let decision = validate(&cfg, &intent, realized_pnl, kill_switch);
        metrics
            .stage_latency_us
            .with_label_values(&["risk"])
            .observe(started.elapsed().as_micros() as f64);

        match decision {
            Ok(()) => {
                if abnormal(&intent) {
                    kill_switch = true;
                    metrics
                        .rejected_orders_total
                        .with_label_values(&["kill_switch"])
                        .inc();
                    warn!(?intent, "risk kill switch tripped");
                    continue;
                }
                if tx.try_send(intent.clone()).is_err() {
                    metrics
                        .channel_backpressure_total
                        .with_label_values(&["risk"])
                        .inc();
                    if tx.send(intent).await.is_err() {
                        break;
                    }
                }
            }
            Err(reason) => {
                if reason == "daily_drawdown" {
                    kill_switch = true;
                }
                metrics.rejected_orders_total.with_label_values(&[reason]).inc();
                warn!(reason, "order rejected by risk");
            }
        }
        realized_pnl = realized_pnl.min(0.0);
        metrics
            .drawdown
            .with_label_values(&[&cfg.symbol])
            .set((-realized_pnl).max(0.0));
    }
}

fn validate<'a>(
    cfg: &Config,
    intent: &OrderIntent,
    realized_pnl: f64,
    kill_switch: bool,
) -> Result<(), &'a str> {
    if kill_switch {
        return Err("kill_switch");
    }
    if intent.request.size <= 0.0 || !intent.request.size.is_finite() {
        return Err("bad_size");
    }
    if !intent.last_price.is_finite() || intent.last_price <= 0.0 {
        return Err("bad_price");
    }
    let notional = intent.request.size * intent.last_price;
    let max_trade_risk = cfg.capital * cfg.max_risk_pct;
    let stop_distance = cfg.stop_loss_bps / 10_000.0 * intent.last_price;
    let risk_estimate = intent.request.size * stop_distance;
    if risk_estimate > max_trade_risk {
        return Err("trade_risk");
    }
    if notional > cfg.capital * 0.25 {
        return Err("notional");
    }
    if -realized_pnl > cfg.capital * cfg.max_daily_drawdown_pct {
        return Err("daily_drawdown");
    }
    Ok(())
}

#[inline]
fn abnormal(intent: &OrderIntent) -> bool {
    !intent.score.is_finite() || intent.score > 1.0 || intent.score < 0.0
}

