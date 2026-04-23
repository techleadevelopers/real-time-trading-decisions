package execution

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"time"

	"control-plane/internal/api"
	"control-plane/internal/config"
	"control-plane/internal/domain"
	"control-plane/internal/risk"
	"control-plane/internal/state"
)

type ExchangeClient interface {
	SendOrder(context.Context, domain.Order) (domain.Order, error)
}

type Gateway struct {
	store    *state.Store
	risk     *risk.Service
	exchange ExchangeClient
	updates  chan<- api.Update
	cfg      config.ExecutionConfig
}

func NewGateway(store *state.Store, riskSvc *risk.Service, exchange ExchangeClient, updates chan<- api.Update, cfg config.ExecutionConfig) *Gateway {
	return &Gateway{store: store, risk: riskSvc, exchange: exchange, updates: updates, cfg: cfg}
}

func (g *Gateway) Submit(ctx context.Context, req domain.ExecutionRequest) (domain.Order, error) {
	started := time.Now()
	requestAt := req.RequestTime
	if requestAt.IsZero() {
		requestAt = started.UTC()
	}
	if req.IdempotencyKey == "" {
		return domain.Order{}, errors.New("missing idempotency_key")
	}
	decision := g.risk.Evaluate(req)
	if !decision.Allowed {
		order := domain.Order{ID: newID(), IdempotencyKey: req.IdempotencyKey, Symbol: req.Symbol, Side: req.Side, Size: req.Size, Price: req.Price, Status: domain.OrderRejected, RejectReason: decision.Reason}
		g.emit("order_rejected", order)
		return order, errors.Join(risk.ErrRiskRejected, errors.New(decision.Reason))
	}
	if decision.SizeMultiplier > 0 && decision.SizeMultiplier < 1 && !req.ReduceOnly {
		req.Size *= decision.SizeMultiplier
	}
	order := domain.Order{ID: newID(), IdempotencyKey: req.IdempotencyKey, Symbol: req.Symbol, Side: req.Side, Size: req.Size, Price: req.Price, RequestAt: requestAt, SendAt: time.Now().UTC()}
	reserved, duplicate, err := g.store.ReserveOrder(order)
	if duplicate {
		return reserved, nil
	}
	if err != nil && !errors.Is(err, state.ErrDuplicate) {
		return domain.Order{}, err
	}
	sent, err := g.exchange.SendOrder(ctx, reserved)
	if err != nil {
		g.risk.ObserveExecution(time.Since(started), true)
		reserved.Status = domain.OrderRejected
		reserved.RejectReason = err.Error()
		g.store.UpdateOrder(reserved)
		g.emit("order_rejected", reserved)
		return reserved, err
	}
	g.risk.ObserveExecution(time.Since(started), false)
	g.emit("order_update", sent)
	return sent, nil
}

func (g *Gateway) emit(kind string, value any) {
	select {
	case g.updates <- api.Update{Type: kind, Time: time.Now().UTC(), Data: value}:
	default:
	}
}

func newID() string {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return time.Now().UTC().Format("20060102150405.000000000")
	}
	return hex.EncodeToString(b[:])
}
