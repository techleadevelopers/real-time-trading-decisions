use std::time::Duration;

use crate::{
    accounting::{
        latency::LatencyBreakdown,
        ledger::{AccountState, FillLedgerEntry},
    },
    types::{Direction, ScoredDecision},
};

pub trait StrategyDecision {
    fn expected_edge(&self) -> f64;
    fn confidence(&self) -> f64;
    fn direction(&self) -> Direction;
    fn expected_duration(&self) -> Duration;
}

pub trait ExecutionReport {
    fn fills(&self) -> &[FillLedgerEntry];
    fn slippage(&self) -> f64;
    fn latency(&self) -> &LatencyBreakdown;
    fn fill_ratio(&self) -> f64;
}

pub trait AccountingUpdate {
    fn ledger_entries(&self) -> &[FillLedgerEntry];
    fn pnl_state(&self) -> &AccountState;
}

#[derive(Clone, Debug, Default)]
pub struct ExecutionSummary {
    pub fills: Vec<FillLedgerEntry>,
    pub slippage: f64,
    pub latency: LatencyBreakdown,
    pub fill_ratio: f64,
}

#[derive(Clone, Debug)]
pub struct LedgerBatchUpdate {
    pub ledger_entries: Vec<FillLedgerEntry>,
    pub pnl_state: AccountState,
}

impl StrategyDecision for ScoredDecision {
    fn expected_edge(&self) -> f64 {
        (self.continuation_prob - self.reversal_prob).clamp(-1.0, 1.0)
    }

    fn confidence(&self) -> f64 {
        self.confidence
    }

    fn direction(&self) -> Direction {
        self.direction
    }

    fn expected_duration(&self) -> Duration {
        Duration::from_millis(self.expected_duration_ms)
    }
}

impl ExecutionReport for ExecutionSummary {
    fn fills(&self) -> &[FillLedgerEntry] {
        &self.fills
    }

    fn slippage(&self) -> f64 {
        self.slippage
    }

    fn latency(&self) -> &LatencyBreakdown {
        &self.latency
    }

    fn fill_ratio(&self) -> f64 {
        self.fill_ratio
    }
}

impl AccountingUpdate for LedgerBatchUpdate {
    fn ledger_entries(&self) -> &[FillLedgerEntry] {
        &self.ledger_entries
    }

    fn pnl_state(&self) -> &AccountState {
        &self.pnl_state
    }
}
