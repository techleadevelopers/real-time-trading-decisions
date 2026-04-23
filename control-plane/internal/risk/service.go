package risk

import (
	"errors"
	"sync"
	"time"

	"control-plane/internal/config"
	"control-plane/internal/domain"
	"control-plane/internal/state"
)

var ErrRiskRejected = errors.New("risk rejected")

type Service struct {
	mu       sync.RWMutex
	cfg      config.RiskConfig
	store    *state.Store
	kill     bool
	breakers map[string]bool
	dailyPnL float64
	marks    map[string]float64
}

func NewService(cfg config.RiskConfig, store *state.Store) *Service {
	return &Service{cfg: cfg, store: store, breakers: make(map[string]bool), marks: make(map[string]float64)}
}

func (s *Service) ObserveMarket(event domain.MarketEvent) {
	if event.Price <= 0 {
		return
	}
	s.mu.Lock()
	s.marks[event.Symbol] = event.Price
	s.mu.Unlock()
}

func (s *Service) Validate(req domain.ExecutionRequest) error {
	s.mu.RLock()
	kill := s.kill
	breaker := s.breakers[req.Symbol]
	dailyPnL := s.dailyPnL
	marks := make(map[string]float64, len(s.marks))
	for k, v := range s.marks {
		marks[k] = v
	}
	s.mu.RUnlock()

	if kill {
		return wrap("kill_switch")
	}
	if breaker {
		return wrap("symbol_circuit_breaker")
	}
	if req.Decision != domain.DecisionExecute {
		return wrap("not_execute_decision")
	}
	if time.Since(req.SignalTime) > s.cfg.MaxSignalAge {
		return wrap("stale_signal")
	}
	if req.Size <= 0 {
		return wrap("bad_size")
	}
	if dailyPnL <= -s.cfg.MaxDailyLossUSD {
		return wrap("daily_loss_limit")
	}
	price := 0.0
	if req.Price != nil {
		price = *req.Price
	} else {
		price = marks[req.Symbol]
	}
	if price <= 0 {
		return wrap("missing_mark_price")
	}
	orderNotional := req.Size * price
	if !req.ReduceOnly && orderNotional > s.cfg.MaxPositionUSD {
		return wrap("position_limit")
	}
	if !req.ReduceOnly && s.store.GrossExposureUSD(marks)+orderNotional > s.cfg.MaxExposureUSD {
		return wrap("max_exposure")
	}
	return nil
}

func (s *Service) SetKillSwitch(enabled bool) {
	s.mu.Lock()
	s.kill = enabled
	s.mu.Unlock()
}

func (s *Service) SetCircuitBreaker(symbol string, enabled bool) {
	s.mu.Lock()
	s.breakers[symbol] = enabled
	s.mu.Unlock()
}

func (s *Service) Status() domain.RiskStatus {
	s.mu.RLock()
	defer s.mu.RUnlock()
	breakers := make(map[string]bool, len(s.breakers))
	for k, v := range s.breakers {
		breakers[k] = v
	}
	return domain.RiskStatus{KillSwitch: s.kill, DailyPnL: s.dailyPnL, MaxDailyLossUSD: s.cfg.MaxDailyLossUSD, CircuitBreakers: breakers}
}

func wrap(reason string) error {
	return errors.Join(ErrRiskRejected, errors.New(reason))
}
