use crate::{
    accounting::edge_validation::EdgeState,
    config::Config,
    metrics::Metrics,
    types::{
        CompetitionFlag, CompetitionState, ExecutionMode, FillProbabilityClass, MarketUpdate,
        OrderIntent, QueueEstimate, ScoredDecision, Side,
    },
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::mpsc::{Receiver, Sender};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionStrategy {
    Passive,
    Aggressive,
    Defensive,
}

impl From<ExecutionMode> for ExecutionStrategy {
    fn from(value: ExecutionMode) -> Self {
        match value {
            ExecutionMode::Passive => Self::Passive,
            ExecutionMode::Aggressive => Self::Aggressive,
            ExecutionMode::Defensive => Self::Defensive,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct QueueState {
    pub queue_position: f64,
    pub volume_ahead: f64,
    pub fill_probability: f64,
    pub competition_state: CompetitionState,
    pub best_bid: f64,
    pub best_ask: f64,
    pub last_reference_price: f64,
    pub liquidity_pull_score: f64,
    pub outbid_count: u32,
}

#[derive(Clone, Debug, Default)]
pub struct FillProgress {
    pub filled_qty: f64,
    pub remaining_qty: f64,
    pub fill_rate: f64,
    pub last_fill_ts: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExecutionAction {
    Hold,
    Cancel,
    Replace { new_price: f64 },
    SwitchStrategy { new_strategy: ExecutionStrategy },
    Abort,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionFailureReason {
    QueueTooDeep,
    Outbid,
    LatencyTooHigh,
    CompetitionSpike,
    LiquidityPull,
    NoFillTimeout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderLifecycleState {
    New,
    Sent,
    Ack,
    Partial,
    Filled,
    Canceled,
    Rejected,
}

impl OrderLifecycleState {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Filled | Self::Canceled | Self::Rejected)
    }
}

#[derive(Clone, Debug)]
pub struct ExecutionControllerEvent {
    pub order_id: String,
    pub idempotency_key: Option<String>,
    pub symbol: String,
    pub status: OrderLifecycleState,
    pub filled_qty_delta: f64,
    pub cumulative_filled_qty: f64,
    pub remaining_qty: f64,
    pub partial_fill_ratio: f64,
    pub slippage_bps: f64,
    pub competition_flag: CompetitionFlag,
    pub latency_us: u64,
    pub event_ts: u64,
}

#[derive(Clone, Debug)]
pub struct ExecutionControlFeedback {
    pub order_id: String,
    pub symbol: String,
    pub reason: ExecutionFailureReason,
    pub elapsed_ms: f64,
    pub fill_ratio: f64,
    pub expected_fill_time_ms: f64,
    pub edge_half_life_ms: f64,
    pub aborted_due_to_decay: bool,
}

#[derive(Clone, Debug)]
pub enum ExecutionInstruction {
    Submit { intent: OrderIntent, idempotency_key: String },
    Cancel { order_id: String, symbol: String, reason: &'static str },
    Replace {
        order_id: String,
        symbol: String,
        new_price: f64,
        reason: &'static str,
    },
    SwitchStrategy {
        order_id: String,
        symbol: String,
        new_strategy: ExecutionStrategy,
        price: Option<f64>,
        reason: &'static str,
    },
    Abort { order_id: String, symbol: String, reason: &'static str },
}

#[derive(Clone, Debug)]
pub struct ExecutionController {
    pub order_id: String,
    pub symbol: String,
    pub side: Side,
    pub initial_decision: ScoredDecision,
    pub current_strategy: ExecutionStrategy,
    pub queue_state: QueueState,
    pub fill_state: FillProgress,
    pub start_ts: u64,
    pub last_update_ts: u64,
    pub expected_fill_time_ms: f64,
    pub edge_half_life_ms: f64,
    pub cancel_count: u32,
    pub replace_count: u32,
    idempotency_key: String,
    current_price: Option<f64>,
    status: OrderLifecycleState,
    initial_volume_ahead: f64,
    last_action_ts: u64,
    last_latency_us: u64,
    last_slippage_bps: f64,
    action_cooldown_ms: u64,
    max_cancel_per_order: u32,
    max_replace_per_order: u32,
    queue_replace_volume_factor: f64,
    min_fill_probability: f64,
}

impl ExecutionController {
    pub fn new(intent: &OrderIntent, cfg: &Config) -> Self {
        let idempotency_key = idempotency_key(intent);
        let expected_fill_time_ms = expected_fill_time_ms(intent);
        let edge_half_life_ms = edge_half_life_ms(intent);
        let initial_volume_ahead = intent.queue_estimate.volume_ahead.max(intent.request.size);
        Self {
            order_id: idempotency_key.clone(),
            symbol: intent.request.symbol.clone(),
            side: intent.request.side,
            initial_decision: scored_decision_from_intent(intent),
            current_strategy: ExecutionStrategy::from(intent.execution_mode),
            queue_state: QueueState {
                queue_position: intent.queue_estimate.queue_position,
                volume_ahead: initial_volume_ahead,
                fill_probability: queue_fill_probability(intent.queue_estimate),
                competition_state: intent.competition_state,
                best_bid: 0.0,
                best_ask: 0.0,
                last_reference_price: intent.request.price.unwrap_or(intent.last_price),
                liquidity_pull_score: 0.0,
                outbid_count: 0,
            },
            fill_state: FillProgress {
                filled_qty: 0.0,
                remaining_qty: intent.request.size,
                fill_rate: 0.0,
                last_fill_ts: None,
            },
            start_ts: intent.timestamp,
            last_update_ts: intent.timestamp,
            expected_fill_time_ms,
            edge_half_life_ms,
            cancel_count: 0,
            replace_count: 0,
            idempotency_key,
            current_price: intent.request.price,
            status: OrderLifecycleState::New,
            initial_volume_ahead,
            last_action_ts: 0,
            last_latency_us: 0,
            last_slippage_bps: 0.0,
            action_cooldown_ms: cfg.execution_action_cooldown_ms,
            max_cancel_per_order: cfg.max_cancel_per_order,
            max_replace_per_order: cfg.max_replace_per_order,
            queue_replace_volume_factor: cfg.queue_replace_volume_factor.max(1.05),
            min_fill_probability: cfg.min_fill_probability.clamp(0.05, 0.95),
        }
    }

    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }

    pub fn is_terminal(&self) -> bool {
        self.status.is_terminal()
    }

    pub fn on_market_update(&mut self, update: &MarketUpdate) {
        match update {
            MarketUpdate::BookDelta(book) => {
                let Some(best_bid) = book.bids.first() else {
                    return;
                };
                let Some(best_ask) = book.asks.first() else {
                    return;
                };
                self.last_update_ts = book.timestamp;
                self.queue_state.best_bid = best_bid.price;
                self.queue_state.best_ask = best_ask.price;
                self.queue_state.last_reference_price = match self.side {
                    Side::Buy => best_bid.price,
                    Side::Sell => best_ask.price,
                };
                let visible_volume = match self.side {
                    Side::Buy => best_bid.quantity,
                    Side::Sell => best_ask.quantity,
                };
                if let Some(current_price) = self.current_price {
                    let outbid = match self.side {
                        Side::Buy => best_bid.price > current_price,
                        Side::Sell => best_ask.price < current_price,
                    };
                    if outbid {
                        self.queue_state.outbid_count = self.queue_state.outbid_count.saturating_add(1);
                    }
                }
                self.queue_state.volume_ahead = visible_volume.max(self.fill_state.remaining_qty);
                self.queue_state.queue_position =
                    self.queue_state.volume_ahead / self.fill_state.remaining_qty.max(1e-9);
                let spread = (best_ask.price - best_bid.price).max(0.0);
                self.queue_state.liquidity_pull_score = ((self.initial_volume_ahead - visible_volume)
                    / self.initial_volume_ahead.max(1e-9))
                    .clamp(0.0, 1.0)
                    + (spread / self.queue_state.last_reference_price.max(1e-9) * 10_000.0 / 12.0)
                        .clamp(0.0, 1.0)
                        * 0.25;
                self.queue_state.fill_probability = self.estimate_fill_probability();
                self.queue_state.competition_state = classify_competition_state(
                    self.queue_state.fill_probability,
                    self.queue_state.outbid_count,
                    self.queue_state.liquidity_pull_score,
                );
            }
            MarketUpdate::Trade(trade) => {
                self.last_update_ts = trade.timestamp;
            }
        }
    }

    pub fn on_execution_event(&mut self, event: &ExecutionControllerEvent) {
        self.last_update_ts = event.event_ts;
        self.order_id = event.order_id.clone();
        self.status = event.status;
        self.last_latency_us = event.latency_us;
        self.last_slippage_bps = event.slippage_bps;
        self.queue_state.competition_state =
            competition_state_from_flag(event.competition_flag, self.queue_state.competition_state);
        if event.filled_qty_delta > 0.0 {
            self.fill_state.filled_qty = event.cumulative_filled_qty.max(self.fill_state.filled_qty);
            self.fill_state.remaining_qty = event.remaining_qty.max(0.0);
            self.fill_state.last_fill_ts = Some(event.event_ts);
            let elapsed_ms = (event.event_ts.saturating_sub(self.start_ts)).max(1) as f64;
            self.fill_state.fill_rate = self.fill_state.filled_qty / elapsed_ms;
            self.queue_state.fill_probability = self.estimate_fill_probability();
        }
    }

    pub fn evaluate_action(&self, now_ts: u64) -> ExecutionAction {
        if self.status.is_terminal() {
            return ExecutionAction::Hold;
        }
        if !self.cooldown_elapsed(now_ts) {
            return ExecutionAction::Hold;
        }
        if self.initial_decision.edge_state == EdgeState::Invalid
            || self.queue_state.competition_state == CompetitionState::Saturated
            || self.elapsed_time_ms(now_ts) > self.edge_half_life_ms
        {
            return ExecutionAction::Abort;
        }
        if self.expected_fill_time_ms > self.edge_half_life_ms {
            if self.should_switch_strategy(now_ts).is_some() {
                return ExecutionAction::SwitchStrategy {
                    new_strategy: ExecutionStrategy::Aggressive,
                };
            }
            return ExecutionAction::Abort;
        }
        if let Some(new_strategy) = self.should_switch_strategy(now_ts) {
            return ExecutionAction::SwitchStrategy { new_strategy };
        }
        if self.should_replace(now_ts) {
            if let Some(new_price) = self.compute_improved_price() {
                return ExecutionAction::Replace { new_price };
            }
        }
        if self.should_cancel(now_ts) {
            return ExecutionAction::Cancel;
        }
        ExecutionAction::Hold
    }

    pub fn should_replace(&self, now_ts: u64) -> bool {
        if self.current_strategy != ExecutionStrategy::Passive
            || self.replace_count >= self.max_replace_per_order
            || self.fill_state.remaining_qty <= f64::EPSILON
        {
            return false;
        }
        let queue_deteriorated = self.queue_state.queue_position > 1.4
            || self.queue_state.volume_ahead
                > self.initial_volume_ahead * self.queue_replace_volume_factor;
        let fill_prob_dropped = self.queue_state.fill_probability < self.min_fill_probability;
        let competition_spike = matches!(
            self.queue_state.competition_state,
            CompetitionState::Competitive | CompetitionState::Saturated
        );
        let time_pressure = self.elapsed_time_ms(now_ts) > self.expected_fill_time_ms * 0.65;
        (queue_deteriorated || fill_prob_dropped || competition_spike) && time_pressure
    }

    pub fn compute_improved_price(&self) -> Option<f64> {
        let reference = self.current_price.or(Some(self.queue_state.last_reference_price))?;
        let touch = match self.side {
            Side::Buy => self.queue_state.best_ask.max(reference),
            Side::Sell => {
                let best_bid = if self.queue_state.best_bid > 0.0 {
                    self.queue_state.best_bid
                } else {
                    reference
                };
                best_bid.min(reference)
            }
        };
        let tick = derived_tick(self.queue_state.best_bid, self.queue_state.best_ask, reference);
        let weak_edge = self.initial_decision.edge_reliability_score < 0.60
            || self.initial_decision.score < 0.68;
        let improvement = if weak_edge { tick * 0.5 } else { tick };
        let improved = match self.side {
            Side::Buy => (reference + improvement).min(touch),
            Side::Sell => (reference - improvement).max(touch),
        };
        if (improved - reference).abs() <= f64::EPSILON {
            None
        } else {
            Some(improved)
        }
    }

    pub fn should_switch_strategy(&self, now_ts: u64) -> Option<ExecutionStrategy> {
        let time_remaining = (self.edge_half_life_ms - self.elapsed_time_ms(now_ts)).max(0.0);
        let strong_edge =
            self.initial_decision.score > 0.78 && self.initial_decision.edge_reliability_score > 0.60;
        if self.current_strategy == ExecutionStrategy::Passive
            && self.queue_state.fill_probability < self.min_fill_probability * 0.80
            && strong_edge
            && time_remaining < self.expected_fill_time_ms.max(1.0)
        {
            return Some(ExecutionStrategy::Aggressive);
        }
        if self.current_strategy == ExecutionStrategy::Aggressive
            && self.last_slippage_bps > self.initial_decision.expected_slippage_bps * 1.35
            && time_remaining > self.expected_fill_time_ms * 0.75
        {
            return Some(ExecutionStrategy::Passive);
        }
        if self.queue_state.competition_state == CompetitionState::Saturated
            || self.initial_decision.edge_state == EdgeState::Invalid
        {
            return Some(ExecutionStrategy::Defensive);
        }
        None
    }

    pub fn classify_failure(&self, now_ts: u64) -> ExecutionFailureReason {
        if self.elapsed_time_ms(now_ts) > self.edge_half_life_ms
            || self.elapsed_time_ms(now_ts) > self.expected_fill_time_ms * 1.5
        {
            return ExecutionFailureReason::NoFillTimeout;
        }
        if self.queue_state.competition_state == CompetitionState::Saturated {
            return ExecutionFailureReason::CompetitionSpike;
        }
        if self.last_latency_us > 8_000 {
            return ExecutionFailureReason::LatencyTooHigh;
        }
        if self.queue_state.liquidity_pull_score > 0.70 {
            return ExecutionFailureReason::LiquidityPull;
        }
        if self.queue_state.outbid_count >= 2 {
            return ExecutionFailureReason::Outbid;
        }
        ExecutionFailureReason::QueueTooDeep
    }

    fn should_cancel(&self, now_ts: u64) -> bool {
        if self.cancel_count >= self.max_cancel_per_order {
            return false;
        }
        self.fill_state.filled_qty <= f64::EPSILON
            && self.elapsed_time_ms(now_ts) > self.expected_fill_time_ms * 1.35
    }

    fn estimate_fill_probability(&self) -> f64 {
        let base = if self.initial_decision.fill_probability == FillProbabilityClass::HighFill {
            0.72
        } else {
            0.42
        };
        let queue_penalty = (self.queue_state.queue_position / 3.0).clamp(0.0, 0.65);
        let competition_penalty = match self.queue_state.competition_state {
            CompetitionState::Normal => 0.0,
            CompetitionState::Competitive => 0.18,
            CompetitionState::Saturated => 0.40,
        };
        let outbid_penalty = (self.queue_state.outbid_count as f64 * 0.08).clamp(0.0, 0.24);
        (base - queue_penalty - competition_penalty - outbid_penalty).clamp(0.0, 1.0)
    }

    fn elapsed_time_ms(&self, now_ts: u64) -> f64 {
        now_ts.saturating_sub(self.start_ts) as f64
    }

    fn cooldown_elapsed(&self, now_ts: u64) -> bool {
        now_ts.saturating_sub(self.last_action_ts) >= self.action_cooldown_ms
    }

    fn apply_action(&mut self, action: &ExecutionAction, now_ts: u64) {
        self.last_action_ts = now_ts;
        match action {
            ExecutionAction::Cancel => {
                self.cancel_count = self.cancel_count.saturating_add(1);
            }
            ExecutionAction::Replace { new_price } => {
                self.replace_count = self.replace_count.saturating_add(1);
                self.current_price = Some(*new_price);
            }
            ExecutionAction::SwitchStrategy { new_strategy } => {
                self.current_strategy = *new_strategy;
            }
            ExecutionAction::Abort => {
                self.cancel_count = self.cancel_count.saturating_add(1);
                self.status = OrderLifecycleState::Canceled;
            }
            ExecutionAction::Hold => {}
        }
    }
}

pub async fn run(
    cfg: Config,
    mut order_rx: Receiver<OrderIntent>,
    mut market_rx: Receiver<MarketUpdate>,
    mut execution_rx: Receiver<ExecutionControllerEvent>,
    action_tx: Sender<ExecutionInstruction>,
    feedback_tx: Sender<ExecutionControlFeedback>,
    metrics: Arc<Metrics>,
) {
    let mut controllers: HashMap<String, ExecutionController> = HashMap::new();
    let mut order_lookup: HashMap<String, String> = HashMap::new();

    loop {
        tokio::select! {
            Some(intent) = order_rx.recv() => {
                let mut controller = ExecutionController::new(&intent, &cfg);
                let key = controller.idempotency_key().to_string();
                controller.status = OrderLifecycleState::Sent;
                if action_tx
                    .try_send(ExecutionInstruction::Submit {
                        intent,
                        idempotency_key: key.clone(),
                    })
                    .is_err()
                {
                    metrics.channel_backpressure_total.with_label_values(&["execution_controller_submit"]).inc();
                }
                controllers.insert(key.clone(), controller);
                order_lookup.insert(key.clone(), key);
            }
            Some(update) = execution_rx.recv() => {
                let key = resolve_controller_key(&update, &order_lookup);
                let Some(key) = key else {
                    continue;
                };
                let mut remove_key = None;
                if let Some(controller) = controllers.get_mut(&key) {
                    controller.on_execution_event(&update);
                    order_lookup.insert(update.order_id.clone(), key.clone());
                    if let Some(client_key) = &update.idempotency_key {
                        order_lookup.insert(client_key.clone(), key.clone());
                    }
                    emit_execution_metrics(&metrics, controller, &update);
                    if controller.is_terminal() {
                        if controller.status != OrderLifecycleState::Filled {
                            let feedback = ExecutionControlFeedback {
                                order_id: controller.order_id.clone(),
                                symbol: controller.symbol.clone(),
                                reason: controller.classify_failure(update.event_ts),
                                elapsed_ms: controller.elapsed_time_ms(update.event_ts),
                                fill_ratio: 1.0 - controller.fill_state.remaining_qty / (controller.fill_state.filled_qty + controller.fill_state.remaining_qty).max(1e-9),
                                expected_fill_time_ms: controller.expected_fill_time_ms,
                                edge_half_life_ms: controller.edge_half_life_ms,
                                aborted_due_to_decay: controller.elapsed_time_ms(update.event_ts) > controller.edge_half_life_ms,
                            };
                            let _ = feedback_tx.try_send(feedback);
                        }
                        remove_key = Some(key.clone());
                    } else if let Some(instruction) =
                        evaluate_and_convert(controller, update.event_ts)
                    {
                        observe_action_metrics(&metrics, controller, &instruction);
                        let _ = action_tx.try_send(instruction);
                    }
                }
                if let Some(key) = remove_key {
                    controllers.remove(&key);
                }
            }
            Some(update) = market_rx.recv() => {
                let now_ts = market_timestamp(&update);
                let keys: Vec<String> = controllers
                    .iter()
                    .filter_map(|(key, controller)| {
                        if controller.symbol == cfg.symbol && !controller.is_terminal() {
                            Some(key.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                for key in keys {
                    if let Some(controller) = controllers.get_mut(&key) {
                        controller.on_market_update(&update);
                        if let Some(instruction) = evaluate_and_convert(controller, now_ts) {
                            observe_action_metrics(&metrics, controller, &instruction);
                            let _ = action_tx.try_send(instruction);
                        }
                    }
                }
            }
            else => break,
        }
    }
}

fn evaluate_and_convert(
    controller: &mut ExecutionController,
    now_ts: u64,
) -> Option<ExecutionInstruction> {
    let action = controller.evaluate_action(now_ts);
    let instruction = match action {
        ExecutionAction::Hold => return None,
        ExecutionAction::Cancel => Some(ExecutionInstruction::Cancel {
            order_id: controller.order_id.clone(),
            symbol: controller.symbol.clone(),
            reason: "queue_timeout",
        }),
        ExecutionAction::Replace { new_price } => Some(ExecutionInstruction::Replace {
            order_id: controller.order_id.clone(),
            symbol: controller.symbol.clone(),
            new_price,
            reason: "queue_fight",
        }),
        ExecutionAction::SwitchStrategy { new_strategy } => Some(ExecutionInstruction::SwitchStrategy {
            order_id: controller.order_id.clone(),
            symbol: controller.symbol.clone(),
            new_strategy,
            price: controller.compute_improved_price(),
            reason: "adaptive_switch",
        }),
        ExecutionAction::Abort => Some(ExecutionInstruction::Abort {
            order_id: controller.order_id.clone(),
            symbol: controller.symbol.clone(),
            reason: "edge_decay_abort",
        }),
    };
    controller.apply_action(&action, now_ts);
    instruction
}

fn queue_fill_probability(queue: QueueEstimate) -> f64 {
    queue.fill_probability.clamp(0.0, 1.0)
}

fn expected_fill_time_ms(intent: &OrderIntent) -> f64 {
    let base = intent.expected_duration_ms as f64;
    let fill_probability = queue_fill_probability(intent.queue_estimate).max(0.05);
    let urgency_factor = (1.15 - intent.urgency * 0.35).clamp(0.65, 1.20);
    base / fill_probability * urgency_factor
}

fn edge_half_life_ms(intent: &OrderIntent) -> f64 {
    let half_life_samples = intent.edge_half_life_samples.max(0.5);
    (half_life_samples * intent.expected_duration_ms as f64).clamp(
        intent.expected_duration_ms as f64 * 0.75,
        intent.expected_duration_ms as f64 * 12.0,
    )
}

fn market_timestamp(update: &MarketUpdate) -> u64 {
    match update {
        MarketUpdate::Trade(trade) => trade.timestamp,
        MarketUpdate::BookDelta(book) => book.timestamp,
    }
}

fn resolve_controller_key(
    update: &ExecutionControllerEvent,
    order_lookup: &HashMap<String, String>,
) -> Option<String> {
    order_lookup
        .get(&update.order_id)
        .cloned()
        .or_else(|| update.idempotency_key.as_ref().and_then(|key| order_lookup.get(key).cloned()))
}

fn competition_state_from_flag(
    flag: CompetitionFlag,
    fallback: CompetitionState,
) -> CompetitionState {
    match flag {
        CompetitionFlag::RepeatedOutbid | CompetitionFlag::PartialFillToxicity => {
            CompetitionState::Saturated
        }
        CompetitionFlag::SlowFill | CompetitionFlag::CancelLatency => CompetitionState::Competitive,
        CompetitionFlag::None => fallback,
    }
}

fn classify_competition_state(
    fill_probability: f64,
    outbid_count: u32,
    liquidity_pull_score: f64,
) -> CompetitionState {
    if fill_probability < 0.15 || outbid_count >= 3 || liquidity_pull_score > 0.80 {
        CompetitionState::Saturated
    } else if fill_probability < 0.32 || outbid_count >= 1 || liquidity_pull_score > 0.45 {
        CompetitionState::Competitive
    } else {
        CompetitionState::Normal
    }
}

fn derived_tick(best_bid: f64, best_ask: f64, reference: f64) -> f64 {
    let spread = (best_ask - best_bid).abs();
    spread
        .max(reference.abs() * 0.00002)
        .max(0.01)
        .min(reference.abs() * 0.00025)
}

fn scored_decision_from_intent(intent: &OrderIntent) -> ScoredDecision {
    ScoredDecision {
        market: crate::types::MarketEvent {
            timestamp: intent.timestamp,
            price: intent.last_price,
            volume: 0.0,
            side: intent.request.side,
            bid_ask_imbalance: 0.0,
            spread: intent.regime.spread,
        },
        event: crate::types::Event::Neutral,
        features: crate::types::Features::default(),
        regime: intent.regime.clone(),
        context: intent.context.clone(),
        flow: intent.flow,
        timing: intent.timing,
        direction: if intent.request.side == Side::Buy {
            crate::types::Direction::Long
        } else {
            crate::types::Direction::Short
        },
        confidence: intent.score,
        continuation_prob: intent.score,
        reversal_prob: (1.0 - intent.score).clamp(0.0, 1.0),
        score: intent.score,
        decision: intent.reason,
        expected_duration_ms: intent.expected_duration_ms,
        urgency: intent.urgency,
        expected_slippage_bps: intent.expected_slippage_bps,
        data_latency_ms: intent.data_latency_ms,
        adversarial_risk: 0.0,
        edge_state: intent.edge_state,
        edge_regime: intent.edge_regime,
        edge_reliability_score: intent.edge_reliability_score,
        edge_half_life_samples: intent.edge_half_life_samples,
        dynamic_size_multiplier: intent.dynamic_size_multiplier,
        competition_state: intent.competition_state,
        competition_score: intent.competition_score,
        fill_probability: intent.fill_probability,
    }
}

pub fn idempotency_key(intent: &OrderIntent) -> String {
    format!(
        "{}-{}-{}",
        intent.request.symbol,
        intent.timestamp,
        match intent.request.side {
            Side::Buy => "buy",
            Side::Sell => "sell",
        }
    )
}

fn emit_execution_metrics(metrics: &Metrics, controller: &ExecutionController, update: &ExecutionControllerEvent) {
    let symbol = controller.symbol.as_str();
    let expected_ratio = if controller.expected_fill_time_ms > 0.0 {
        controller.elapsed_time_ms(update.event_ts) / controller.expected_fill_time_ms
    } else {
        0.0
    };
    metrics
        .execution_efficiency
        .with_label_values(&[symbol])
        .observe((1.0 - expected_ratio).clamp(-1.0, 1.0));
    metrics
        .fill_expected_divergence
        .with_label_values(&[symbol])
        .observe((controller.queue_state.fill_probability - update.partial_fill_ratio).abs());
    if update.status == OrderLifecycleState::Filled {
        metrics
            .avg_time_to_fill_ms
            .with_label_values(&[symbol])
            .observe(controller.elapsed_time_ms(update.event_ts));
    }
}

fn observe_action_metrics(
    metrics: &Metrics,
    controller: &ExecutionController,
    instruction: &ExecutionInstruction,
) {
    let symbol = controller.symbol.as_str();
    let ratio = (controller.cancel_count + controller.replace_count) as f64;
    metrics
        .cancel_replace_ratio
        .with_label_values(&[symbol])
        .observe(ratio);
    if matches!(instruction, ExecutionInstruction::Abort { .. }) {
        metrics
            .aborted_due_to_decay_total
            .with_label_values(&[symbol])
            .inc();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        accounting::edge_validation::{EdgeRegime, EdgeState},
        config::Exchange,
        types::{
            CompetitionState, Decision, Event, Features, FlowState, MarketContext, MarketEvent,
            MarketRegime, MicroTimingState, OrderRequest, OrderType, Position,
        },
    };

    fn decision() -> ScoredDecision {
        ScoredDecision {
            market: MarketEvent {
                timestamp: 1_000,
                price: 100.0,
                volume: 1.0,
                side: Side::Buy,
                bid_ask_imbalance: 0.0,
                spread: 0.02,
            },
            event: Event::Neutral,
            features: Features::default(),
            regime: MarketRegime {
                volatility: 1.2,
                spread: 0.02,
                trend_strength: 1.1,
            },
            context: MarketContext::default(),
            flow: FlowState::default(),
            timing: MicroTimingState::default(),
            direction: crate::types::Direction::Long,
            confidence: 0.8,
            continuation_prob: 0.82,
            reversal_prob: 0.18,
            score: 0.82,
            decision: Decision::EnterSmall,
            expected_duration_ms: 100,
            urgency: 0.7,
            expected_slippage_bps: 1.0,
            data_latency_ms: 5,
            adversarial_risk: 0.1,
            edge_state: EdgeState::Valid,
            edge_regime: EdgeRegime::Stable,
            edge_reliability_score: 0.8,
            edge_half_life_samples: 3.0,
            dynamic_size_multiplier: 0.9,
            competition_state: CompetitionState::Normal,
            competition_score: 0.1,
            fill_probability: FillProbabilityClass::LowFill,
        }
    }

    fn intent() -> OrderIntent {
        let decision = decision();
        OrderIntent {
            request: OrderRequest {
                symbol: "BTCUSDT".to_string(),
                side: Side::Buy,
                size: 1.0,
                price: Some(100.0),
                order_type: OrderType::Limit,
                post_only: true,
                reduce_only: false,
                max_slippage_bps: 2.0,
            },
            reason: Decision::EnterSmall,
            score: decision.score,
            last_price: 100.0,
            position_before: Position::default(),
            timestamp: 1_000,
            urgency: 0.7,
            expected_slippage_bps: 1.0,
            expected_duration_ms: 100,
            data_latency_ms: 5,
            regime: decision.regime,
            context: decision.context,
            flow: decision.flow,
            timing: decision.timing,
            edge_state: decision.edge_state,
            edge_regime: decision.edge_regime,
            edge_reliability_score: decision.edge_reliability_score,
            edge_half_life_samples: decision.edge_half_life_samples,
            dynamic_size_multiplier: decision.dynamic_size_multiplier,
            competition_state: decision.competition_state,
            competition_score: decision.competition_score,
            execution_mode: ExecutionMode::Passive,
            queue_estimate: QueueEstimate {
                queue_position: 1.0,
                volume_ahead: 1.0,
                fill_probability: 0.5,
                placement_depth_bps: 0.5,
            },
            fill_probability: FillProbabilityClass::LowFill,
            meta: None,
        }
    }

    fn config() -> Config {
        Config {
            exchange: Exchange::Mock,
            symbol: "BTCUSDT".to_string(),
            capital: 10_000.0,
            max_risk_pct: 0.005,
            max_daily_drawdown_pct: 0.02,
            base_order_usd: 25.0,
            max_entries: 4,
            stop_loss_bps: 25.0,
            max_data_age_ms: 250,
            max_decision_latency_us: 1_500,
            max_execution_latency_us: 8_000,
            max_consecutive_losses: 3,
            channel_capacity: 128,
            window_ms: 500,
            metrics_addr: "127.0.0.1:9898".to_string(),
            control_plane_http: "http://127.0.0.1:8088".to_string(),
            control_plane_ws: "ws://127.0.0.1:8088/ws".to_string(),
            max_cancel_per_order: 2,
            max_replace_per_order: 3,
            execution_action_cooldown_ms: 10,
            queue_replace_volume_factor: 1.25,
            min_fill_probability: 0.28,
        }
    }

    #[test]
    fn order_aborts_when_exceeding_edge_half_life() {
        let controller = ExecutionController::new(&intent(), &config());
        assert_eq!(controller.evaluate_action(1_500), ExecutionAction::Abort);
    }

    #[test]
    fn replace_triggers_when_queue_worsens() {
        let mut controller = ExecutionController::new(&intent(), &config());
        controller.initial_decision.score = 0.70;
        controller.initial_decision.edge_reliability_score = 0.58;
        controller.queue_state.best_bid = 100.0;
        controller.queue_state.best_ask = 100.05;
        controller.queue_state.volume_ahead = 2.0;
        controller.queue_state.queue_position = 2.2;
        controller.queue_state.fill_probability = 0.10;
        assert!(matches!(
            controller.evaluate_action(1_140),
            ExecutionAction::Replace { .. }
        ));
    }

    #[test]
    fn strategy_switches_under_low_fill_probability() {
        let mut controller = ExecutionController::new(&intent(), &config());
        controller.queue_state.fill_probability = 0.10;
        controller.queue_state.competition_state = CompetitionState::Competitive;
        assert_eq!(
            controller.evaluate_action(1_220),
            ExecutionAction::SwitchStrategy {
                new_strategy: ExecutionStrategy::Aggressive
            }
        );
    }

    #[test]
    fn cancel_replace_respects_limits() {
        let mut controller = ExecutionController::new(&intent(), &config());
        controller.replace_count = controller.max_replace_per_order;
        controller.cancel_count = controller.max_cancel_per_order;
        controller.initial_decision.score = 0.60;
        controller.initial_decision.edge_reliability_score = 0.50;
        controller.queue_state.fill_probability = 0.05;
        controller.queue_state.queue_position = 3.0;
        controller.queue_state.volume_ahead = 3.0;
        assert_eq!(controller.evaluate_action(1_300), ExecutionAction::Hold);
    }

    #[test]
    fn failure_classification_correctness() {
        let mut controller = ExecutionController::new(&intent(), &config());
        controller.queue_state.outbid_count = 2;
        assert_eq!(
            controller.classify_failure(1_040),
            ExecutionFailureReason::Outbid
        );
    }

    #[test]
    fn controller_reacts_to_competition_spike() {
        let mut controller = ExecutionController::new(&intent(), &config());
        controller.queue_state.competition_state = CompetitionState::Saturated;
        assert_eq!(controller.evaluate_action(1_050), ExecutionAction::Abort);
    }
}
