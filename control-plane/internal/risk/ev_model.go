package risk

import (
	"math"
	"time"

	"control-plane/internal/domain"
)

type ExpectedValue struct {
	EV                      float64
	AdjustedEV              float64
	InclusionProbability    float64
	CompetitionAdjustedRisk float64
	TimeDecayFactor         float64
}

type ExpectedValueModel struct {
	minAdjustedEV float64
}

func NewExpectedValueModel() ExpectedValueModel {
	return ExpectedValueModel{minAdjustedEV: 0.0}
}

func (m ExpectedValueModel) Compute(
	req domain.ExecutionRequest,
	price float64,
	latency time.Duration,
	snap IntelligenceSnapshot,
) ExpectedValue {
	if price <= 0 || req.Size <= 0 {
		return ExpectedValue{CompetitionAdjustedRisk: 1.0}
	}
	target := price
	if req.Price != nil {
		target = *req.Price
	}
	priceDeviationBps := math.Abs(target-price) / price * 10_000.0
	slippageEstimate := req.MaxSlippageBps
	if slippageEstimate <= 0 {
		slippageEstimate = 3.0
	}
	notional := req.Size * price
	sideEdge := 1.8 - priceDeviationBps*0.12 - slippageEstimate*0.20
	rawEV := notional * sideEdge / 10_000.0
	decay := DecayFactor(latency, defaultDecayTauMs)
	inclusion := clamp01(snap.HistoricalInclusionRate * decay * (1.0 - snap.MempoolPressureScore*0.45))
	competitionRisk := clamp01(
		0.42*snap.CompetitionIntensityScore +
			0.28*snap.OutbidLikelihoodIndex +
			0.18*snap.LatencyAdvantagePenalty +
			0.12*snap.MempoolPressureScore,
	)
	competitionPenalty := notional * competitionRisk * slippageEstimate / 10_000.0
	riskPenalty := notional * (0.55*snap.SystemStressIndex + 0.45*snap.ExecutionFragilityScore) / 10_000.0
	adjusted := rawEV*inclusion - competitionPenalty - riskPenalty
	return ExpectedValue{
		EV:                      rawEV,
		AdjustedEV:              adjusted,
		InclusionProbability:    inclusion,
		CompetitionAdjustedRisk: competitionRisk,
		TimeDecayFactor:         decay,
	}
}
