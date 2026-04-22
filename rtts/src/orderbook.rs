use crate::types::{BookDelta, BookLevel, OrderBookState, Side, TradeEvent};
use std::collections::{BTreeMap, VecDeque};

const PRICE_SCALE: f64 = 100.0;
const MAX_LEVELS: usize = 32;

#[derive(Clone, Copy, Debug)]
struct BookChange {
    timestamp: u64,
    side: Side,
    price_tick: i64,
    old_qty: f64,
    new_qty: f64,
}

#[derive(Debug)]
pub struct OrderBook {
    bids: BTreeMap<i64, f64>,
    asks: BTreeMap<i64, f64>,
    changes: VecDeque<BookChange>,
    last_state: OrderBookState,
    last_trade_price: f64,
    last_trade_volume: f64,
}

impl Default for OrderBook {
    fn default() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            changes: VecDeque::with_capacity(256),
            last_state: OrderBookState::default(),
            last_trade_price: 0.0,
            last_trade_volume: 0.0,
        }
    }
}

impl OrderBook {
    pub fn apply_delta(&mut self, delta: &BookDelta) -> OrderBookState {
        for level in &delta.bids {
            self.apply_level(delta.timestamp, Side::Buy, *level);
        }
        for level in &delta.asks {
            self.apply_level(delta.timestamp, Side::Sell, *level);
        }
        self.trim_changes(delta.timestamp, 750);
        self.last_state = self.compute_state(delta.timestamp);
        self.last_state.clone()
    }

    pub fn observe_trade(&mut self, trade: &TradeEvent) -> OrderBookState {
        let previous_mid = self.mid_price();
        self.last_trade_price = trade.price;
        self.last_trade_volume = trade.volume;
        let mut state = self.compute_state(trade.timestamp);
        let mid_move = (self.mid_price() - previous_mid).abs();
        let top_qty = match trade.side {
            Side::Buy => self.best_ask_qty(),
            Side::Sell => self.best_bid_qty(),
        };
        state.absorption =
            if trade.volume > top_qty.max(0.01) * 0.8 && mid_move < state.spread * 0.15 {
                1.0
            } else {
                (trade.volume / top_qty.max(0.01)).clamp(0.0, 1.0) * 0.35
            };
        self.last_state = state.clone();
        state
    }

    pub fn state(&self) -> OrderBookState {
        self.last_state.clone()
    }

    fn apply_level(&mut self, timestamp: u64, side: Side, level: BookLevel) {
        if !level.price.is_finite() || !level.quantity.is_finite() {
            return;
        }
        let tick = to_tick(level.price);
        let book = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };
        let old_qty = book.get(&tick).copied().unwrap_or(0.0);
        if level.quantity <= f64::EPSILON {
            book.remove(&tick);
        } else {
            book.insert(tick, level.quantity);
        }
        self.changes.push_back(BookChange {
            timestamp,
            side,
            price_tick: tick,
            old_qty,
            new_qty: level.quantity.max(0.0),
        });
    }

    fn compute_state(&self, now: u64) -> OrderBookState {
        let best_bid_tick = self.bids.keys().next_back().copied().unwrap_or_default();
        let best_ask_tick = self.asks.keys().next().copied().unwrap_or_default();
        let best_bid = from_tick(best_bid_tick);
        let best_ask = from_tick(best_ask_tick);
        let spread = if best_bid > 0.0 && best_ask > best_bid {
            (best_ask - best_bid) / midpoint(best_bid, best_ask).max(f64::EPSILON)
        } else {
            0.0
        };

        let bid_volume = self
            .bids
            .iter()
            .rev()
            .take(MAX_LEVELS)
            .map(|(_, qty)| *qty)
            .sum();
        let ask_volume = self.asks.iter().take(MAX_LEVELS).map(|(_, qty)| *qty).sum();
        let imbalance = normalized_diff(bid_volume, ask_volume);
        let top_pressure = normalized_diff(self.best_bid_qty(), self.best_ask_qty());
        let weighted_imbalance = self.weighted_imbalance();
        let liquidity_clusters = self.liquidity_clusters();
        let spoofing_score = self.spoofing_score(now);
        let liquidity_pull = self.liquidity_pull(now);

        OrderBookState {
            best_bid,
            best_ask,
            bid_volume,
            ask_volume,
            imbalance,
            liquidity_clusters,
            top_pressure,
            weighted_imbalance,
            spread,
            spoofing_score,
            liquidity_pull,
            absorption: self.last_state.absorption * 0.85,
        }
    }

    fn weighted_imbalance(&self) -> f64 {
        let bid_weighted = self
            .bids
            .iter()
            .rev()
            .take(MAX_LEVELS)
            .enumerate()
            .map(|(idx, (_, qty))| qty / (idx as f64 + 1.0))
            .sum::<f64>();
        let ask_weighted = self
            .asks
            .iter()
            .take(MAX_LEVELS)
            .enumerate()
            .map(|(idx, (_, qty))| qty / (idx as f64 + 1.0))
            .sum::<f64>();
        normalized_diff(bid_weighted, ask_weighted)
    }

    fn liquidity_clusters(&self) -> Vec<f64> {
        let mut levels = Vec::with_capacity(6);
        let avg_bid = avg_top_qty(
            self.bids
                .iter()
                .rev()
                .take(MAX_LEVELS)
                .map(|(tick, qty)| (*tick, *qty)),
        );
        let avg_ask = avg_top_qty(
            self.asks
                .iter()
                .take(MAX_LEVELS)
                .map(|(tick, qty)| (*tick, *qty)),
        );
        for (tick, qty) in self.bids.iter().rev().take(MAX_LEVELS) {
            if *qty > avg_bid * 2.5 && levels.len() < 3 {
                levels.push(from_tick(*tick));
            }
        }
        for (tick, qty) in self.asks.iter().take(MAX_LEVELS) {
            if *qty > avg_ask * 2.5 && levels.len() < 6 {
                levels.push(from_tick(*tick));
            }
        }
        levels
    }

    fn spoofing_score(&self, now: u64) -> f64 {
        let cutoff = now.saturating_sub(650);
        let mut rapid_add_remove = 0.0;
        let mut total = 0.0;
        for add in self
            .changes
            .iter()
            .filter(|change| change.timestamp >= cutoff && change.new_qty > change.old_qty * 2.0)
        {
            total += 1.0;
            if self.changes.iter().any(|remove| {
                remove.timestamp > add.timestamp
                    && remove.timestamp.saturating_sub(add.timestamp) < 450
                    && remove.side == add.side
                    && remove.price_tick == add.price_tick
                    && remove.new_qty < add.new_qty * 0.25
            }) {
                rapid_add_remove += 1.0;
            }
        }
        if total > 0.0 {
            rapid_add_remove / total
        } else {
            0.0
        }
    }

    fn liquidity_pull(&self, now: u64) -> f64 {
        let cutoff = now.saturating_sub(350);
        let pulled = self
            .changes
            .iter()
            .filter(|change| change.timestamp >= cutoff && change.old_qty > change.new_qty * 2.5)
            .map(|change| change.old_qty - change.new_qty)
            .sum::<f64>();
        let current = self.last_state.bid_volume + self.last_state.ask_volume;
        (pulled / current.max(1.0)).clamp(0.0, 1.0)
    }

    fn trim_changes(&mut self, now: u64, window_ms: u64) {
        let cutoff = now.saturating_sub(window_ms);
        while self
            .changes
            .front()
            .is_some_and(|change| change.timestamp < cutoff)
        {
            self.changes.pop_front();
        }
    }

    fn mid_price(&self) -> f64 {
        midpoint(self.last_state.best_bid, self.last_state.best_ask)
    }

    fn best_bid_qty(&self) -> f64 {
        self.bids
            .iter()
            .next_back()
            .map(|(_, qty)| *qty)
            .unwrap_or(0.0)
    }

    fn best_ask_qty(&self) -> f64 {
        self.asks.iter().next().map(|(_, qty)| *qty).unwrap_or(0.0)
    }
}

fn avg_top_qty<I>(iter: I) -> f64
where
    I: Iterator<Item = (i64, f64)>,
{
    let mut sum: f64 = 0.0;
    let mut count: f64 = 0.0;
    for (_, qty) in iter {
        sum += qty;
        count += 1.0;
    }
    sum / count.max(1.0)
}

#[inline]
fn to_tick(price: f64) -> i64 {
    (price * PRICE_SCALE).round() as i64
}

#[inline]
fn from_tick(tick: i64) -> f64 {
    tick as f64 / PRICE_SCALE
}

#[inline]
fn midpoint(best_bid: f64, best_ask: f64) -> f64 {
    if best_bid > 0.0 && best_ask > 0.0 {
        (best_bid + best_ask) * 0.5
    } else {
        best_bid.max(best_ask)
    }
}

#[inline]
fn normalized_diff(lhs: f64, rhs: f64) -> f64 {
    (lhs - rhs) / (lhs + rhs).max(f64::EPSILON)
}
