use crate::types::{CompetitionState, QueueEstimate};

const EMA_ALPHA_FAST: f64 = 0.22;
const EMA_ALPHA_SLOW: f64 = 0.10;

#[derive(Clone, Debug, Default)]
pub struct QueueState {
    pub volume_ahead: f64,
    pub queue_position_ratio: f64,
    pub expected_fill_time_ms: f64,
    pub fill_probability: f64,
    pub queue_decay_rate: f64,
    pub competition_state: CompetitionState,
    pub best_bid: f64,
    pub best_ask: f64,
    pub last_reference_price: f64,
}

#[derive(Clone, Debug, Default)]
pub struct QueueEngine {
    state: QueueState,
    cancel_rate: f64,
    trade_through_rate: f64,
    last_volume_ahead: f64,
    last_visible_volume: f64,
    last_ts: u64,
}

impl QueueEngine {
    pub fn new(estimate: QueueEstimate, order_qty: f64, expected_duration_ms: u64) -> Self {
        let volume_ahead = estimate.volume_ahead.max(order_qty);
        let queue_position_ratio = if volume_ahead > 0.0 {
            estimate.volume_ahead / volume_ahead.max(1e-9)
        } else {
            1.0
        };
        Self {
            state: QueueState {
                volume_ahead,
                queue_position_ratio,
                expected_fill_time_ms: expected_duration_ms as f64 / estimate.fill_probability.max(0.05),
                fill_probability: estimate.fill_probability.clamp(0.0, 1.0),
                queue_decay_rate: 0.0,
                competition_state: CompetitionState::Normal,
                best_bid: 0.0,
                best_ask: 0.0,
                last_reference_price: 0.0,
            },
            cancel_rate: 0.0,
            trade_through_rate: 0.0,
            last_volume_ahead: volume_ahead,
            last_visible_volume: volume_ahead,
            last_ts: 0,
        }
    }

    pub fn observe_book(
        &mut self,
        visible_volume: f64,
        order_qty: f64,
        outbid: bool,
        spread_bps: f64,
        now_ts: u64,
    ) -> QueueState {
        let elapsed_ms = now_ts.saturating_sub(self.last_ts).max(1) as f64;
        let visible_volume = visible_volume.max(order_qty);
        let volume_delta = self.last_visible_volume - visible_volume;
        let cancel_impulse = (volume_delta / self.last_visible_volume.max(1e-9)).clamp(-1.0, 1.0);
        self.cancel_rate = ewma(
            self.cancel_rate,
            cancel_impulse.max(0.0),
            EMA_ALPHA_SLOW,
        );

        let mut volume_ahead = visible_volume;
        if outbid {
            volume_ahead = (visible_volume + order_qty).max(self.state.volume_ahead);
        }
        let decay = ((self.last_volume_ahead - volume_ahead) / elapsed_ms).clamp(-10_000.0, 10_000.0);
        self.state.queue_decay_rate = ewma(self.state.queue_decay_rate, decay, EMA_ALPHA_FAST);
        self.state.volume_ahead = volume_ahead.max(order_qty);
        self.state.queue_position_ratio =
            (self.state.volume_ahead / visible_volume.max(1e-9)).clamp(0.0, 8.0);

        let spread_penalty = (spread_bps / 12.0).clamp(0.0, 0.45);
        let outbid_penalty = if outbid { 0.14 } else { 0.0 };
        let queue_penalty = (self.state.queue_position_ratio - 1.0).max(0.0) * 0.28;
        let decay_boost = (self.state.queue_decay_rate.max(0.0) / order_qty.max(1e-9) * 50.0).clamp(0.0, 0.20);
        let cancel_boost = self.cancel_rate.clamp(0.0, 0.18);
        let trade_through_boost = self.trade_through_rate.clamp(0.0, 0.20);
        self.state.fill_probability = (0.55 + decay_boost + cancel_boost + trade_through_boost
            - queue_penalty - spread_penalty - outbid_penalty)
            .clamp(0.0, 1.0);
        self.state.expected_fill_time_ms = expected_fill_time_ms(
            self.state.volume_ahead,
            order_qty,
            self.state.fill_probability,
            self.state.queue_decay_rate,
        );

        self.last_ts = now_ts;
        self.last_visible_volume = visible_volume;
        self.last_volume_ahead = self.state.volume_ahead;
        self.state.clone()
    }

    pub fn observe_trade(
        &mut self,
        traded_volume: f64,
        order_qty: f64,
        now_ts: u64,
    ) -> QueueState {
        let elapsed_ms = now_ts.saturating_sub(self.last_ts).max(1) as f64;
        let trade_impulse =
            (traded_volume / self.state.volume_ahead.max(order_qty).max(1e-9)).clamp(0.0, 1.0);
        self.trade_through_rate = ewma(self.trade_through_rate, trade_impulse, EMA_ALPHA_FAST);
        let consumed = traded_volume.min(self.state.volume_ahead);
        self.state.volume_ahead = (self.state.volume_ahead - consumed).max(order_qty);
        let decay = consumed / elapsed_ms;
        self.state.queue_decay_rate = ewma(self.state.queue_decay_rate, decay, EMA_ALPHA_FAST);
        self.state.queue_position_ratio =
            (self.state.volume_ahead / self.last_visible_volume.max(order_qty).max(1e-9)).clamp(0.0, 8.0);
        self.state.fill_probability = (self.state.fill_probability + trade_impulse * 0.18).clamp(0.0, 1.0);
        self.state.expected_fill_time_ms = expected_fill_time_ms(
            self.state.volume_ahead,
            order_qty,
            self.state.fill_probability,
            self.state.queue_decay_rate,
        );
        self.last_ts = now_ts;
        self.last_volume_ahead = self.state.volume_ahead;
        self.state.clone()
    }

    pub fn cancel_rate(&self) -> f64 {
        self.cancel_rate
    }

    pub fn trade_through_rate(&self) -> f64 {
        self.trade_through_rate
    }

    pub fn state(&self) -> QueueState {
        self.state.clone()
    }
}

fn expected_fill_time_ms(
    volume_ahead: f64,
    order_qty: f64,
    fill_probability: f64,
    queue_decay_rate: f64,
) -> f64 {
    let effective_decay = queue_decay_rate.max(order_qty * 0.001).max(1e-6);
    let backlog_ms = volume_ahead / effective_decay;
    (backlog_ms / fill_probability.max(0.05)).clamp(10.0, 2_000.0)
}

fn ewma(current: f64, sample: f64, alpha: f64) -> f64 {
    if current == 0.0 {
        sample
    } else {
        current * (1.0 - alpha) + sample * alpha
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_fill_probability_improves_when_volume_clears() {
        let mut engine = QueueEngine::new(
            QueueEstimate {
                queue_position: 1.0,
                volume_ahead: 5.0,
                fill_probability: 0.30,
                placement_depth_bps: 0.5,
            },
            1.0,
            500,
        );
        let before = engine.state().fill_probability;
        let after = engine.observe_trade(2.0, 1.0, 50);
        assert!(after.fill_probability > before);
        assert!(after.expected_fill_time_ms < 2_000.0);
    }
}
