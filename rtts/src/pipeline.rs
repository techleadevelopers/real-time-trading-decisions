use crate::{
    adaptive_engine, config::Config, execution_smart, ingestion, meta_engine, metrics::Metrics,
    microstructure, position, risk,
};
use anyhow::{Context, Result};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::mpsc;
use tracing::info;

pub async fn run(cfg: Config, metrics: Arc<Metrics>) -> Result<()> {
    let (update_tx, update_rx) = mpsc::channel(cfg.channel_capacity);
    let (micro_tx, micro_rx) = mpsc::channel(cfg.channel_capacity);
    let (decision_tx, decision_rx) = mpsc::channel(cfg.channel_capacity);
    let (intent_tx, intent_rx) = mpsc::channel(cfg.channel_capacity);
    let (risk_tx, risk_rx) = mpsc::channel(cfg.channel_capacity);
    let (meta_tx, meta_rx) = mpsc::channel(cfg.channel_capacity);
    let (fill_tx, fill_rx) = mpsc::channel(cfg.channel_capacity);
    let (learning_tx, learning_rx) = mpsc::channel(cfg.channel_capacity);

    let metrics_addr: SocketAddr = cfg
        .metrics_addr
        .parse()
        .context("invalid RTTS_METRICS_ADDR")?;
    tokio::spawn(metrics.clone().serve(metrics_addr));

    tokio::spawn(microstructure::run(
        cfg.window_ms,
        cfg.max_data_age_ms,
        update_rx,
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
        fill_tx,
        learning_tx,
        metrics.clone(),
    ));

    info!(addr = %metrics_addr, "metrics endpoint listening at /metrics");
    ingestion::run(cfg, update_tx, metrics).await
}
