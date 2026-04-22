use crate::{
    config::{Config, Exchange},
    metrics::Metrics,
    types::{MarketEvent, Side},
};
use anyhow::{Context, Result};
use futures_util::StreamExt;
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde::Deserialize;
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::connect_async;
use tracing::{info, warn};

pub async fn run(cfg: Config, tx: Sender<MarketEvent>, metrics: Arc<Metrics>) -> Result<()> {
    match cfg.exchange {
        Exchange::Mock => mock_feed(tx, metrics).await,
        Exchange::Binance => binance_feed(cfg, tx, metrics).await,
    }
}

async fn mock_feed(tx: Sender<MarketEvent>, metrics: Arc<Metrics>) -> Result<()> {
    let mut rng = StdRng::seed_from_u64(42);
    let mut price: f64 = 67_000.0;
    let mut interval = tokio::time::interval(Duration::from_millis(10));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        interval.tick().await;
        let impulse = if rng.gen_bool(0.04) {
            rng.gen_range(-18.0..18.0)
        } else {
            rng.gen_range(-2.5..2.5)
        };
        price = (price + impulse).max(1.0);
        let side = if impulse >= 0.0 {
            Side::Buy
        } else {
            Side::Sell
        };
        let event = MarketEvent {
            timestamp: now_ms(),
            price,
            volume: rng.gen_range(0.02..1.6),
            side,
            bid_ask_imbalance: rng.gen_range(-0.85..0.85),
            spread: rng.gen_range(0.00001..0.00045),
        };
        send_market_event(&tx, event, &metrics, "ingestion").await?;
    }
}

async fn binance_feed(cfg: Config, tx: Sender<MarketEvent>, metrics: Arc<Metrics>) -> Result<()> {
    let symbol = cfg.symbol.to_lowercase();
    let url = format!(
        "wss://stream.binance.com:9443/stream?streams={}@aggTrade/{}@depth5@100ms",
        symbol, symbol
    );
    let mut best_bid = 0.0;
    let mut best_ask = 0.0;
    let mut bid_qty = 0.0;
    let mut ask_qty = 0.0;

    loop {
        info!(url, "connecting websocket");
        let (stream, _) = connect_async(&url)
            .await
            .context("binance websocket connect")?;
        let (_, mut read) = stream.split();

        while let Some(message) = read.next().await {
            let message = match message {
                Ok(message) if message.is_text() => message,
                Ok(_) => continue,
                Err(err) => {
                    warn!(%err, "websocket read error");
                    break;
                }
            };

            let envelope: CombinedEnvelope = match serde_json::from_str(message.to_text()?) {
                Ok(envelope) => envelope,
                Err(err) => {
                    warn!(%err, "failed to parse binance envelope");
                    continue;
                }
            };

            match envelope.data {
                BinancePayload::AggTrade(trade) => {
                    let spread = if best_bid > 0.0 && best_ask > 0.0 {
                        (best_ask - best_bid) / trade.price.max(f64::EPSILON)
                    } else {
                        0.0
                    };
                    let imbalance = if bid_qty + ask_qty > f64::EPSILON {
                        (bid_qty - ask_qty) / (bid_qty + ask_qty)
                    } else {
                        0.0
                    };
                    let side = if trade.buyer_is_maker {
                        Side::Sell
                    } else {
                        Side::Buy
                    };
                    let event = MarketEvent {
                        timestamp: trade.event_time,
                        price: trade.price,
                        volume: trade.quantity,
                        side,
                        bid_ask_imbalance: imbalance,
                        spread,
                    };
                    send_market_event(&tx, event, &metrics, "ingestion").await?;
                }
                BinancePayload::Depth(depth) => {
                    if let Some((price, qty)) = depth.bids.first() {
                        best_bid = *price;
                        bid_qty = *qty;
                    }
                    if let Some((price, qty)) = depth.asks.first() {
                        best_ask = *price;
                        ask_qty = *qty;
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn send_market_event(
    tx: &Sender<MarketEvent>,
    event: MarketEvent,
    metrics: &Metrics,
    stage: &'static str,
) -> Result<()> {
    metrics.events_total.with_label_values(&[stage]).inc();
    if tx.try_send(event.clone()).is_err() {
        metrics
            .channel_backpressure_total
            .with_label_values(&[stage])
            .inc();
        tx.send(event).await.context("market channel closed")?;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct CombinedEnvelope {
    #[allow(dead_code)]
    stream: String,
    data: BinancePayload,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BinancePayload {
    AggTrade(AggTrade),
    Depth(Depth),
}

#[derive(Debug, Deserialize)]
struct AggTrade {
    #[serde(rename = "E")]
    event_time: u64,
    #[serde(rename = "p", deserialize_with = "de_f64_str")]
    price: f64,
    #[serde(rename = "q", deserialize_with = "de_f64_str")]
    quantity: f64,
    #[serde(rename = "m")]
    buyer_is_maker: bool,
}

#[derive(Debug, Deserialize)]
struct Depth {
    #[serde(rename = "b", deserialize_with = "de_book")]
    bids: Vec<(f64, f64)>,
    #[serde(rename = "a", deserialize_with = "de_book")]
    asks: Vec<(f64, f64)>,
}

fn de_f64_str<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    value.parse::<f64>().map_err(serde::de::Error::custom)
}

fn de_book<'de, D>(deserializer: D) -> Result<Vec<(f64, f64)>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Vec::<[String; 2]>::deserialize(deserializer)?;
    raw.into_iter()
        .map(|[price, qty]| {
            Ok((
                price.parse::<f64>().map_err(serde::de::Error::custom)?,
                qty.parse::<f64>().map_err(serde::de::Error::custom)?,
            ))
        })
        .collect()
}

#[inline]
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
