use crate::{
    accounting::edge_validation::{EdgeState, EdgeValidationSnapshot},
    model_weights,
    types::{CompetitionState, EntryScoringSnapshot, MicrostructureFrame},
};

#[derive(Clone, Debug, Default)]
pub struct EntryScoring;

impl EntryScoring {
    #[inline]
    pub fn evaluate(
        frame: &MicrostructureFrame,
        edge_snapshot: &EdgeValidationSnapshot,
    ) -> EntryScoringSnapshot {
        let weights = model_weights::current();
        let params = weights.entry_scoring;
        let classifier = frame.reversal_classifier;
        let directional_gate = match frame.reversal.state {
            crate::types::ReversalState::ObserveLongToShort => classifier.ready_short,
            crate::types::ReversalState::ObserveShortToLong => classifier.ready_long,
            crate::types::ReversalState::Idle => classifier.ready_long,
        };
        let edge_gate = edge_snapshot.edge_state != EdgeState::Invalid && edge_snapshot.trading_enabled;
        let competition_gate = edge_snapshot.competition_state != CompetitionState::Saturated;
        let context_gate = frame.context.regime != crate::types::RegimeKind::TrendExpansion;
        let score = (
            params.reversal_probability * classifier.reversal_probability
                + params.intent_score * classifier.intent_score
                + params.trigger_edge * frame.trigger.expected_edge.min(1.0)
                + params.reversal_edge * frame.reversal.expected_edge.min(1.0)
                + params.edge_reliability * edge_snapshot.edge_reliability_score
        )
            .clamp(0.0, 1.0);
        let confidence = (0.55 * classifier.intent_score
            + 0.25 * edge_snapshot.edge_reliability_score
            + 0.20 * (1.0 - edge_snapshot.competition_score))
            .clamp(0.0, 1.0);
        let expected_edge = (
            frame.trigger.expected_edge.max(frame.reversal.expected_edge)
                * classifier.reversal_probability
                * edge_snapshot.edge_reliability_score.max(0.1)
        )
            .max(0.0);
        let enter = directional_gate
            && edge_gate
            && competition_gate
            && context_gate
            && classifier.reversal_probability > classifier.continuation_probability
            && score > params.min_enter_score
            && confidence > params.min_enter_confidence;

        EntryScoringSnapshot {
            enter_long: enter && classifier.ready_long,
            enter_short: enter && classifier.ready_short,
            wait: !enter
                && classifier.reversal_probability > params.min_wait_probability
                && edge_gate
                && competition_gate,
            score,
            confidence,
            expected_edge,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        accounting::edge_validation::{EdgeRegime, EdgeValidationSnapshot},
        types::{
            CompetitionState, EntryScoringSnapshot as EntrySnapshot, Features, FlowSignal, FlowState,
            MarketContext, MarketRegime, MicroTimingState, OrderBookState, RegimeKind,
            ReversalClassifierSnapshot, ReversalSnapshot, ReversalState, Side, TapeState, TradeEvent,
            TriggerSnapshot,
        },
    };

    fn frame() -> MicrostructureFrame {
        MicrostructureFrame {
            timestamp: 1,
            trade: Some(TradeEvent { timestamp: 1, price: 100.0, volume: 1.0, side: Side::Sell }),
            book: OrderBookState::default(),
            tape: TapeState::default(),
            features: Features::default(),
            regime: MarketRegime::default(),
            context: MarketContext {
                regime: RegimeKind::Normal,
                volatility: 0.5,
                liquidity_score: 0.8,
                stability_score: 0.8,
            },
            flow: FlowState {
                signal: FlowSignal::Exhaustion,
                aggressive_ratio: 0.3,
                absorption: 0.3,
                exhaustion: 0.6,
                continuation_strength: 0.2,
            },
            timing: MicroTimingState::default(),
            trigger: TriggerSnapshot { expected_edge: 0.7, ..TriggerSnapshot::default() },
            reversal: ReversalSnapshot {
                state: ReversalState::ObserveShortToLong,
                active: true,
                confirmed: false,
                direction: crate::types::Direction::Long,
                confidence: 0.7,
                expected_edge: 0.6,
            },
            reversal_classifier: ReversalClassifierSnapshot {
                reversal_probability: 0.75,
                continuation_probability: 0.25,
                chop_probability: 0.15,
                intent_score: 0.8,
                movement_score: 0.5,
                ready_long: true,
                ready_short: false,
            },
            entry_scoring: EntrySnapshot::default(),
            stale: false,
        }
    }

    fn edge_snapshot() -> EdgeValidationSnapshot {
        EdgeValidationSnapshot {
            edge_state: EdgeState::Valid,
            edge_regime: EdgeRegime::Stable,
            competition_state: CompetitionState::Normal,
            edge_reliability_score: 0.8,
            competition_score: 0.2,
            trading_enabled: true,
            ..EdgeValidationSnapshot::default()
        }
    }

    #[test]
    fn entry_scoring_prefers_valid_reversal_context() {
        let scored = EntryScoring::evaluate(&frame(), &edge_snapshot());
        assert!(scored.enter_long);
        assert!(scored.score > 0.58);
    }
}
