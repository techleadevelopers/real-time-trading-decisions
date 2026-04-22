use crate::{
    metrics::Metrics,
    types::{DetectedEvent, Event, MarketEvent, Side},
};
use std::{collections::VecDeque, sync::Arc, time::Instant};
use tokio::sync::mpsc::{Receiver, Sender};

pub async fn run(
    window_ms: u64,
    mut rx: Receiver<MarketEvent>,
    tx: Sender<DetectedEvent>,
    metrics: Arc<Metrics>,
) {
    let mut window = VecDeque::<MarketEvent>::with_capacity(256);
    while let Some(market) = rx.recv().await {
        let started = Instant::now();
        trim_window(&mut window, market.timestamp, window_ms);
        window.push_back(market.clone());
        let event = classify(&window);
        metrics.events_total.with_label_values(&["event"]).inc();
        metrics
            .stage_latency_us
            .with_label_values(&["event"])
            .observe(started.elapsed().as_micros() as f64);

        let output = DetectedEvent { market, event };
        if tx.try_send(output.clone()).is_err() {
            metrics
                .channel_backpressure_total
                .with_label_values(&["event"])
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

fn classify(window: &VecDeque<MarketEvent>) -> Event {
    if window.len() < 4 {
        return Event::Neutral;
    }

    let first = window.front().expect("non-empty window");
    let last = window.back().expect("non-empty window");
    let elapsed_ms = last.timestamp.saturating_sub(first.timestamp).max(1) as f64;
    let price_velocity = (last.price - first.price) / first.price.max(f64::EPSILON) / elapsed_ms;
    let mut buy_volume = 0.0;
    let mut sell_volume = 0.0;
    for event in window {
        match event.side {
            Side::Buy => buy_volume += event.volume,
            Side::Sell => sell_volume += event.volume,
        }
    }
    let total_volume = (buy_volume + sell_volume).max(f64::EPSILON);
    let aggressive_buy_pressure = buy_volume / total_volume;
    let aggressive_sell_pressure = sell_volume / total_volume;
    let spread_tight = last.spread < first.spread * 0.85;
    let liquidity_thin = last.bid_ask_imbalance.abs() > 0.55 || last.spread > first.spread * 1.35;

    if aggressive_sell_pressure > 0.68 && price_velocity < -0.000_000_35 && liquidity_thin {
        Event::DumpDetected
    } else if aggressive_buy_pressure > 0.68 && price_velocity > 0.000_000_35 && spread_tight {
        Event::PumpDetected
    } else {
        Event::Neutral
    }
}
