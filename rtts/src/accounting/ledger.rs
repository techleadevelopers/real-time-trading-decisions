use std::collections::{HashMap, VecDeque};

use crate::{
    accounting::interfaces::LedgerBatchUpdate,
    types::{FillEvent, Side},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiquidityFlag {
    Maker,
    #[default]
    Taker,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LotMatchingMethod {
    #[default]
    Fifo,
    Lifo,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FillLedgerEntry {
    pub order_id: String,
    pub fill_id: String,
    pub symbol: String,
    pub side: Side,
    pub price: f64,
    pub quantity: f64,
    pub liquidity_flag: LiquidityFlag,
    pub fee_amount: f64,
    pub fee_asset: String,
    pub rebate_amount: f64,
    pub funding_amount: f64,
    pub event_time: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PositionLot {
    pub side: Side,
    pub entry_price: f64,
    pub quantity: f64,
    pub remaining_qty: f64,
    pub realized_pnl: f64,
    pub remaining_fee_amount: f64,
    pub remaining_rebate_amount: f64,
    pub remaining_funding_amount: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RealizedPnLEntry {
    pub trade_id: String,
    pub pnl: f64,
    pub fees: f64,
    pub funding: f64,
    pub timestamp: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccountState {
    pub open_positions: HashMap<String, VecDeque<PositionLot>>,
    pub realized_pnl_total: f64,
    pub unrealized_pnl: f64,
    pub fee_total: f64,
    pub funding_total: f64,
    pub rebate_total: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PositionExposure {
    pub net_quantity: f64,
    pub avg_entry_price: f64,
    pub open_lots: usize,
    pub realized_pnl: f64,
    pub unrealized_pnl: f64,
}

#[derive(Clone, Debug, Default)]
pub struct AccountingEngine {
    matching_method: LotMatchingMethod,
    state: AccountState,
    ledger_entries: Vec<FillLedgerEntry>,
    realized_entries: Vec<RealizedPnLEntry>,
    mark_prices: HashMap<String, f64>,
}

impl AccountingEngine {
    pub fn new(matching_method: LotMatchingMethod) -> Self {
        Self {
            matching_method,
            ..Self::default()
        }
    }

    pub fn apply_fill(&mut self, entry: FillLedgerEntry) -> LedgerBatchUpdate {
        self.state.fee_total += entry.fee_amount;
        self.state.funding_total += entry.funding_amount;
        self.state.rebate_total += entry.rebate_amount;
        self.ledger_entries.push(entry.clone());

        let mut ledger_batch = vec![entry.clone()];
        let matched_entries = self.apply_fill_internal(&entry);
        self.realized_entries.extend(matched_entries.iter().cloned());
        let symbol = entry.symbol.clone();
        let mark = self.mark_prices.get(&symbol).copied();
        if let Some(mark_price) = mark {
            self.state.unrealized_pnl = self.derive_unrealized_with_mark(&symbol, mark_price);
        } else {
            self.state.unrealized_pnl = self.derive_unrealized_total();
        }
        LedgerBatchUpdate {
            ledger_entries: {
                ledger_batch.shrink_to_fit();
                ledger_batch
            },
            pnl_state: self.state.clone(),
        }
    }

    pub fn mark_to_market(&mut self, symbol: &str, mark_price: f64) -> f64 {
        self.mark_prices.insert(symbol.to_string(), mark_price);
        self.state.unrealized_pnl = self.derive_unrealized_total();
        self.state.unrealized_pnl
    }

    pub fn state(&self) -> &AccountState {
        &self.state
    }

    pub fn realized_entries(&self) -> &[RealizedPnLEntry] {
        &self.realized_entries
    }

    pub fn position_exposure(&self, symbol: &str) -> PositionExposure {
        let Some(lots) = self.state.open_positions.get(symbol) else {
            return PositionExposure::default();
        };
        let mut signed_qty = 0.0;
        let mut gross_qty = 0.0;
        let mut gross_notional = 0.0;
        let mut unrealized = 0.0;
        let mark = self.mark_prices.get(symbol).copied();
        for lot in lots {
            let signed = signed_quantity(lot.side, lot.remaining_qty);
            signed_qty += signed;
            gross_qty += lot.remaining_qty;
            gross_notional += lot.entry_price * lot.remaining_qty;
            if let Some(mark_price) = mark {
                unrealized += unrealized_for_lot(lot, mark_price);
            }
        }
        PositionExposure {
            net_quantity: signed_qty,
            avg_entry_price: if gross_qty > 0.0 {
                gross_notional / gross_qty
            } else {
                0.0
            },
            open_lots: lots.len(),
            realized_pnl: self.state.realized_pnl_total,
            unrealized_pnl: unrealized,
        }
    }

    fn apply_fill_internal(&mut self, entry: &FillLedgerEntry) -> Vec<RealizedPnLEntry> {
        let lots = self
            .state
            .open_positions
            .entry(entry.symbol.clone())
            .or_default();
        let mut qty_left = entry.quantity.max(0.0);
        let fee_per_unit = per_unit(entry.fee_amount, entry.quantity);
        let rebate_per_unit = per_unit(entry.rebate_amount, entry.quantity);
        let funding_per_unit = per_unit(entry.funding_amount, entry.quantity);
        let mut realized = Vec::new();
        let mut match_index = 0usize;

        while qty_left > 1e-12 {
            let Some(index) = opposite_lot_index(lots, entry.side, self.matching_method) else {
                break;
            };
            let lot = lots.get_mut(index).expect("index from opposite_lot_index must exist");
            let lot_qty_before = lot.remaining_qty;
            let matched_qty = qty_left.min(lot.remaining_qty);
            let ratio = if lot_qty_before > 0.0 {
                matched_qty / lot_qty_before
            } else {
                0.0
            };

            let entry_fee = lot.remaining_fee_amount * ratio;
            let entry_rebate = lot.remaining_rebate_amount * ratio;
            let entry_funding = lot.remaining_funding_amount * ratio;
            let exit_fee = fee_per_unit * matched_qty;
            let exit_rebate = rebate_per_unit * matched_qty;
            let exit_funding = funding_per_unit * matched_qty;
            let gross = realized_gross(lot.side, lot.entry_price, entry.price, matched_qty);
            let net = gross - entry_fee - exit_fee + entry_rebate + exit_rebate - entry_funding - exit_funding;

            lot.realized_pnl += net;
            lot.remaining_qty = (lot.remaining_qty - matched_qty).max(0.0);
            lot.remaining_fee_amount = (lot.remaining_fee_amount - entry_fee).max(0.0);
            lot.remaining_rebate_amount = (lot.remaining_rebate_amount - entry_rebate).max(0.0);
            lot.remaining_funding_amount = (lot.remaining_funding_amount - entry_funding).max(0.0);

            self.state.realized_pnl_total += net;
            realized.push(RealizedPnLEntry {
                trade_id: format!("{}:{}:{}", entry.order_id, entry.fill_id, match_index),
                pnl: net,
                fees: entry_fee + exit_fee - entry_rebate - exit_rebate,
                funding: entry_funding + exit_funding,
                timestamp: entry.event_time,
            });
            match_index = match_index.saturating_add(1);
            qty_left -= matched_qty;
            if lot.remaining_qty <= 1e-12 {
                lots.remove(index);
            }
        }

        if qty_left > 1e-12 {
            let new_lot = PositionLot {
                side: entry.side,
                entry_price: entry.price,
                quantity: qty_left,
                remaining_qty: qty_left,
                realized_pnl: 0.0,
                remaining_fee_amount: fee_per_unit * qty_left,
                remaining_rebate_amount: rebate_per_unit * qty_left,
                remaining_funding_amount: funding_per_unit * qty_left,
            };
            push_lot(lots, new_lot, self.matching_method);
        }

        realized
    }

    fn derive_unrealized_total(&self) -> f64 {
        self.mark_prices
            .iter()
            .map(|(symbol, mark)| self.derive_unrealized_with_mark(symbol, *mark))
            .sum()
    }

    fn derive_unrealized_with_mark(&self, symbol: &str, mark_price: f64) -> f64 {
        self.state
            .open_positions
            .get(symbol)
            .map(|lots| lots.iter().map(|lot| unrealized_for_lot(lot, mark_price)).sum())
            .unwrap_or_default()
    }
}

impl From<&FillEvent> for FillLedgerEntry {
    fn from(fill: &FillEvent) -> Self {
        Self {
            order_id: fill.order_id.clone(),
            fill_id: fill.fill_id.clone(),
            symbol: fill.symbol.clone(),
            side: fill.side,
            price: fill.price,
            quantity: fill.filled_size,
            liquidity_flag: fill.liquidity_flag,
            fee_amount: fill.fee,
            fee_asset: fill.fee_asset.clone(),
            rebate_amount: fill.rebate_amount,
            funding_amount: fill.funding_amount,
            event_time: fill.truth.last_fill_timestamp.max(fill.timestamp),
        }
    }
}

fn push_lot(lots: &mut VecDeque<PositionLot>, lot: PositionLot, method: LotMatchingMethod) {
    match method {
        LotMatchingMethod::Fifo | LotMatchingMethod::Lifo => lots.push_back(lot),
    }
}

fn opposite_lot_index(
    lots: &VecDeque<PositionLot>,
    side: Side,
    method: LotMatchingMethod,
) -> Option<usize> {
    match method {
        LotMatchingMethod::Fifo => lots.iter().position(|lot| lot.side != side),
        LotMatchingMethod::Lifo => lots.iter().rposition(|lot| lot.side != side),
    }
}

fn realized_gross(entry_side: Side, entry_price: f64, exit_price: f64, quantity: f64) -> f64 {
    match entry_side {
        Side::Buy => (exit_price - entry_price) * quantity,
        Side::Sell => (entry_price - exit_price) * quantity,
    }
}

fn unrealized_for_lot(lot: &PositionLot, mark_price: f64) -> f64 {
    match lot.side {
        Side::Buy => (mark_price - lot.entry_price) * lot.remaining_qty,
        Side::Sell => (lot.entry_price - mark_price) * lot.remaining_qty,
    }
}

fn signed_quantity(side: Side, quantity: f64) -> f64 {
    match side {
        Side::Buy => quantity,
        Side::Sell => -quantity,
    }
}

fn per_unit(total: f64, quantity: f64) -> f64 {
    if quantity <= 1e-12 {
        0.0
    } else {
        total / quantity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(
        fill_id: &str,
        side: Side,
        price: f64,
        quantity: f64,
        liquidity_flag: LiquidityFlag,
        fee_amount: f64,
        rebate_amount: f64,
    ) -> FillLedgerEntry {
        FillLedgerEntry {
            order_id: "order-1".to_string(),
            fill_id: fill_id.to_string(),
            symbol: "BTCUSDT".to_string(),
            side,
            price,
            quantity,
            liquidity_flag,
            fee_amount,
            fee_asset: "USDT".to_string(),
            rebate_amount,
            funding_amount: 0.0,
            event_time: 1,
        }
    }

    #[test]
    fn partial_fills_compute_fifo_realized_pnl() {
        let mut engine = AccountingEngine::new(LotMatchingMethod::Fifo);
        engine.apply_fill(fill("f1", Side::Buy, 100.0, 1.0, LiquidityFlag::Maker, 0.10, 0.02));
        engine.apply_fill(fill("f2", Side::Buy, 101.0, 1.0, LiquidityFlag::Taker, 0.20, 0.00));
        engine.apply_fill(fill("f3", Side::Sell, 103.0, 1.5, LiquidityFlag::Taker, 0.30, 0.00));

        assert!((engine.state().realized_pnl_total - 3.25).abs() < 1e-9);
        let exposure = engine.position_exposure("BTCUSDT");
        assert!((exposure.net_quantity - 0.5).abs() < 1e-9);
        assert_eq!(engine.realized_entries().len(), 2);
    }

    #[test]
    fn maker_rebate_and_taker_fee_flow_into_realized_pnl() {
        let mut engine = AccountingEngine::new(LotMatchingMethod::Fifo);
        engine.apply_fill(fill("f1", Side::Buy, 100.0, 1.0, LiquidityFlag::Maker, 0.05, 0.03));
        engine.apply_fill(fill("f2", Side::Sell, 101.0, 1.0, LiquidityFlag::Taker, 0.10, 0.00));

        let expected = (101.0 - 100.0) - 0.05 - 0.10 + 0.03;
        assert!((engine.state().realized_pnl_total - expected).abs() < 1e-9);
        assert!((engine.state().fee_total - 0.15).abs() < 1e-9);
        assert!((engine.state().rebate_total - 0.03).abs() < 1e-9);
    }

    #[test]
    fn position_scaling_and_mark_to_market_are_consistent() {
        let mut engine = AccountingEngine::new(LotMatchingMethod::Fifo);
        engine.apply_fill(fill("f1", Side::Buy, 100.0, 1.0, LiquidityFlag::Maker, 0.0, 0.0));
        engine.apply_fill(fill("f2", Side::Buy, 102.0, 2.0, LiquidityFlag::Taker, 0.0, 0.0));
        let unrealized = engine.mark_to_market("BTCUSDT", 103.0);
        let exposure = engine.position_exposure("BTCUSDT");

        assert!((exposure.net_quantity - 3.0).abs() < 1e-9);
        assert!((exposure.avg_entry_price - (304.0 / 3.0)).abs() < 1e-9);
        assert!((unrealized - 5.0).abs() < 1e-9);
        assert!((engine.state().unrealized_pnl - 5.0).abs() < 1e-9);
    }

    #[test]
    fn accounting_consistency_matches_realized_plus_open_state() {
        let mut engine = AccountingEngine::new(LotMatchingMethod::Fifo);
        engine.apply_fill(fill("f1", Side::Sell, 110.0, 2.0, LiquidityFlag::Maker, 0.08, 0.01));
        engine.apply_fill(fill("f2", Side::Buy, 108.0, 1.0, LiquidityFlag::Taker, 0.12, 0.00));
        engine.mark_to_market("BTCUSDT", 107.0);

        let exposure = engine.position_exposure("BTCUSDT");
        assert!((engine.state().realized_pnl_total - 1.81).abs() < 1e-9);
        assert!((exposure.net_quantity + 1.0).abs() < 1e-9);
        assert!((engine.state().unrealized_pnl - 3.0).abs() < 1e-9);
        assert!(engine.state().open_positions.contains_key("BTCUSDT"));
    }
}
