use crate::{
    metrics::Metrics,
    model::LogisticFilter,
    types::{Decision, Direction, Event, FeatureFrame, MarketRegime, ScoredDecision},
};
use std::{sync::Arc, time::Instant};
use tokio::sync::mpsc::{Receiver, Sender};

pub async fn run(
    mut rx: Receiver<FeatureFrame>,
    tx: Sender<ScoredDecision>,
    metrics: Arc<Metrics>,
) {
    let model = LogisticFilter::default();
    let mut previous_score = 0.5;

    while let Some(frame) = rx.recv().await {
        let started = Instant::now();
        let direction = match frame.event {
            Event::PumpDetected => 1.0,
            Event::DumpDetected => -1.0,
            Event::Neutral => frame.market.side_factor(),
        };
        let raw = 1.45 * frame.features.velocity * direction
            + 0.55 * frame.features.vol_z
            + 0.95 * frame.features.imbalance * direction
            + 0.20 * frame.features.volatility
            - 1.25 * frame.features.spread;
        let base_score = sigmoid(raw);
        let (continuation_prob, reversal_prob) = model.probabilities(&frame.features);
        let filtered_score = (0.75 * base_score + 0.25 * continuation_prob).clamp(0.0, 1.0);

        let decision = if filtered_score < 0.36 || previous_score - filtered_score > 0.18 {
            Decision::Exit
        } else if filtered_score > 0.70 && filtered_score > previous_score + 0.035 {
            Decision::ScaleIn
        } else if filtered_score > 0.70 {
            Decision::EnterSmall
        } else {
            Decision::Ignore
        };

        previous_score = filtered_score;
        metrics
            .decisions_total
            .with_label_values(&[decision_label(decision)])
            .inc();
        metrics
            .stage_latency_us
            .with_label_values(&["decision"])
            .observe(started.elapsed().as_micros() as f64);

        let expected_slippage_bps = frame.market.spread * 10_000.0 * 0.5;
        let output = ScoredDecision {
            market: frame.market,
            event: frame.event,
            features: frame.features,
            regime: MarketRegime::default(),
            direction: match frame.event {
                Event::PumpDetected => Direction::Long,
                Event::DumpDetected => Direction::Short,
                Event::Neutral => Direction::Flat,
            },
            confidence: ((filtered_score - 0.5).abs() * 2.0).clamp(0.0, 1.0),
            continuation_prob,
            reversal_prob,
            score: filtered_score,
            decision,
            expected_duration_ms: 500,
            urgency: (filtered_score - 0.65).max(0.0).clamp(0.0, 1.0),
            expected_slippage_bps,
            data_latency_ms: 0,
            adversarial_risk: 0.0,
        };
        if tx.try_send(output.clone()).is_err() {
            metrics
                .channel_backpressure_total
                .with_label_values(&["decision"])
                .inc();
            if tx.send(output).await.is_err() {
                break;
            }
        }
    }
}

trait SideFactor {
    fn side_factor(&self) -> f64;
}

impl SideFactor for crate::types::MarketEvent {
    #[inline]
    fn side_factor(&self) -> f64 {
        match self.side {
            crate::types::Side::Buy => 1.0,
            crate::types::Side::Sell => -1.0,
        }
    }
}

#[inline]
fn sigmoid(x: f64) -> f64 {
    if x >= 0.0 {
        1.0 / (1.0 + (-x).exp())
    } else {
        let z = x.exp();
        z / (1.0 + z)
    }
}

#[inline]
fn decision_label(decision: Decision) -> &'static str {
    match decision {
        Decision::Ignore => "ignore",
        Decision::EnterSmall => "enter_small",
        Decision::ScaleIn => "scale_in",
        Decision::Exit => "exit",
    }
}
