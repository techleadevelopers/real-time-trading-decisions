use crate::{
    metrics::Metrics,
    orderbook::OrderBook,
    tape::Tape,
    types::{Features, MarketRegime, MarketUpdate, MicrostructureFrame, OrderBookState, TapeState},
};
use std::{sync::Arc, time::Instant};
use tokio::sync::mpsc::{Receiver, Sender};

#[derive(Debug, Default)]
struct EmaStats {
    mean: f64,
    var: f64,
    initialized: bool,
}

impl EmaStats {
    fn normalize(&mut self, value: f64, alpha: f64) -> f64 {
        if !value.is_finite() {
            return 0.0;
        }
        if !self.initialized {
            self.mean = value;
            self.var = 1.0;
            self.initialized = true;
            return 0.0;
        }
        let diff = value - self.mean;
        self.mean += alpha * diff;
        self.var = (1.0 - alpha) * (self.var + alpha * diff * diff);
        ((value - self.mean) / self.var.sqrt().max(1e-9)).clamp(-5.0, 5.0)
    }
}

#[derive(Debug, Default)]
struct FeatureNormalizer {
    velocity: EmaStats,
    volume: EmaStats,
    volatility: EmaStats,
    spread: EmaStats,
    spread_dynamics: EmaStats,
    trade_clustering: EmaStats,
    liquidity_shift: EmaStats,
    delta: EmaStats,
    previous_mid: f64,
    previous_spread: f64,
    previous_depth: f64,
}

pub async fn run(
    window_ms: u64,
    max_data_age_ms: u64,
    mut rx: Receiver<MarketUpdate>,
    tx: Sender<MicrostructureFrame>,
    metrics: Arc<Metrics>,
) {
    let mut book = OrderBook::default();
    let mut tape = Tape::new(window_ms);
    let mut normalizer = FeatureNormalizer::default();

    while let Some(update) = rx.recv().await {
        let started = Instant::now();
        let (timestamp, trade, book_state, tape_state) = match update {
            MarketUpdate::BookDelta(delta) => {
                let timestamp = delta.timestamp;
                let book_state = book.apply_delta(&delta);
                (timestamp, None, book_state, tape.state())
            }
            MarketUpdate::Trade(trade) => {
                let timestamp = trade.timestamp;
                let tape_state = tape.observe(trade);
                let book_state = book.observe_trade(&trade);
                (timestamp, Some(trade), book_state, tape_state)
            }
        };

        let features = normalizer.features(&book_state, &tape_state, timestamp);
        let regime = normalizer.regime(&features, &book_state);
        let stale = data_age_ms(timestamp) > max_data_age_ms;
        let output = MicrostructureFrame {
            timestamp,
            trade,
            book: book_state,
            tape: tape_state,
            features,
            regime,
            stale,
        };

        metrics
            .events_total
            .with_label_values(&["microstructure"])
            .inc();
        metrics
            .stage_latency_us
            .with_label_values(&["microstructure"])
            .observe(started.elapsed().as_micros() as f64);

        if tx.try_send(output.clone()).is_err() {
            metrics
                .channel_backpressure_total
                .with_label_values(&["microstructure"])
                .inc();
            if tx.send(output).await.is_err() {
                break;
            }
        }
    }
}

impl FeatureNormalizer {
    fn features(&mut self, book: &OrderBookState, tape: &TapeState, timestamp: u64) -> Features {
        let mid = if book.best_bid > 0.0 && book.best_ask > 0.0 {
            (book.best_bid + book.best_ask) * 0.5
        } else {
            tape.last_price
        };
        let raw_velocity = if self.previous_mid > 0.0 {
            (mid - self.previous_mid) / self.previous_mid.max(f64::EPSILON)
        } else {
            0.0
        };
        self.previous_mid = mid;

        let spread_dynamics = if self.previous_spread > 0.0 {
            (book.spread - self.previous_spread) / self.previous_spread.max(f64::EPSILON)
        } else {
            0.0
        };
        self.previous_spread = book.spread;

        let depth = book.bid_volume + book.ask_volume;
        let liquidity_shift = if self.previous_depth > 0.0 {
            (depth - self.previous_depth) / self.previous_depth.max(f64::EPSILON)
        } else {
            0.0
        };
        self.previous_depth = depth;

        let alpha = if timestamp % 2 == 0 { 0.035 } else { 0.030 };
        Features {
            velocity: self.velocity.normalize(raw_velocity * 10_000.0, alpha),
            vol_z: self.volume.normalize(tape.volume_burst, alpha),
            imbalance: book.imbalance,
            volatility: self
                .volatility
                .normalize(raw_velocity.abs() * 10_000.0, alpha)
                .abs(),
            spread: self
                .spread
                .normalize(book.spread * 10_000.0, alpha)
                .max(0.0),
            weighted_imbalance: book.weighted_imbalance,
            spread_dynamics: self.spread_dynamics.normalize(spread_dynamics, alpha),
            micro_price_velocity: self.velocity.normalize(raw_velocity * 100_000.0, alpha),
            trade_clustering: self.trade_clustering.normalize(tape.trade_frequency, alpha),
            liquidity_shift: self.liquidity_shift.normalize(liquidity_shift, alpha),
            order_flow_delta: self.delta.normalize(tape.delta, alpha),
            absorption: book.absorption.max(tape.exhaustion).clamp(0.0, 1.0),
            spoofing_risk: book.spoofing_score.clamp(0.0, 1.0),
            liquidity_pull: book.liquidity_pull.clamp(0.0, 1.0),
        }
    }

    fn regime(&self, features: &Features, book: &OrderBookState) -> MarketRegime {
        MarketRegime {
            volatility: features.volatility.abs().clamp(0.0, 5.0),
            spread: (book.spread * 10_000.0).clamp(0.0, 100.0),
            trend_strength: (features.velocity.abs()
                + features.micro_price_velocity.abs()
                + features.order_flow_delta.abs())
                / 3.0,
        }
    }
}

fn data_age_ms(timestamp: u64) -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(timestamp);
    now.saturating_sub(timestamp)
}
