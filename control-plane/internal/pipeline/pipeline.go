package pipeline

import (
	"context"
	"log/slog"
	"time"

	"control-plane/internal/api"
	"control-plane/internal/config"
	"control-plane/internal/domain"
	"control-plane/internal/risk"
	"control-plane/internal/state"
)

type Pipeline struct {
	in      <-chan domain.MarketEvent
	updates chan<- api.Update
	store   *state.Store
	risk    *risk.Service
	cfg     config.PipelineConfig
}

func New(in <-chan domain.MarketEvent, updates chan<- api.Update, store *state.Store, riskSvc *risk.Service, cfg config.PipelineConfig) *Pipeline {
	return &Pipeline{in: in, updates: updates, store: store, risk: riskSvc, cfg: cfg}
}

func (p *Pipeline) Run(ctx context.Context) {
	for {
		select {
		case <-ctx.Done():
			return
		case event := <-p.in:
			if time.Since(event.Timestamp) > p.cfg.StaleAfter {
				slog.Debug("dropping stale market event", "symbol", event.Symbol, "type", event.Type)
				continue
			}
			p.risk.ObserveMarket(event)
			p.offer(api.Update{Type: "market_event", Time: time.Now().UTC(), Data: event})
		}
	}
}

func (p *Pipeline) offer(update api.Update) {
	select {
	case p.updates <- update:
	default:
		slog.Warn("update stream dropped event under backpressure", "type", update.Type)
	}
}
