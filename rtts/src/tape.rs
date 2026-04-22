use crate::types::{Side, TapeState, TradeEvent};
use std::collections::VecDeque;

#[derive(Debug)]
pub struct Tape {
    window_ms: u64,
    trades: VecDeque<TradeEvent>,
    last_state: TapeState,
}

impl Tape {
    pub fn new(window_ms: u64) -> Self {
        Self {
            window_ms,
            trades: VecDeque::with_capacity(512),
            last_state: TapeState::default(),
        }
    }

    pub fn observe(&mut self, trade: TradeEvent) -> TapeState {
        self.trim(trade.timestamp);
        self.trades.push_back(trade);
        self.last_state = self.compute();
        self.last_state.clone()
    }

    pub fn state(&self) -> TapeState {
        self.last_state.clone()
    }

    fn compute(&self) -> TapeState {
        let Some(first) = self.trades.front() else {
            return TapeState::default();
        };
        let last = self.trades.back().expect("non-empty tape");
        let elapsed_ms = last.timestamp.saturating_sub(first.timestamp).max(1) as f64;
        let mut buy_volume = 0.0;
        let mut sell_volume = 0.0;
        for trade in &self.trades {
            match trade.side {
                Side::Buy => buy_volume += trade.volume,
                Side::Sell => sell_volume += trade.volume,
            }
        }

        let delta = buy_volume - sell_volume;
        let trade_frequency = self.trades.len() as f64 / elapsed_ms * 1_000.0;
        let mean_volume = (buy_volume + sell_volume) / self.trades.len().max(1) as f64;
        let volume_burst = (last.volume / mean_volume.max(f64::EPSILON)).clamp(0.0, 10.0);
        let price_move = (last.price - first.price) / first.price.max(f64::EPSILON);
        let aggression = delta.abs() / (buy_volume + sell_volume).max(f64::EPSILON);
        let exhaustion = if aggression > 0.65 && price_move.abs() < 0.00008 {
            aggression
        } else {
            0.0
        };
        let continuation = if price_move.signum() == delta.signum() && aggression > 0.25 {
            (aggression * price_move.abs() * 10_000.0).clamp(0.0, 1.0)
        } else {
            0.0
        };

        TapeState {
            buy_volume,
            sell_volume,
            delta,
            trade_frequency,
            volume_burst,
            exhaustion,
            continuation,
            last_price: last.price,
        }
    }

    fn trim(&mut self, now: u64) {
        let cutoff = now.saturating_sub(self.window_ms);
        while self
            .trades
            .front()
            .is_some_and(|trade| trade.timestamp < cutoff)
        {
            self.trades.pop_front();
        }
    }
}
