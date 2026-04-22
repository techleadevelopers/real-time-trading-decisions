use crate::{
    metrics::Metrics,
    types::{DetectedEvent, FeatureFrame, Features, MarketEvent},
};
use std::{collections::VecDeque, sync::Arc, time::Instant};
use tokio::sync::mpsc::{Receiver, Sender};

pub async fn run(
    window_ms: u64,
    mut rx: Receiver<DetectedEvent>,
    tx: Sender<FeatureFrame>,
    metrics: Arc<Metrics>,
) {
    let mut window = VecDeque::<MarketEvent>::with_capacity(512);
    while let Some(input) = rx.recv().await {
        let started = Instant::now();
        trim_window(&mut window, input.market.timestamp, window_ms);
        window.push_back(input.market.clone());
        let features = compute(&window);
        metrics.events_total.with_label_values(&["features"]).inc();
        metrics
            .stage_latency_us
            .with_label_values(&["features"])
            .observe(started.elapsed().as_micros() as f64);

        let output = FeatureFrame {
            market: input.market,
            event: input.event,
            features,
        };
        if tx.try_send(output.clone()).is_err() {
            metrics
                .channel_backpressure_total
                .with_label_values(&["features"])
                .inc();
            if tx.send(output).await.is_err() {
                break;
            }
        }
    }
}

fn trim_window(window: &mut VecDeque<MarketEvent>, now: u64, window_ms: u64) {
    let cutoff = now.saturating_sub(window_ms);
    while window.front().is_some_and(|event| event.timestamp < cutoff) {
        window.pop_front();
    }
}

fn compute(window: &VecDeque<MarketEvent>) -> Features {
    let Some(first) = window.front() else {
        return Features::default();
    };
    let last = window.back().expect("window has first");
    let elapsed_ms = last.timestamp.saturating_sub(first.timestamp).max(1) as f64;
    let velocity =
        ((last.price - first.price) / first.price.max(f64::EPSILON) / elapsed_ms) * 1_000.0;

    let len = window.len() as f64;
    let mean_volume = window.iter().map(|event| event.volume).sum::<f64>() / len;
    let variance = window
        .iter()
        .map(|event| {
            let diff = event.volume - mean_volume;
            diff * diff
        })
        .sum::<f64>()
        / len.max(1.0);
    let vol_z = if variance > f64::EPSILON {
        (last.volume - mean_volume) / variance.sqrt()
    } else {
        0.0
    };

    let mean_price = window.iter().map(|event| event.price).sum::<f64>() / len;
    let volatility = window
        .iter()
        .map(|event| {
            let ret = (event.price - mean_price) / mean_price.max(f64::EPSILON);
            ret * ret
        })
        .sum::<f64>()
        .sqrt();
    let prev_spread = window
        .iter()
        .rev()
        .nth(1)
        .map(|event| event.spread)
        .unwrap_or(last.spread);
    let spread_change = (last.spread - prev_spread) / prev_spread.max(f64::EPSILON);

    Features {
        velocity,
        vol_z: vol_z.clamp(-5.0, 5.0),
        imbalance: last.bid_ask_imbalance.clamp(-1.0, 1.0),
        volatility: volatility.clamp(0.0, 5.0),
        spread: (last.spread + spread_change.abs()).clamp(0.0, 5.0),
    }
}
