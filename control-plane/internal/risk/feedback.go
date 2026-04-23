package risk

import (
	"strings"
	"sync/atomic"

	"control-plane/internal/domain"
)

type FailureCategory string

const (
	FailureRiskReject      FailureCategory = "risk_reject"
	FailureExecutionFail   FailureCategory = "execution_fail"
	FailureOutbidFail      FailureCategory = "outbid_fail"
	FailureStaleSkip       FailureCategory = "stale_skip"
	FailureMarketCancel    FailureCategory = "market_cancel"
	FailurePartialFillLoss FailureCategory = "partial_fill_loss"
)

type RegimeState string

const (
	RegimeSafe         RegimeState = "SAFE"
	RegimeDegraded     RegimeState = "DEGRADED"
	RegimeHostile      RegimeState = "HOSTILE"
	RegimeContaminated RegimeState = "CONTAMINATED"
)

type FeedbackSnapshot struct {
	RealEvents                 uint64
	ContaminatedEvents         uint64
	RiskRejects                uint64
	ExecutionFails             uint64
	OutbidFails                uint64
	StaleSkips                 uint64
	MarketCancels              uint64
	PartialFillLosses          uint64
	RealizedPnLEMA             float64
	Markout100EMA              float64
	Markout500EMA              float64
	Markout1sEMA               float64
	Markout5sEMA               float64
	MarkoutDegradationScore    float64
	AdverseSelectionEMA        float64
	SlippageEMA                float64
	SlippageVarianceEMA        float64
	ExecutionFailureRateEMA    float64
	OutbidRateEMA              float64
	PartialFillLossRateEMA     float64
	ExpectedRealizedMarkoutEMA float64
	RegimeState                RegimeState
}

type FeedbackEngine struct {
	snapshot atomic.Value // FeedbackSnapshot

	realEvents        atomic.Uint64
	contaminated      atomic.Uint64
	riskRejects       atomic.Uint64
	executionFails    atomic.Uint64
	outbidFails       atomic.Uint64
	staleSkips        atomic.Uint64
	marketCancels     atomic.Uint64
	partialFillLosses atomic.Uint64

	realizedPnLEMA             float64
	markout100EMA              float64
	markout500EMA              float64
	markout1sEMA               float64
	markout5sEMA               float64
	adverseSelectionEMA        float64
	slippageEMA                float64
	slippageVarianceEMA        float64
	executionFailureRateEMA    float64
	outbidRateEMA              float64
	partialFillLossRateEMA     float64
	expectedRealizedMarkoutEMA float64
}

func NewFeedbackEngine() *FeedbackEngine {
	e := &FeedbackEngine{}
	e.snapshot.Store(FeedbackSnapshot{RegimeState: RegimeSafe})
	return e
}

func (e *FeedbackEngine) Snapshot() FeedbackSnapshot {
	if snap, ok := e.snapshot.Load().(FeedbackSnapshot); ok {
		return snap
	}
	return FeedbackSnapshot{RegimeState: RegimeSafe}
}

func (e *FeedbackEngine) ObserveFailure(category FailureCategory) FeedbackSnapshot {
	switch category {
	case FailureRiskReject:
		e.riskRejects.Add(1)
	case FailureExecutionFail:
		e.executionFails.Add(1)
		e.executionFailureRateEMA = ema(e.executionFailureRateEMA, 1.0, 0.08)
	case FailureOutbidFail:
		e.outbidFails.Add(1)
		e.outbidRateEMA = ema(e.outbidRateEMA, 1.0, 0.10)
	case FailureStaleSkip:
		e.staleSkips.Add(1)
	case FailureMarketCancel:
		e.marketCancels.Add(1)
		e.executionFailureRateEMA = ema(e.executionFailureRateEMA, 0.65, 0.08)
	case FailurePartialFillLoss:
		e.partialFillLosses.Add(1)
		e.partialFillLossRateEMA = ema(e.partialFillLossRateEMA, 1.0, 0.10)
	}
	return e.recompute()
}

func (e *FeedbackEngine) ObserveExecutionEvent(event domain.ExecutionEvent) FeedbackSnapshot {
	if event.Simulated {
		e.contaminated.Add(1)
		return e.recompute()
	}
	e.realEvents.Add(1)
	alpha := 0.06
	realized := event.RealizedPnL
	if realized == 0 {
		realized = event.MarkoutCurve.PnL500ms
	}
	e.realizedPnLEMA = ema(e.realizedPnLEMA, realized, alpha)
	e.markout100EMA = ema(e.markout100EMA, event.MarkoutCurve.PnL100ms, alpha)
	e.markout500EMA = ema(e.markout500EMA, event.MarkoutCurve.PnL500ms, alpha)
	e.markout1sEMA = ema(e.markout1sEMA, event.MarkoutCurve.PnL1s, alpha)
	e.markout5sEMA = ema(e.markout5sEMA, event.MarkoutCurve.PnL5s, alpha)
	e.adverseSelectionEMA = ema(e.adverseSelectionEMA, event.AdverseSelectionScore, 0.08)
	delta := event.SlippageReal - e.slippageEMA
	e.slippageEMA = ema(e.slippageEMA, event.SlippageReal, 0.08)
	e.slippageVarianceEMA = ema(e.slippageVarianceEMA, delta*delta, 0.06)
	e.executionFailureRateEMA = ema(e.executionFailureRateEMA, 0.0, 0.04)
	e.outbidRateEMA = ema(e.outbidRateEMA, 0.0, 0.04)
	e.partialFillLossRateEMA = ema(e.partialFillLossRateEMA, 0.0, 0.04)
	if event.ExpectedRealizedMarkout != 0 {
		e.expectedRealizedMarkoutEMA = ema(e.expectedRealizedMarkoutEMA, event.ExpectedRealizedMarkout, 0.05)
	}

	flag := strings.ToLower(event.CompetitionFlag)
	if strings.Contains(flag, "outbid") {
		e.outbidFails.Add(1)
		e.outbidRateEMA = ema(e.outbidRateEMA, 1.0, 0.10)
	}
	if event.PartialFillRatio > 0 && event.PartialFillRatio < 0.70 && event.MarkoutCurve.PnL500ms < 0 {
		e.partialFillLosses.Add(1)
		e.partialFillLossRateEMA = ema(e.partialFillLossRateEMA, 1.0, 0.10)
	}
	return e.recompute()
}

func (e *FeedbackEngine) recompute() FeedbackSnapshot {
	degradation := 0.0
	if e.markout100EMA > e.markout500EMA {
		degradation += clamp01((e.markout100EMA-e.markout500EMA)/absNonZero(e.markout100EMA, 1.0)) * 0.35
	}
	if e.markout500EMA > e.markout1sEMA {
		degradation += clamp01((e.markout500EMA-e.markout1sEMA)/absNonZero(e.markout500EMA, 1.0)) * 0.30
	}
	if e.markout1sEMA > e.markout5sEMA {
		degradation += clamp01((e.markout1sEMA-e.markout5sEMA)/absNonZero(e.markout1sEMA, 1.0)) * 0.25
	}
	if e.markout500EMA < 0 && e.markout1sEMA < 0 {
		degradation += 0.25
	}
	degradation = clamp01(degradation)

	regime := RegimeSafe
	if e.realEvents.Load() == 0 && e.contaminated.Load() > 0 {
		regime = RegimeContaminated
	} else if e.adverseSelectionEMA > 0.72 || e.slippageVarianceEMA > 36 || (e.markout500EMA < 0 && e.markout1sEMA < 0 && e.markout5sEMA < 0) {
		regime = RegimeHostile
	} else if degradation > 0.45 || e.executionFailureRateEMA > 0.35 || e.outbidRateEMA > 0.30 || e.partialFillLossRateEMA > 0.25 {
		regime = RegimeDegraded
	}

	snap := FeedbackSnapshot{
		RealEvents:                 e.realEvents.Load(),
		ContaminatedEvents:         e.contaminated.Load(),
		RiskRejects:                e.riskRejects.Load(),
		ExecutionFails:             e.executionFails.Load(),
		OutbidFails:                e.outbidFails.Load(),
		StaleSkips:                 e.staleSkips.Load(),
		MarketCancels:              e.marketCancels.Load(),
		PartialFillLosses:          e.partialFillLosses.Load(),
		RealizedPnLEMA:             e.realizedPnLEMA,
		Markout100EMA:              e.markout100EMA,
		Markout500EMA:              e.markout500EMA,
		Markout1sEMA:               e.markout1sEMA,
		Markout5sEMA:               e.markout5sEMA,
		MarkoutDegradationScore:    degradation,
		AdverseSelectionEMA:        e.adverseSelectionEMA,
		SlippageEMA:                e.slippageEMA,
		SlippageVarianceEMA:        e.slippageVarianceEMA,
		ExecutionFailureRateEMA:    e.executionFailureRateEMA,
		OutbidRateEMA:              e.outbidRateEMA,
		PartialFillLossRateEMA:     e.partialFillLossRateEMA,
		ExpectedRealizedMarkoutEMA: e.expectedRealizedMarkoutEMA,
		RegimeState:                regime,
	}
	e.snapshot.Store(snap)
	return snap
}

func absNonZero(v, fallback float64) float64 {
	if v < 0 {
		v = -v
	}
	if v <= 1e-9 {
		return fallback
	}
	return v
}
