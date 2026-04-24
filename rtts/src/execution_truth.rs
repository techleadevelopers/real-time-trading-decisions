use crate::{
    accounting::{
        latency::{LatencyDistributions, LatencyDistributionSnapshot},
        quality::{execution_quality_report, EdgeReliabilityModel, ExecutionQualityReport},
    },
    metrics::Metrics,
    types::{
        CompetitionFlag, Direction, ExecutionEvent, FillEvent, LearningSample, MarketRegime,
        MarketUpdate, MarkoutSnapshot, Side,
    },
};
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::info;

const MARKOUT_100_MS: u64 = 100;
const MARKOUT_500_MS: u64 = 500;
const MARKOUT_1S: u64 = 1_000;
const MARKOUT_5S: u64 = 5_000;

#[derive(Clone, Debug)]
struct PendingFill {
    fill: FillEvent,
    created_at: u64,
    price_100ms: Option<f64>,
    price_500ms: Option<f64>,
    price_1s: Option<f64>,
    price_5s: Option<f64>,
}

pub async fn run(
    mut market_rx: Receiver<MarketUpdate>,
    mut fill_rx: Receiver<FillEvent>,
    learning_tx: Sender<LearningSample>,
    metrics: Arc<Metrics>,
) {
    let mut pending: VecDeque<PendingFill> = VecDeque::with_capacity(4096);
    let mut edge_reliability = EdgeReliabilityModel::default();
    let mut latency_distributions = LatencyDistributions::default();
    loop {
        tokio::select! {
            Some(fill) = fill_rx.recv() => {
                if fill.truth.simulated {
                    emit_structural_event(&fill);
                    continue;
                }
                pending.push_back(PendingFill {
                    created_at: fill.timestamp,
                    fill,
                    price_100ms: None,
                    price_500ms: None,
                    price_1s: None,
                    price_5s: None,
                });
            }
            Some(update) = market_rx.recv() => {
                if let Some((ts, price)) = market_price(update) {
                    update_pending(
                        &mut pending,
                        ts,
                        price,
                        &learning_tx,
                        &metrics,
                        &mut edge_reliability,
                        &mut latency_distributions,
                    );
                }
            }
            else => break,
        }
    }
}

fn update_pending(
    pending: &mut VecDeque<PendingFill>,
    market_ts: u64,
    market_price: f64,
    learning_tx: &Sender<LearningSample>,
    metrics: &Metrics,
    edge_reliability: &mut EdgeReliabilityModel,
    latency_distributions: &mut LatencyDistributions,
) {
    for item in pending.iter_mut() {
        let elapsed = market_ts.saturating_sub(item.created_at);
        if elapsed >= MARKOUT_100_MS && item.price_100ms.is_none() {
            item.price_100ms = Some(market_price);
        }
        if elapsed >= MARKOUT_500_MS && item.price_500ms.is_none() {
            item.price_500ms = Some(market_price);
        }
        if elapsed >= MARKOUT_1S && item.price_1s.is_none() {
            item.price_1s = Some(market_price);
        }
        if elapsed >= MARKOUT_5S && item.price_5s.is_none() {
            item.price_5s = Some(market_price);
        }
    }

    while pending.front().is_some_and(|item| {
        item.price_5s.is_some() || market_ts.saturating_sub(item.created_at) > MARKOUT_5S + 250
    }) {
        if let Some(item) = pending.pop_front() {
            let event = finalized_event(item);
            latency_distributions.record(event.latency_breakdown);
            let decision_quality = edge_reliability.observe(
                event.expected_markout,
                event.markout_curve.pnl_500ms,
            );
            let execution_quality = build_execution_quality_report(
                &event,
                latency_distributions.snapshot(),
            );
            let sample = learning_sample_from_event(&event);
            metrics
                .microtrade_pnl
                .with_label_values(&[&event.symbol])
                .observe(sample.pnl);
            let _ = learning_tx.try_send(sample);
            info!(
                symbol = event.symbol,
                fill_quality = event.fill_quality,
                slippage_real = event.slippage_real,
                adverse_selection_score = event.adverse_selection_score,
                latency_us = event.execution_latency_us,
                edge_reliability = decision_quality.edge_reliability_score,
                fill_rate = execution_quality.fill_rate,
                ?event.competition_flag,
                "execution truth event"
            );
        }
    }
}

fn finalized_event(item: PendingFill) -> ExecutionEvent {
    let fill = item.fill;
    let markout = MarkoutSnapshot {
        pnl_100ms: markout_pnl(&fill, item.price_100ms.unwrap_or(fill.price)),
        pnl_500ms: markout_pnl(&fill, item.price_500ms.unwrap_or(fill.price)),
        pnl_1s: markout_pnl(&fill, item.price_1s.unwrap_or(fill.price)),
        pnl_5s: markout_pnl(&fill, item.price_5s.unwrap_or(fill.price)),
    };
    let adverse = adverse_selection_score(&markout);
    let fill_quality = fill_quality(&fill, &markout, adverse);
    ExecutionEvent {
        symbol: fill.symbol.clone(),
        fill_quality,
        slippage_real: fill.actual_slippage_bps,
        adverse_selection_score: adverse,
        markout_curve: markout,
        execution_latency_us: fill.latency_us,
        latency_breakdown: fill.latency_breakdown,
        expected_markout: fill.expected_markout,
        competition_flag: competition_flag(&fill, &markout),
        truth: fill.truth,
    }
}

fn learning_sample_from_event(event: &ExecutionEvent) -> LearningSample {
    let side = if event.markout_curve.pnl_100ms >= 0.0 {
        Direction::Long
    } else {
        Direction::Short
    };
    LearningSample {
        timestamp: event.truth.last_fill_timestamp,
        direction: side,
        confidence: event.fill_quality,
        predicted_score: event.fill_quality,
        expected_slippage_bps: 0.0,
        actual_slippage_bps: event.slippage_real,
        pnl: event.markout_curve.pnl_500ms - event.slippage_real.max(0.0),
        duration_ms: (event.execution_latency_us / 1_000).max(1),
        entry_quality: event.fill_quality,
        markout_100ms: event.markout_curve.pnl_100ms,
        markout_500ms: event.markout_curve.pnl_500ms,
        markout_1s: event.markout_curve.pnl_1s,
        markout_5s: event.markout_curve.pnl_5s,
        regime: MarketRegime::default(),
    }
}

fn market_price(update: MarketUpdate) -> Option<(u64, f64)> {
    match update {
        MarketUpdate::Trade(trade) if trade.price > 0.0 => Some((trade.timestamp, trade.price)),
        MarketUpdate::BookDelta(book) => {
            let bid = book.bids.first()?.price;
            let ask = book.asks.first()?.price;
            if bid > 0.0 && ask > 0.0 {
                Some((book.timestamp, (bid + ask) * 0.5))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn markout_pnl(fill: &FillEvent, future_price: f64) -> f64 {
    let side = match fill.side {
        Side::Buy => 1.0,
        Side::Sell => -1.0,
    };
    (future_price - fill.price) * fill.filled_size * side
        - fill.fee
        + fill.rebate_amount
        - fill.funding_amount
}

fn adverse_selection_score(markout: &MarkoutSnapshot) -> f64 {
    let early = (-markout.pnl_100ms).max(0.0) * 0.65 + (-markout.pnl_500ms).max(0.0) * 0.35;
    (early / 10.0).clamp(0.0, 1.0)
}

fn fill_quality(fill: &FillEvent, markout: &MarkoutSnapshot, adverse: f64) -> f64 {
    let fill_ratio = if fill.size > 0.0 {
        (fill.filled_size / fill.size).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let markout_score = (0.5 + markout.pnl_500ms / 20.0).clamp(0.0, 1.0);
    (0.40 * fill_ratio + 0.35 * markout_score + 0.25 * (1.0 - adverse)).clamp(0.0, 1.0)
}

fn competition_flag(fill: &FillEvent, markout: &MarkoutSnapshot) -> CompetitionFlag {
    if fill.truth.queue_delay_us > 2_000 || fill.latency_us > 5_000 {
        return CompetitionFlag::SlowFill;
    }
    if fill.truth.partial_fill_ratio < 0.60 && markout.pnl_100ms < 0.0 {
        return CompetitionFlag::PartialFillToxicity;
    }
    CompetitionFlag::None
}

fn emit_structural_event(fill: &FillEvent) {
    info!(
        symbol = fill.symbol,
        latency_us = fill.latency_us,
        simulated = true,
        "structural paper fill ignored by execution learning"
    );
}

fn build_execution_quality_report(
    event: &ExecutionEvent,
    latency_distribution: LatencyDistributionSnapshot,
) -> ExecutionQualityReport {
    execution_quality_report(
        event.truth.partial_fill_ratio.clamp(0.0, 1.0),
        event.slippage_real,
        if matches!(event.competition_flag, CompetitionFlag::RepeatedOutbid) {
            1.0
        } else {
            0.0
        },
        latency_distribution,
    )
}
