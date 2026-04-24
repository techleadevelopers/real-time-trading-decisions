use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LatencyBreakdown {
    pub decision_latency_us: u64,
    pub send_latency_us: u64,
    pub ack_latency_us: u64,
    pub first_fill_latency_us: u64,
    pub full_fill_latency_us: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LatencyPercentiles {
    pub p50: u64,
    pub p90: u64,
    pub p99: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LatencyDistributionSnapshot {
    pub decision_latency: LatencyPercentiles,
    pub send_latency: LatencyPercentiles,
    pub ack_latency: LatencyPercentiles,
    pub first_fill_latency: LatencyPercentiles,
    pub full_fill_latency: LatencyPercentiles,
}

#[derive(Clone, Debug)]
pub struct LatencyDistributions {
    capacity: usize,
    decision: VecDeque<u64>,
    send: VecDeque<u64>,
    ack: VecDeque<u64>,
    first_fill: VecDeque<u64>,
    full_fill: VecDeque<u64>,
}

impl Default for LatencyDistributions {
    fn default() -> Self {
        Self::new(512)
    }
}

impl LatencyDistributions {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(32);
        Self {
            capacity,
            decision: VecDeque::with_capacity(capacity),
            send: VecDeque::with_capacity(capacity),
            ack: VecDeque::with_capacity(capacity),
            first_fill: VecDeque::with_capacity(capacity),
            full_fill: VecDeque::with_capacity(capacity),
        }
    }

    pub fn record(&mut self, breakdown: LatencyBreakdown) {
        push_capped(&mut self.decision, breakdown.decision_latency_us, self.capacity);
        push_capped(&mut self.send, breakdown.send_latency_us, self.capacity);
        push_capped(&mut self.ack, breakdown.ack_latency_us, self.capacity);
        push_capped(
            &mut self.first_fill,
            breakdown.first_fill_latency_us,
            self.capacity,
        );
        push_capped(
            &mut self.full_fill,
            breakdown.full_fill_latency_us,
            self.capacity,
        );
    }

    pub fn snapshot(&self) -> LatencyDistributionSnapshot {
        LatencyDistributionSnapshot {
            decision_latency: summarize(&self.decision),
            send_latency: summarize(&self.send),
            ack_latency: summarize(&self.ack),
            first_fill_latency: summarize(&self.first_fill),
            full_fill_latency: summarize(&self.full_fill),
        }
    }
}

pub fn latency_impact_score(
    distribution: &LatencyDistributionSnapshot,
    slippage_bps: f64,
    fill_quality: f64,
) -> f64 {
    let decision_penalty = normalize_us(distribution.decision_latency.p90, 2_000);
    let send_penalty = normalize_us(distribution.send_latency.p90, 4_000);
    let ack_penalty = normalize_us(distribution.ack_latency.p90, 5_000);
    let first_fill_penalty = normalize_us(distribution.first_fill_latency.p99, 8_000);
    let full_fill_penalty = normalize_us(distribution.full_fill_latency.p99, 12_000);
    let slip_penalty = (slippage_bps.abs() / 12.0).clamp(0.0, 1.0);
    let fill_quality_penalty = (1.0 - fill_quality).clamp(0.0, 1.0);
    (0.20 * decision_penalty
        + 0.12 * send_penalty
        + 0.12 * ack_penalty
        + 0.22 * first_fill_penalty
        + 0.18 * full_fill_penalty
        + 0.08 * slip_penalty
        + 0.08 * fill_quality_penalty)
        .clamp(0.0, 1.0)
}

fn push_capped(queue: &mut VecDeque<u64>, value: u64, capacity: usize) {
    if queue.len() == capacity {
        queue.pop_front();
    }
    queue.push_back(value);
}

fn summarize(values: &VecDeque<u64>) -> LatencyPercentiles {
    if values.is_empty() {
        return LatencyPercentiles::default();
    }
    let mut sorted: Vec<u64> = values.iter().copied().collect();
    sorted.sort_unstable();
    LatencyPercentiles {
        p50: percentile(&sorted, 0.50),
        p90: percentile(&sorted, 0.90),
        p99: percentile(&sorted, 0.99),
    }
}

fn percentile(sorted: &[u64], q: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn normalize_us(value: u64, budget: u64) -> f64 {
    if budget == 0 {
        return 1.0;
    }
    (value as f64 / budget as f64).clamp(0.0, 1.0)
}
