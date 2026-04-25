package risk

import (
	"errors"
	"math"
	"sync"
	"sync/atomic"
	"time"

	"control-plane/internal/config"
	"control-plane/internal/domain"
	"control-plane/internal/state"
)

var ErrRiskRejected = errors.New("risk rejected")

type Service struct {
	mu            sync.RWMutex
	cfg           config.RiskConfig
	store         *state.Store
	kill          atomic.Bool
	forcedBreaker atomic.Bool
	breakers      map[string]bool
	dailyPnLBits  atomic.Uint64
	lastMarkBits  atomic.Uint64
	marks         map[string]float64
	intelligence  *IntelligenceEngine
	feedback      *FeedbackEngine
}

func NewService(cfg config.RiskConfig, store *state.Store) *Service {
	feedback := NewFeedbackEngine()
	return &Service{
		cfg:          cfg,
		store:        store,
		breakers:     make(map[string]bool),
		marks:        make(map[string]float64),
		feedback:     feedback,
		intelligence: NewIntelligenceEngine(cfg, store, feedback),
	}
}

func (s *Service) ObserveMarket(event domain.MarketEvent) {
	price := event.Price
	if price <= 0 {
		price = (event.BestBid + event.BestAsk) * 0.5
	}
	if price <= 0 {
		return
	}
	s.mu.Lock()
	s.marks[event.Symbol] = price
	s.lastMarkBits.Store(math.Float64bits(price))
	marks := make(map[string]float64, len(s.marks))
	for k, v := range s.marks {
		marks[k] = v
	}
	s.mu.Unlock()
	s.intelligence.ObserveMarket(event, marks)
}

func (s *Service) Evaluate(req domain.ExecutionRequest) RiskDecision {
	if s.kill.Load() {
		return s.observeDecision(RiskDecision{Allowed: false, RiskScore: 1, Reason: "kill_switch", Action: "deny", ReasonCategory: FailureRiskReject, RegimeState: s.feedback.Snapshot().RegimeState, Confidence: 1})
	}
	if s.forcedBreaker.Load() {
		return s.observeDecision(RiskDecision{Allowed: false, RiskScore: s.intelligence.Snapshot().SystemStressIndex, Reason: "symbol_circuit_breaker", Action: "deny", ReasonCategory: FailureRiskReject, RegimeState: s.feedback.Snapshot().RegimeState, Confidence: 1})
	}
	if req.Decision != domain.DecisionExecute {
		return s.observeDecision(RiskDecision{Allowed: false, RiskScore: 0, Reason: "not_execute_decision", Action: "deny", ReasonCategory: FailureRiskReject, RegimeState: s.feedback.Snapshot().RegimeState, Confidence: 1})
	}
	if time.Since(req.SignalTime) > s.cfg.MaxSignalAge {
		return s.observeDecision(RiskDecision{Allowed: false, RiskScore: s.intelligence.Snapshot().SystemStressIndex, Reason: "stale_signal", Action: "deny", ReasonCategory: FailureStaleSkip, RegimeState: s.feedback.Snapshot().RegimeState, Confidence: 1})
	}
	if req.Size <= 0 {
		return s.observeDecision(RiskDecision{Allowed: false, RiskScore: 0, Reason: "bad_size", Action: "deny", ReasonCategory: FailureRiskReject, RegimeState: s.feedback.Snapshot().RegimeState, Confidence: 1})
	}
	if math.Float64frombits(s.dailyPnLBits.Load()) <= -s.cfg.MaxDailyLossUSD {
		return s.observeDecision(RiskDecision{Allowed: false, RiskScore: 1, Reason: "daily_loss_limit", Action: "deny", ReasonCategory: FailureRiskReject, RegimeState: s.feedback.Snapshot().RegimeState, Confidence: 1})
	}
	price := math.Float64frombits(s.lastMarkBits.Load())
	if req.Price != nil {
		price = *req.Price
	}
	if price <= 0 {
		return s.observeDecision(RiskDecision{Allowed: false, RiskScore: 0.5, Reason: "missing_mark_price", Action: "deny", ReasonCategory: FailureStaleSkip, RegimeState: s.feedback.Snapshot().RegimeState, Confidence: 0.8})
	}
	orderNotional := req.Size * price
	account := s.store.AccountState()
	if account.AvailableBalance > 0 && !req.ReduceOnly && orderNotional > account.AvailableBalance*account.LeverageOrDefault() {
		return s.observeDecision(RiskDecision{Allowed: false, RiskScore: 1, Reason: "insufficient_available_balance", Action: "deny", ReasonCategory: FailureRiskReject, RegimeState: s.feedback.Snapshot().RegimeState, Confidence: 1})
	}
	dynamicPositionLimit := s.cfg.MaxPositionUSD
	if s.intelligence.Snapshot().SystemStressIndex > 0.55 {
		dynamicPositionLimit *= 0.65
	}
	if !req.ReduceOnly && orderNotional > s.cfg.MaxPositionUSD {
		return s.observeDecision(RiskDecision{Allowed: false, RiskScore: s.intelligence.Snapshot().ExposureRiskScore, Reason: "position_limit", Action: "deny", ReasonCategory: FailureRiskReject, RegimeState: s.feedback.Snapshot().RegimeState, Confidence: 1})
	}
	if !req.ReduceOnly && orderNotional > dynamicPositionLimit {
		return s.observeDecision(RiskDecision{Allowed: false, RiskScore: s.intelligence.Snapshot().ExposureRiskScore, Reason: "dynamic_position_limit", Action: "deny", ReasonCategory: FailureRiskReject, RegimeState: s.feedback.Snapshot().RegimeState, Confidence: 1})
	}
	if req.Price == nil {
		req.Price = &price
	}
	base := RiskDecision{Allowed: true, SizeMultiplier: 1, RiskScore: s.intelligence.Snapshot().SystemStressIndex, Reason: "allowed", Action: "allow", RegimeState: s.feedback.Snapshot().RegimeState, Confidence: 1}
	return s.observeDecision(s.intelligence.Evaluate(req, base))
}

func (s *Service) UpdateAccountState(account domain.AccountState) {
	s.store.SetAccountState(account)
}

func (s *Service) Validate(req domain.ExecutionRequest) error {
	decision := s.Evaluate(req)
	if !decision.Allowed {
		return wrap(decision.Reason)
	}
	return nil
}

func (s *Service) ObserveExecution(latency time.Duration, rejected bool) {
	s.intelligence.ObserveExecution(latency, rejected)
	if rejected {
		s.feedback.ObserveFailure(FailureExecutionFail)
	}
}

func (s *Service) ObserveExecutionEvent(event domain.ExecutionEvent) {
	s.feedback.ObserveExecutionEvent(event)
	s.intelligence.recompute(nil, s.intelligence.Snapshot().MempoolPressureScore)
}

func (s *Service) ObserveFailure(category FailureCategory) {
	s.feedback.ObserveFailure(category)
	s.intelligence.recompute(nil, s.intelligence.Snapshot().MempoolPressureScore)
}

func (s *Service) observeDecision(decision RiskDecision) RiskDecision {
	if !decision.Allowed {
		category := decision.ReasonCategory
		if category == "" {
			category = FailureRiskReject
		}
		s.feedback.ObserveFailure(category)
	}
	return decision
}

func (s *Service) SetKillSwitch(enabled bool) {
	s.kill.Store(enabled)
}

func (s *Service) SetCircuitBreaker(symbol string, enabled bool) {
	s.mu.Lock()
	s.breakers[symbol] = enabled
	any := false
	for _, active := range s.breakers {
		if active {
			any = true
			break
		}
	}
	s.mu.Unlock()
	s.forcedBreaker.Store(any)
}

func (s *Service) Status() domain.RiskStatus {
	s.mu.RLock()
	breakers := make(map[string]bool, len(s.breakers))
	for k, v := range s.breakers {
		breakers[k] = v
	}
	s.mu.RUnlock()
	snap := s.intelligence.Snapshot()
	feedback := s.feedback.Snapshot()
	return domain.RiskStatus{
		KillSwitch:               s.kill.Load(),
		DailyPnL:                 math.Float64frombits(s.dailyPnLBits.Load()),
		MaxDailyLossUSD:          s.cfg.MaxDailyLossUSD,
		CircuitBreakers:          breakers,
		SystemStressIndex:        snap.SystemStressIndex,
		MempoolPressure:          snap.MempoolPressureScore,
		ExecutionFragility:       snap.ExecutionFragilityScore,
		ExposureRisk:             snap.ExposureRiskScore,
		ActiveCircuitState:       snap.CircuitState.String(),
		CurrentRiskMultiplier:    snap.RiskMultiplier,
		RejectionCountLastWindow: snap.RejectionCount,
		RegimeState:              string(feedback.RegimeState),
		RealEvents:               feedback.RealEvents,
		ContaminatedEvents:       feedback.ContaminatedEvents,
		FailureBreakdown: map[string]uint64{
			string(FailureRiskReject):      feedback.RiskRejects,
			string(FailureExecutionFail):   feedback.ExecutionFails,
			string(FailureOutbidFail):      feedback.OutbidFails,
			string(FailureStaleSkip):       feedback.StaleSkips,
			string(FailureMarketCancel):    feedback.MarketCancels,
			string(FailurePartialFillLoss): feedback.PartialFillLosses,
		},
		RealizedPnLEMA:          feedback.RealizedPnLEMA,
		MarkoutDegradationScore: feedback.MarkoutDegradationScore,
		AdverseSelectionEMA:     feedback.AdverseSelectionEMA,
		SlippageVarianceEMA:     feedback.SlippageVarianceEMA,
		ExecutionFailureRateEMA: feedback.ExecutionFailureRateEMA,
	}
}

func wrap(reason string) error {
	return errors.Join(ErrRiskRejected, errors.New(reason))
}
