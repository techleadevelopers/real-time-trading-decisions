use crate::{
    adaptive_engine, config::Config, execution_external, execution_smart, execution_truth, ingestion, meta_engine,
    metrics::Metrics, microstructure, position, risk, types::MarketUpdate,
};
use anyhow::{Context, Result};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::mpsc;
use tracing::info;

pub async fn run(cfg: Config, metrics: Arc<Metrics>) -> Result<()> {
    let (update_tx, mut update_rx) = mpsc::channel(cfg.channel_capacity);
    let (micro_update_tx, micro_update_rx) = mpsc::channel(cfg.channel_capacity);
    let (truth_market_tx, truth_market_rx) = mpsc::channel(cfg.channel_capacity);
    let (micro_tx, micro_rx) = mpsc::channel(cfg.channel_capacity);
    let (decision_tx, decision_rx) = mpsc::channel(cfg.channel_capacity);
    let (intent_tx, intent_rx) = mpsc::channel(cfg.channel_capacity);
    let (risk_tx, risk_rx) = mpsc::channel(cfg.channel_capacity);
    let (meta_tx, meta_rx) = mpsc::channel(cfg.channel_capacity);
    let (fill_tx, fill_rx) = mpsc::channel(cfg.channel_capacity);
    let (truth_fill_tx, truth_fill_rx) = mpsc::channel(cfg.channel_capacity);
    let (learning_tx, learning_rx) = mpsc::channel(cfg.channel_capacity);

    let metrics_addr: SocketAddr = cfg
        .metrics_addr
        .parse()
        .context("invalid RTTS_METRICS_ADDR")?;
    tokio::spawn(metrics.clone().serve(metrics_addr));

    let fanout_metrics = metrics.clone();
    tokio::spawn(async move {
        while let Some(update) = update_rx.recv().await {
            fanout_market_update(update, &micro_update_tx, &truth_market_tx, &fanout_metrics).await;
        }
    });

    tokio::spawn(microstructure::run(
        cfg.window_ms,
        cfg.max_data_age_ms,
        micro_update_rx,
        micro_tx,
        metrics.clone(),
    ));
    tokio::spawn(adaptive_engine::run(
        cfg.clone(),
        micro_rx,
        learning_rx,
        decision_tx,
        metrics.clone(),
    ));
    tokio::spawn(position::run(
        cfg.clone(),
        decision_rx,
        fill_rx,
        intent_tx,
        metrics.clone(),
    ));
    tokio::spawn(risk::run(cfg.clone(), intent_rx, risk_tx, metrics.clone()));
    tokio::spawn(meta_engine::run(
        cfg.clone(),
        risk_rx,
        meta_tx,
        metrics.clone(),
    ));
    tokio::spawn(execution_smart::run(
        cfg.clone(),
        meta_rx,
        metrics.clone(),
    ));
    tokio::spawn(execution_external::run(
        cfg.clone(),
        fill_tx,
        truth_fill_tx,
        metrics.clone(),
    ));
    tokio::spawn(execution_truth::run(
        truth_market_rx,
        truth_fill_rx,
        learning_tx,
        metrics.clone(),
    ));

    info!(addr = %metrics_addr, "metrics endpoint listening at /metrics");
    ingestion::run(cfg, update_tx, metrics).await
}

async fn fanout_market_update(
    update: MarketUpdate,
    micro_tx: &mpsc::Sender<MarketUpdate>,
    truth_tx: &mpsc::Sender<MarketUpdate>,
    metrics: &Metrics,
) {
    if micro_tx.try_send(update.clone()).is_err() {
        metrics
            .channel_backpressure_total
            .with_label_values(&["market_fanout_micro"])
            .inc();
        let _ = micro_tx.send(update.clone()).await;
    }
    if truth_tx.try_send(update).is_err() {
        metrics
            .channel_backpressure_total
            .with_label_values(&["market_fanout_truth"])
            .inc();
    }
}
