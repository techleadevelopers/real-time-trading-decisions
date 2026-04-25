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
    pub execution_latency_us: HistogramVec,
    pub slippage_bps: HistogramVec,
    pub microtrade_pnl: HistogramVec,
    pub hit_rate: GaugeVec,
    pub scale_efficiency: GaugeVec,
    pub meta_decisions_total: IntCounterVec,
    pub ev_adjusted: HistogramVec,
    pub entry_quality: HistogramVec,
    pub competition_score: HistogramVec,
    pub false_positives_avoided: IntCounterVec,
    pub position_size: GaugeVec,
    pub drawdown: GaugeVec,
    pub avg_time_to_fill_ms: HistogramVec,
    pub cancel_replace_ratio: HistogramVec,
    pub execution_efficiency: HistogramVec,
    pub fill_expected_divergence: HistogramVec,
    pub aborted_due_to_decay_total: IntCounterVec,
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
        let execution_latency_us = HistogramVec::new(
            HistogramOpts::new(
                "rtts_execution_latency_us",
                "Order submit-to-fill latency in microseconds",
            )
            .buckets(vec![
                50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10_000.0,
            ]),
            &["order_type"],
        )?;
        let slippage_bps = HistogramVec::new(
            HistogramOpts::new("rtts_slippage_bps", "Actual slippage in basis points")
                .buckets(vec![-2.0, -1.0, 0.0, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0]),
            &["side"],
        )?;
        let microtrade_pnl = HistogramVec::new(
            HistogramOpts::new("rtts_microtrade_pnl", "Per microtrade paper PnL")
                .buckets(vec![-10.0, -5.0, -2.0, -1.0, 0.0, 1.0, 2.0, 5.0, 10.0]),
            &["symbol"],
        )?;
        let hit_rate = GaugeVec::new(
            prometheus::Opts::new("rtts_hit_rate_by_regime", "Adaptive hit rate by regime"),
            &["regime"],
        )?;
        let scale_efficiency = GaugeVec::new(
            prometheus::Opts::new(
                "rtts_scale_efficiency",
                "PnL per added unit of scaled exposure",
            ),
            &["symbol"],
        )?;
        let meta_decisions_total = IntCounterVec::new(
            prometheus::Opts::new("rtts_meta_decisions_total", "Final meta decisions"),
            &["decision", "reason"],
        )?;
        let ev_adjusted = HistogramVec::new(
            HistogramOpts::new(
                "rtts_adjusted_ev",
                "Latency and slippage adjusted expected value",
            )
            .buckets(vec![-10.0, -5.0, -2.0, -1.0, 0.0, 0.5, 1.0, 2.0, 5.0, 10.0]),
            &["symbol"],
        )?;
        let entry_quality = HistogramVec::new(
            HistogramOpts::new("rtts_entry_quality", "Meta entry quality score")
                .buckets(vec![0.0, 0.25, 0.40, 0.55, 0.65, 0.75, 0.85, 0.95, 1.0]),
            &["symbol"],
        )?;
        let competition_score = HistogramVec::new(
            HistogramOpts::new("rtts_competition_score", "Estimated opportunity crowding")
                .buckets(vec![0.0, 0.25, 0.40, 0.55, 0.70, 0.85, 1.0]),
            &["symbol"],
        )?;
        let false_positives_avoided = IntCounterVec::new(
            prometheus::Opts::new(
                "rtts_false_positives_avoided_total",
                "Signals skipped by meta engine that likely avoided poor execution",
            ),
            &["reason"],
        )?;
        let position_size = GaugeVec::new(
            prometheus::Opts::new("rtts_position_size", "Signed current position size"),
            &["symbol"],
        )?;
        let drawdown = GaugeVec::new(
            prometheus::Opts::new("rtts_drawdown", "Current daily drawdown"),
            &["symbol"],
        )?;
        let avg_time_to_fill_ms = HistogramVec::new(
            HistogramOpts::new("rtts_avg_time_to_fill_ms", "Observed order time to fill in milliseconds")
                .buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1_000.0]),
            &["symbol"],
        )?;
        let cancel_replace_ratio = HistogramVec::new(
            HistogramOpts::new("rtts_cancel_replace_ratio", "Per-order cancel/replace intensity")
                .buckets(vec![0.0, 0.25, 0.50, 1.0, 1.5, 2.0, 3.0, 5.0]),
            &["symbol"],
        )?;
        let execution_efficiency = HistogramVec::new(
            HistogramOpts::new("rtts_execution_efficiency", "Execution efficiency versus expected fill timing")
                .buckets(vec![-1.0, -0.5, -0.25, 0.0, 0.25, 0.50, 0.75, 1.0]),
            &["symbol"],
        )?;
        let fill_expected_divergence = HistogramVec::new(
            HistogramOpts::new("rtts_fill_expected_divergence", "Absolute divergence between expected and realized fill progress")
                .buckets(vec![0.0, 0.05, 0.10, 0.20, 0.35, 0.50, 0.75, 1.0]),
            &["symbol"],
        )?;
        let aborted_due_to_decay_total = IntCounterVec::new(
            prometheus::Opts::new("rtts_aborted_due_to_decay_total", "Orders aborted after edge decay exceeded half-life"),
            &["symbol"],
        )?;

        registry.register(Box::new(events_total.clone()))?;
        registry.register(Box::new(decisions_total.clone()))?;
        registry.register(Box::new(orders_total.clone()))?;
        registry.register(Box::new(fills_total.clone()))?;
        registry.register(Box::new(rejected_orders_total.clone()))?;
        registry.register(Box::new(channel_backpressure_total.clone()))?;
        registry.register(Box::new(stage_latency_us.clone()))?;
        registry.register(Box::new(execution_latency_us.clone()))?;
        registry.register(Box::new(slippage_bps.clone()))?;
        registry.register(Box::new(microtrade_pnl.clone()))?;
        registry.register(Box::new(hit_rate.clone()))?;
        registry.register(Box::new(scale_efficiency.clone()))?;
        registry.register(Box::new(meta_decisions_total.clone()))?;
        registry.register(Box::new(ev_adjusted.clone()))?;
        registry.register(Box::new(entry_quality.clone()))?;
        registry.register(Box::new(competition_score.clone()))?;
        registry.register(Box::new(false_positives_avoided.clone()))?;
        registry.register(Box::new(position_size.clone()))?;
        registry.register(Box::new(drawdown.clone()))?;
        registry.register(Box::new(avg_time_to_fill_ms.clone()))?;
        registry.register(Box::new(cancel_replace_ratio.clone()))?;
        registry.register(Box::new(execution_efficiency.clone()))?;
        registry.register(Box::new(fill_expected_divergence.clone()))?;
        registry.register(Box::new(aborted_due_to_decay_total.clone()))?;

        Ok(Arc::new(Self {
            registry,
            events_total,
            decisions_total,
            orders_total,
            fills_total,
            rejected_orders_total,
            channel_backpressure_total,
            stage_latency_us,
            execution_latency_us,
            slippage_bps,
            microtrade_pnl,
            hit_rate,
            scale_efficiency,
            meta_decisions_total,
            ev_adjusted,
            entry_quality,
            competition_score,
            false_positives_avoided,
            position_size,
            drawdown,
            avg_time_to_fill_ms,
            cancel_replace_ratio,
            execution_efficiency,
            fill_expected_divergence,
            aborted_due_to_decay_total,
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
