package risk

import (
	"math"
	"sync/atomic"
	"time"

	"control-plane/internal/config"
	"control-plane/internal/domain"
	"control-plane/internal/state"
)

type CircuitState uint32

const (
	CircuitClosed CircuitState = iota
	CircuitDegraded
	CircuitOpen
)

func (s CircuitState) String() string {
	switch s {
	case CircuitOpen:
		return "OPEN"
	case CircuitDegraded:
		return "DEGRADED"
	default:
		return "CLOSED"
	}
}

type RiskDecision struct {
	Allowed        bool
	SizeMultiplier float64
	ExpectedValue  ExpectedValue
	RiskScore      float64
	Reason         string
	Action         string
	ReasonCategory FailureCategory
	RegimeState    RegimeState
	Confidence     float64
}

type IntelligenceSnapshot struct {
	ExposureRiskScore         float64
	MempoolPressureScore      float64
	ExecutionFragilityScore   float64
	SystemStressIndex         float64
	RiskMultiplier            float64
	ExpectedValue             ExpectedValue
	HistoricalInclusionRate   float64
	CompetitionIntensityScore float64
	LatencyAdvantagePenalty   float64
	OutbidLikelihoodIndex     float64
	RejectionCount            uint64
	CircuitState              CircuitState
	Feedback                  FeedbackSnapshot
}

type IntelligenceEngine struct {
	cfg      config.RiskConfig
	store    *state.Store
	mempool  MempoolPressureModel
	evModel  ExpectedValueModel
	feedback *FeedbackEngine
	snapshot atomic.Value // IntelligenceSnapshot

	lastPrice      float64
	lastMarketTime time.Time
	volEMA         float64
	latencyEMA     float64
	rejectEMA      float64
	failEMA        float64
	rejections     atomic.Uint64
	failures       atomic.Uint64
	successes      atomic.Uint64
	inclusionEMA   float64
	circuit        atomic.Uint32
}

func NewIntelligenceEngine(cfg config.RiskConfig, store *state.Store, feedback *FeedbackEngine) *IntelligenceEngine {
	e := &IntelligenceEngine{cfg: cfg, store: store, evModel: NewExpectedValueModel(), feedback: feedback, inclusionEMA: 0.50}
	e.snapshot.Store(IntelligenceSnapshot{RiskMultiplier: 1.0, CircuitState: CircuitClosed, HistoricalInclusionRate: 0.50, Feedback: feedback.Snapshot()})
	return e
}

func (e *IntelligenceEngine) ObserveMarket(event domain.MarketEvent, marks map[string]float64) IntelligenceSnapshot {
	price := event.Price
	if price <= 0 {
		price = (event.BestBid + event.BestAsk) * 0.5
	}
	if price <= 0 {
		return e.Snapshot()
	}
	now := event.Timestamp
	if now.IsZero() {
		now = time.Now().UTC()
	}
	velocity := 0.0
	if e.lastPrice > 0 && !e.lastMarketTime.IsZero() {
		dt := now.Sub(e.lastMarketTime).Seconds()
		if dt > 0 {
			velocity = ((price - e.lastPrice) / e.lastPrice) / dt
			e.volEMA = ema(e.volEMA, math.Abs(velocity)*1000.0, 0.05)
		}
	}
	e.lastPrice = price
	e.lastMarketTime = now
	mempool := e.mempool.ObserveMarket(event.Symbol, event.Volume, velocity)
	return e.recompute(marks, mempool)
}

func (e *IntelligenceEngine) ObserveExecution(latency time.Duration, rejected bool) IntelligenceSnapshot {
	e.latencyEMA = ema(e.latencyEMA, float64(latency.Microseconds())/1000.0, 0.08)
	e.mempool.ObserveLatencyPenalty(1.0 - DecayFactor(latency, defaultDecayTauMs))
	if rejected {
		e.rejections.Add(1)
		e.failures.Add(1)
		e.rejectEMA = ema(e.rejectEMA, 1.0, 0.10)
		e.failEMA = ema(e.failEMA, 1.0, 0.10)
		e.inclusionEMA = ema(e.inclusionEMA, 0.0, 0.06)
		return e.recompute(nil, e.mempool.ObserveExecutionFailure())
	}
	e.successes.Add(1)
	e.inclusionEMA = ema(e.inclusionEMA, 1.0, 0.04)
	e.rejectEMA = ema(e.rejectEMA, 0.0, 0.04)
	e.failEMA = ema(e.failEMA, 0.0, 0.04)
	return e.recompute(nil, e.mempool.ObserveExecutionSuccess())
}

func (e *IntelligenceEngine) Snapshot() IntelligenceSnapshot {
	if snap, ok := e.snapshot.Load().(IntelligenceSnapshot); ok {
		return snap
	}
	return IntelligenceSnapshot{RiskMultiplier: 1.0, CircuitState: CircuitClosed}
}

func (e *IntelligenceEngine) Evaluate(req domain.ExecutionRequest, base RiskDecision) RiskDecision {
	snap := e.Snapshot()
	if !base.Allowed {
		return base
	}
	if snap.CircuitState == CircuitOpen || snap.SystemStressIndex > 0.85 {
		return decision(false, 0, snap.ExpectedValue, snap.SystemStressIndex, "system_stress_open", FailureExecutionFail, snap.Feedback.RegimeState, confidence(snap, snap.Feedback))
	}
	price := 0.0
	if req.Price != nil {
		price = *req.Price
	}
	if price <= 0 && e.lastPrice > 0 {
		price = e.lastPrice
	}
	latency := time.Since(req.SignalTime)
	evNow := e.evModel.Compute(req, price, latency, snap)
	evFinal := e.evModel.Compute(req, price, latency+50*time.Millisecond, snap)
	realizedThreshold := riskAdjustedThreshold(snap, snap.Feedback)
	expectedRealizedMarkout := req.ExpectedRealizedMarkout
	if expectedRealizedMarkout == 0 {
		expectedRealizedMarkout = snap.Feedback.ExpectedRealizedMarkoutEMA
	}
	if !req.ReduceOnly && expectedRealizedMarkout <= realizedThreshold {
		return decision(false, 0, evFinal, snap.SystemStressIndex, "expected_real_markout_below_threshold", FailureRiskReject, snap.Feedback.RegimeState, confidence(snap, snap.Feedback))
	}
	if snap.Feedback.RegimeState == RegimeHostile && !req.ReduceOnly {
		return decision(false, 0, evFinal, snap.SystemStressIndex, "hostile_execution_regime", FailureRiskReject, snap.Feedback.RegimeState, confidence(snap, snap.Feedback))
	}
	if snap.Feedback.RegimeState == RegimeContaminated && !req.ReduceOnly {
		return decision(false, 0, evFinal, snap.SystemStressIndex, "execution_truth_contaminated", FailureRiskReject, snap.Feedback.RegimeState, 0.05)
	}
	mult := snap.RiskMultiplier
	reason := "allowed"
	action := "allow"
	category := FailureCategory("")
	if snap.Feedback.RegimeState == RegimeDegraded {
		mult = min(mult, 0.55)
		reason = "regime_degraded"
		action = "degrade"
	}
	if evFinal.AdjustedEV < evNow.EV*0.15 {
		mult = min(mult, 0.55)
		reason = "marginal_ev_reduce"
		action = "degrade"
	}
	if evFinal.TimeDecayFactor < 0.55 {
		mult = min(mult, 0.45)
		reason = "time_decay_reduce"
		action = "degrade"
	}
	if snap.CircuitState == CircuitDegraded {
		mult = min(mult, 0.50)
		reason = "circuit_degraded"
		action = "degrade"
	}
	if snap.MempoolPressureScore > 0.65 {
		mult = min(mult, 0.65)
		reason = "mempool_pressure_reduce"
		action = "degrade"
	}
	if snap.ExecutionFragilityScore > 0.55 {
		mult = min(mult, 0.55)
		reason = "execution_fragility_reduce"
		action = "degrade"
	}
	if snap.ExposureRiskScore > 0.70 && !req.ReduceOnly {
		mult = min(mult, 0.50)
		reason = "exposure_risk_reduce"
		action = "degrade"
	}
	if mult <= 0.05 && !req.ReduceOnly {
		return decision(false, 0, evFinal, snap.SystemStressIndex, "risk_multiplier_zero", FailureRiskReject, snap.Feedback.RegimeState, confidence(snap, snap.Feedback))
	}
	out := decision(true, mult, evFinal, snap.SystemStressIndex, reason, category, snap.Feedback.RegimeState, confidence(snap, snap.Feedback))
	out.Action = action
	return out
}

func (e *IntelligenceEngine) recompute(marks map[string]float64, mempool float64) IntelligenceSnapshot {
	exposure := 0.0
	if marks != nil {
		exposure = clamp01(e.store.GrossExposureUSD(marks) / e.cfg.MaxExposureUSD)
	} else {
		exposure = e.Snapshot().ExposureRiskScore
	}
	feedback := e.feedback.Snapshot()
	fragility := clamp01(0.30*clamp01(e.latencyEMA/float64(e.cfg.LatencyRejectAfter.Milliseconds()+1)) + 0.25*e.failEMA + 0.15*e.rejectEMA + 0.30*feedback.ExecutionFailureRateEMA)
	vol := clamp01(e.volEMA / 8.0)
	markoutStress := clamp01(0.35*feedback.MarkoutDegradationScore + 0.30*feedback.AdverseSelectionEMA + 0.20*clamp01(feedback.SlippageVarianceEMA/36.0) + 0.15*feedback.PartialFillLossRateEMA)
	stress := clamp01(0.20*exposure + 0.20*mempool + 0.25*fragility + 0.15*vol + 0.20*markoutStress)
	state := CircuitClosed
	if stress > 0.90 || e.failEMA > 0.75 || feedback.RegimeState == RegimeHostile {
		state = CircuitOpen
	} else if stress > 0.55 || (vol > 0.45 && mempool > 0.45) || feedback.RegimeState == RegimeDegraded {
		state = CircuitDegraded
	}
	mult := clamp01(1.0 - stress*0.65)
	if state == CircuitDegraded {
		mult = min(mult, 0.60)
	}
	if state == CircuitOpen {
		mult = 0
	}
	snap := IntelligenceSnapshot{
		ExposureRiskScore:         exposure,
		MempoolPressureScore:      mempool,
		ExecutionFragilityScore:   fragility,
		SystemStressIndex:         stress,
		RiskMultiplier:            mult,
		HistoricalInclusionRate:   clamp01(e.inclusionEMA),
		CompetitionIntensityScore: e.mempool.CompetitionIntensityScore(),
		LatencyAdvantagePenalty:   e.mempool.LatencyAdvantagePenalty(),
		OutbidLikelihoodIndex:     e.mempool.OutbidLikelihoodIndex(),
		RejectionCount:            e.rejections.Load(),
		CircuitState:              state,
		Feedback:                  feedback,
	}
	e.circuit.Store(uint32(state))
	e.snapshot.Store(snap)
	return snap
}

func riskAdjustedThreshold(snap IntelligenceSnapshot, feedback FeedbackSnapshot) float64 {
	return 0.001 +
		0.20*snap.MempoolPressureScore +
		0.18*snap.LatencyAdvantagePenalty +
		0.20*clamp01(feedback.SlippageVarianceEMA/36.0) +
		0.22*feedback.MarkoutDegradationScore +
		0.20*feedback.AdverseSelectionEMA
}

func confidence(snap IntelligenceSnapshot, feedback FeedbackSnapshot) float64 {
	dataConfidence := 0.30
	if feedback.RealEvents >= 20 {
		dataConfidence = 0.80
	} else if feedback.RealEvents > 0 {
		dataConfidence = 0.45 + float64(feedback.RealEvents)*0.015
	}
	contaminationPenalty := 0.0
	total := feedback.RealEvents + feedback.ContaminatedEvents
	if total > 0 {
		contaminationPenalty = float64(feedback.ContaminatedEvents) / float64(total)
	}
	return clamp01(dataConfidence * (1.0 - contaminationPenalty) * (1.0 - snap.SystemStressIndex*0.35))
}

func decision(allowed bool, mult float64, ev ExpectedValue, riskScore float64, reason string, category FailureCategory, regime RegimeState, conf float64) RiskDecision {
	action := "deny"
	if allowed {
		action = "allow"
		if mult > 0 && mult < 1 {
			action = "degrade"
		}
	}
	return RiskDecision{
		Allowed:        allowed,
		SizeMultiplier: mult,
		ExpectedValue:  ev,
		RiskScore:      riskScore,
		Reason:         reason,
		Action:         action,
		ReasonCategory: category,
		RegimeState:    regime,
		Confidence:     conf,
	}
}

func ema(prev, value, alpha float64) float64 {
	if prev == 0 {
		return value
	}
	return prev*(1-alpha) + value*alpha
}

func clamp01(v float64) float64 {
	if v < 0 {
		return 0
	}
	if v > 1 {
		return 1
	}
	return v
}

func min(a, b float64) float64 {
	if a < b {
		return a
	}
	return b
}
