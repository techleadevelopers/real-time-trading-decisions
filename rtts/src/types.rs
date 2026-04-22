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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketEvent {
    pub timestamp: u64,
    pub price: f64,
    pub volume: f64,
    pub side: Side,
    pub bid_ask_imbalance: f64,
    pub spread: f64,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Ignore,
    EnterSmall,
    ScaleIn,
    Exit,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Position {
    pub size: f64,
    pub avg_price: f64,
    pub entries: u32,
    pub confidence: f64,
    pub unrealized_pnl: f64,
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
pub struct ScoredDecision {
    pub market: MarketEvent,
    pub event: Event,
    pub features: Features,
    pub continuation_prob: f64,
    pub reversal_prob: f64,
    pub score: f64,
    pub decision: Decision,
}

#[derive(Clone, Debug)]
pub struct OrderRequest {
    pub symbol: String,
    pub side: Side,
    pub size: f64,
    pub price: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct OrderIntent {
    pub request: OrderRequest,
    pub reason: Decision,
    pub score: f64,
    pub last_price: f64,
    pub position_before: Position,
    pub timestamp: u64,
}

#[derive(Clone, Debug)]
pub struct FillEvent {
    pub symbol: String,
    pub side: Side,
    pub size: f64,
    pub price: f64,
    pub fee: f64,
    pub timestamp: u64,
}
