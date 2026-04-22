use crate::{
    config::Config,
    decision, event_engine, execution, features, ingestion, metrics::Metrics, position, risk,
};
use anyhow::{Context, Result};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::mpsc;
use tracing::info;

pub async fn run(cfg: Config, metrics: Arc<Metrics>) -> Result<()> {
    let (market_tx, market_rx) = mpsc::channel(cfg.channel_capacity);
    let (event_tx, event_rx) = mpsc::channel(cfg.channel_capacity);
    let (feature_tx, feature_rx) = mpsc::channel(cfg.channel_capacity);
    let (decision_tx, decision_rx) = mpsc::channel(cfg.channel_capacity);
    let (intent_tx, intent_rx) = mpsc::channel(cfg.channel_capacity);
    let (risk_tx, risk_rx) = mpsc::channel(cfg.channel_capacity);
    let (fill_tx, fill_rx) = mpsc::channel(cfg.channel_capacity);

    let metrics_addr: SocketAddr = cfg
        .metrics_addr
        .parse()
        .context("invalid RTTS_METRICS_ADDR")?;
    tokio::spawn(metrics.clone().serve(metrics_addr));

    tokio::spawn(event_engine::run(
        cfg.window_ms,
        market_rx,
        event_tx,
        metrics.clone(),
    ));
    tokio::spawn(features::run(
        cfg.window_ms,
        event_rx,
        feature_tx,
        metrics.clone(),
    ));
    tokio::spawn(decision::run(feature_rx, decision_tx, metrics.clone()));
    tokio::spawn(position::run(
        cfg.clone(),
        decision_rx,
        fill_rx,
        intent_tx,
        metrics.clone(),
    ));
    tokio::spawn(risk::run(cfg.clone(), intent_rx, risk_tx, metrics.clone()));
    tokio::spawn(execution::run(risk_rx, fill_tx, metrics.clone()));

    info!(addr = %metrics_addr, "metrics endpoint listening at /metrics");
    ingestion::run(cfg, market_tx, metrics).await
}

