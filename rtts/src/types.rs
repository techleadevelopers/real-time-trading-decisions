use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    #[inline]
    pub fn opposite(self) -> Self {
        match self {
            Self::Buy => Self::Sell,
            Self::Sell => Self::Buy,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Long,
    Short,
    Flat,
}

impl Direction {
    #[inline]
    pub fn side(self) -> Option<Side> {
        match self {
            Self::Long => Some(Side::Buy),
            Self::Short => Some(Side::Sell),
            Self::Flat => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TradeEvent {
    pub timestamp: u64,
    pub price: f64,
    pub volume: f64,
    pub side: Side,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BookLevel {
    pub price: f64,
    pub quantity: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BookDelta {
    pub timestamp: u64,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MarketUpdate {
    Trade(TradeEvent),
    BookDelta(BookDelta),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketEvent {
    pub timestamp: u64,
    pub price: f64,
    pub volume: f64,
    pub side: Side,
    pub bid_ask_imbalance: f64,
    pub spread: f64,
}

impl From<TradeEvent> for MarketEvent {
    fn from(value: TradeEvent) -> Self {
        Self {
            timestamp: value.timestamp,
            price: value.price,
            volume: value.volume,
            side: value.side,
            bid_ask_imbalance: 0.0,
            spread: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    DumpDetected,
    PumpDetected,
    Neutral,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Features {
    pub velocity: f64,
    pub vol_z: f64,
    pub imbalance: f64,
    pub volatility: f64,
    pub spread: f64,
    pub weighted_imbalance: f64,
    pub spread_dynamics: f64,
    pub micro_price_velocity: f64,
    pub trade_clustering: f64,
    pub liquidity_shift: f64,
    pub order_flow_delta: f64,
    pub absorption: f64,
    pub spoofing_risk: f64,
    pub liquidity_pull: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Ignore,
    EnterSmall,
    ScaleIn,
    Exit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowSignal {
    StrongContinuation,
    #[default]
    WeakContinuation,
    Exhaustion,
    ReversalRisk,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct FlowState {
    pub signal: FlowSignal,
    pub aggressive_ratio: f64,
    pub absorption: f64,
    pub exhaustion: f64,
    pub continuation_strength: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimingSignal {
    Optimal,
    #[default]
    Neutral,
    Wait,
    Missed,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct MicroTimingState {
    pub signal: TimingSignal,
    pub spread_compression: f64,
    pub liquidity_pull: f64,
    pub trade_burst: f64,
    pub micro_pullback: f64,
    pub timing_score: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScenarioType {
    Continuation,
    Reversal,
    Chop,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scenario {
    pub name: ScenarioType,
    pub probability: f64,
    pub expected_pnl: f64,
    pub risk: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinalDecision {
    Execute,
    Wait,
    Skip,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetaDecision {
    pub decision: FinalDecision,
    pub scenarios: Vec<Scenario>,
    pub ev: f64,
    pub adjusted_ev: f64,
    pub worst_case_loss: f64,
    pub entry_quality: f64,
    pub competition_score: f64,
    pub opportunity_rank: f64,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Position {
    pub size: f64,
    pub avg_price: f64,
    pub entries: u32,
    pub confidence: f64,
    pub unrealized_pnl: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OrderBookState {
    pub best_bid: f64,
    pub best_ask: f64,
    pub bid_volume: f64,
    pub ask_volume: f64,
    pub imbalance: f64,
    pub liquidity_clusters: Vec<f64>,
    pub top_pressure: f64,
    pub weighted_imbalance: f64,
    pub spread: f64,
    pub spoofing_score: f64,
    pub liquidity_pull: f64,
    pub absorption: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TapeState {
    pub buy_volume: f64,
    pub sell_volume: f64,
    pub delta: f64,
    pub trade_frequency: f64,
    pub volume_burst: f64,
    pub exhaustion: f64,
    pub continuation: f64,
    pub last_price: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MarketRegime {
    pub volatility: f64,
    pub spread: f64,
    pub trend_strength: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegimeKind {
    #[default]
    Normal,
    HighVolatility,
    NewsShock,
    LowLiquidity,
    TrendExpansion,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketContext {
    pub regime: RegimeKind,
    pub volatility: f64,
    pub liquidity_score: f64,
    pub stability_score: f64,
}

impl Default for MarketContext {
    fn default() -> Self {
        Self {
            regime: RegimeKind::Normal,
            volatility: 0.0,
            liquidity_score: 1.0,
            stability_score: 1.0,
        }
    }
}

impl Default for SymbolProfile {
    fn default() -> Self {
        Self {
            symbol: String::new(),
            avg_spread_bps: 4.0,
            avg_fill_probability: 0.50,
            volatility_ema: 1.0,
            avg_trade_size: 1.0,
        }
    }
}

impl Position {
    #[inline]
    pub fn is_open(&self) -> bool {
        self.size.abs() > f64::EPSILON
    }

    #[inline]
    pub fn side(&self) -> Option<Side> {
        if self.size > 0.0 {
            Some(Side::Buy)
        } else if self.size < 0.0 {
            Some(Side::Sell)
        } else {
            None
        }
    }

    #[inline]
    pub fn update_unrealized(&mut self, mark_price: f64) {
        if self.is_open() {
            self.unrealized_pnl = (mark_price - self.avg_price) * self.size;
        } else {
            self.unrealized_pnl = 0.0;
        }
    }
}

#[derive(Clone, Debug)]
pub struct DetectedEvent {
    pub market: MarketEvent,
    pub event: Event,
}

#[derive(Clone, Debug)]
pub struct FeatureFrame {
    pub market: MarketEvent,
    pub event: Event,
    pub features: Features,
}

#[derive(Clone, Debug)]
pub struct MicrostructureFrame {
    pub timestamp: u64,
    pub trade: Option<TradeEvent>,
    pub book: OrderBookState,
    pub tape: TapeState,
    pub features: Features,
    pub regime: MarketRegime,
    pub context: MarketContext,
    pub flow: FlowState,
    pub timing: MicroTimingState,
    pub stale: bool,
}

#[derive(Clone, Debug)]
pub struct ScoredDecision {
    pub market: MarketEvent,
    pub event: Event,
    pub features: Features,
    pub regime: MarketRegime,
    pub context: MarketContext,
    pub flow: FlowState,
    pub timing: MicroTimingState,
    pub direction: Direction,
    pub confidence: f64,
    pub continuation_prob: f64,
    pub reversal_prob: f64,
    pub score: f64,
    pub decision: Decision,
    pub expected_duration_ms: u64,
    pub urgency: f64,
    pub expected_slippage_bps: f64,
    pub data_latency_ms: u64,
    pub adversarial_risk: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderType {
    Market,
    Limit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExecutionMode {
    Aggressive,
    #[default]
    Passive,
    Defensive,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FillProbabilityClass {
    HighFill,
    #[default]
    LowFill,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct QueueEstimate {
    pub queue_position: f64,
    pub volume_ahead: f64,
    pub fill_probability: f64,
    pub placement_depth_bps: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MicroExitReason {
    #[default]
    None,
    TakeProfit,
    MomentumFade,
    AdverseFlow,
    LiquidityCollapse,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MicroExitSignal {
    pub reason: MicroExitReason,
    pub reduce_ratio: f64,
    pub urgency: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MarkoutSnapshot {
    pub pnl_100ms: f64,
    pub pnl_500ms: f64,
    pub pnl_1s: f64,
}

#[derive(Clone, Debug)]
pub struct SymbolProfile {
    pub symbol: String,
    pub avg_spread_bps: f64,
    pub avg_fill_probability: f64,
    pub volatility_ema: f64,
    pub avg_trade_size: f64,
}

#[derive(Clone, Debug)]
pub struct OrderRequest {
    pub symbol: String,
    pub side: Side,
    pub size: f64,
    pub price: Option<f64>,
    pub order_type: OrderType,
    pub post_only: bool,
    pub reduce_only: bool,
    pub max_slippage_bps: f64,
}

#[derive(Clone, Debug)]
pub struct OrderIntent {
    pub request: OrderRequest,
    pub reason: Decision,
    pub score: f64,
    pub last_price: f64,
    pub position_before: Position,
    pub timestamp: u64,
    pub urgency: f64,
    pub expected_slippage_bps: f64,
    pub expected_duration_ms: u64,
    pub data_latency_ms: u64,
    pub regime: MarketRegime,
    pub context: MarketContext,
    pub flow: FlowState,
    pub timing: MicroTimingState,
    pub execution_mode: ExecutionMode,
    pub queue_estimate: QueueEstimate,
    pub fill_probability: FillProbabilityClass,
    pub meta: Option<MetaDecision>,
}

#[derive(Clone, Debug)]
pub struct FillEvent {
    pub symbol: String,
    pub side: Side,
    pub size: f64,
    pub price: f64,
    pub requested_price: f64,
    pub filled_size: f64,
    pub remaining_size: f64,
    pub fee: f64,
    pub timestamp: u64,
    pub latency_us: u64,
    pub expected_slippage_bps: f64,
    pub actual_slippage_bps: f64,
    pub queue_estimate: QueueEstimate,
    pub execution_mode: ExecutionMode,
    pub micro_exit: MicroExitSignal,
    pub markout: MarkoutSnapshot,
    pub complete: bool,
}

#[derive(Clone, Debug)]
pub struct LearningSample {
    pub timestamp: u64,
    pub direction: Direction,
    pub confidence: f64,
    pub predicted_score: f64,
    pub expected_slippage_bps: f64,
    pub actual_slippage_bps: f64,
    pub pnl: f64,
    pub duration_ms: u64,
    pub entry_quality: f64,
    pub markout_100ms: f64,
    pub markout_500ms: f64,
    pub markout_1s: f64,
    pub regime: MarketRegime,
}
