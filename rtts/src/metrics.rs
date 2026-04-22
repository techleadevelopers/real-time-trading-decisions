use anyhow::Result;
use prometheus::{
    Encoder, GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, Registry, TextEncoder,
};
use std::{net::SocketAddr, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tracing::warn;

#[derive(Clone)]
pub struct Metrics {
    registry: Registry,
    pub events_total: IntCounterVec,
    pub decisions_total: IntCounterVec,
    pub orders_total: IntCounterVec,
    pub fills_total: IntCounterVec,
    pub rejected_orders_total: IntCounterVec,
    pub channel_backpressure_total: IntCounterVec,
    pub stage_latency_us: HistogramVec,
    pub position_size: GaugeVec,
    pub drawdown: GaugeVec,
}

impl Metrics {
    pub fn new() -> Result<Arc<Self>> {
        let registry = Registry::new();
        let events_total = IntCounterVec::new(
            prometheus::Opts::new("rtts_events_total", "Market events processed by stage"),
            &["stage"],
        )?;
        let decisions_total = IntCounterVec::new(
            prometheus::Opts::new("rtts_decisions_total", "Decisions emitted"),
            &["decision"],
        )?;
        let orders_total = IntCounterVec::new(
            prometheus::Opts::new("rtts_orders_total", "Orders accepted for execution"),
            &["side"],
        )?;
        let fills_total = IntCounterVec::new(
            prometheus::Opts::new("rtts_fills_total", "Paper fills"),
            &["side"],
        )?;
        let rejected_orders_total = IntCounterVec::new(
            prometheus::Opts::new("rtts_rejected_orders_total", "Orders rejected by risk"),
            &["reason"],
        )?;
        let channel_backpressure_total = IntCounterVec::new(
            prometheus::Opts::new(
                "rtts_channel_backpressure_total",
                "Bounded channel full events",
            ),
            &["stage"],
        )?;
        let stage_latency_us = HistogramVec::new(
            HistogramOpts::new(
                "rtts_stage_latency_us",
                "Stage processing latency in microseconds",
            )
            .buckets(vec![
                10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0,
            ]),
            &["stage"],
        )?;
        let position_size = GaugeVec::new(
            prometheus::Opts::new("rtts_position_size", "Signed current position size"),
            &["symbol"],
        )?;
        let drawdown = GaugeVec::new(
            prometheus::Opts::new("rtts_drawdown", "Current daily drawdown"),
            &["symbol"],
        )?;

        registry.register(Box::new(events_total.clone()))?;
        registry.register(Box::new(decisions_total.clone()))?;
        registry.register(Box::new(orders_total.clone()))?;
        registry.register(Box::new(fills_total.clone()))?;
        registry.register(Box::new(rejected_orders_total.clone()))?;
        registry.register(Box::new(channel_backpressure_total.clone()))?;
        registry.register(Box::new(stage_latency_us.clone()))?;
        registry.register(Box::new(position_size.clone()))?;
        registry.register(Box::new(drawdown.clone()))?;

        Ok(Arc::new(Self {
            registry,
            events_total,
            decisions_total,
            orders_total,
            fills_total,
            rejected_orders_total,
            channel_backpressure_total,
            stage_latency_us,
            position_size,
            drawdown,
        }))
    }

    pub async fn serve(self: Arc<Self>, addr: SocketAddr) {
        let listener = match TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(err) => {
                warn!(%err, "metrics listener bind failed");
                return;
            }
        };

        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                continue;
            };
            let metrics = self.clone();
            tokio::spawn(async move {
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request).await;
                let body = metrics.render();
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/plain; version=0.0.4\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    }

    fn render(&self) -> String {
        let mut buffer = Vec::with_capacity(8192);
        let encoder = TextEncoder::new();
        let families = self.registry.gather();
        match encoder.encode(&families, &mut buffer) {
            Ok(()) => String::from_utf8(buffer).unwrap_or_else(|_| String::new()),
            Err(err) => format!("# metrics encode error: {err}\n"),
        }
    }
}
