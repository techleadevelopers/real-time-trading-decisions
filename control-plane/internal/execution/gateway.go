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
	SendOrder(context.Context, domain.Order) ([]ExchangeStep, error)
}

type AsyncExchangeClient interface {
	ExchangeClient
	Start(context.Context, func(AsyncExchangeUpdate)) error
	Reconcile(context.Context, *state.Store) (domain.ReconciliationReport, domain.AccountState, error)
	GetAccountState(context.Context) (domain.AccountState, error)
	CancelRemote(context.Context, string) error
	ReplaceRemote(context.Context, string, float64) error
}

type AsyncExchangeUpdate struct {
	OrderID        string
	ExchangeOrderID string
	IdempotencyKey string
	Step           ExchangeStep
}

type ExchangeStep struct {
	Status       domain.OrderStatus
	OccurredAt   time.Time
	CancelReason string
	ExchangeOrderID string
	Ledger       *domain.FillLedgerEntry
	Execution    *domain.ExecutionEvent
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
	for _, open := range g.store.OpenOrdersBySymbol(req.Symbol) {
		if open.Side != req.Side && open.Status != domain.OrderFilled && open.Status != domain.OrderCanceled && open.Status != domain.OrderRejected {
			if asyncExchange, ok := g.exchange.(AsyncExchangeClient); ok && open.ExchangeOrderID != "" {
				_ = asyncExchange.CancelRemote(ctx, open.ExchangeOrderID)
			}
			_, _ = g.store.CancelOrder(open.ID, "self_trade_prevention", time.Now().UTC())
		}
	}
	order := domain.Order{ID: newID(), IdempotencyKey: req.IdempotencyKey, Symbol: req.Symbol, Side: req.Side, Size: req.Size, Price: req.Price, RequestAt: requestAt, SendAt: time.Now().UTC()}
	order.ExpectedRealizedMarkout = req.ExpectedRealizedMarkout
	order.RegimeKind = req.RegimeKind
	order.RegimeVolatility = req.RegimeVolatility
	order.RegimeSpread = req.RegimeSpread
	order.RegimeTrendStrength = req.RegimeTrendStrength
	reserved, duplicate, err := g.store.ReserveOrder(order)
	if duplicate {
		return reserved, nil
	}
	if err != nil && !errors.Is(err, state.ErrDuplicate) {
		return domain.Order{}, err
	}
	sentOrder, err := g.store.MarkOrderSent(reserved.ID, time.Now().UTC())
	if err == nil {
		g.emit("order_update", sentOrder)
		reserved = sentOrder
	}
	steps, err := g.exchange.SendOrder(ctx, reserved)
	if err != nil {
		g.risk.ObserveExecution(time.Since(started), true)
		reserved.Status = domain.OrderRejected
		reserved.RejectReason = err.Error()
		g.store.UpdateOrder(reserved)
		g.emit("order_rejected", reserved)
		return reserved, err
	}
	g.risk.ObserveExecution(time.Since(started), false)
	finalOrder := reserved
	for _, step := range steps {
		updated, stepErr := g.applyStep(AsyncExchangeUpdate{
			OrderID:         reserved.ID,
			ExchangeOrderID: step.ExchangeOrderID,
			IdempotencyKey:  reserved.IdempotencyKey,
			Step:            step,
		})
		if stepErr != nil {
			return finalOrder, stepErr
		}
		finalOrder = updated
	}
	return finalOrder, nil
}

func (g *Gateway) ApplyAsyncUpdate(update AsyncExchangeUpdate) error {
	_, err := g.applyStep(update)
	return err
}

func (g *Gateway) Cancel(ctx context.Context, orderID, reason string) (domain.Order, error) {
	select {
	case <-ctx.Done():
		return domain.Order{}, ctx.Err()
	default:
	}
	order, ok := g.store.Order(orderID)
	if !ok {
		return domain.Order{}, errors.New("order not found")
	}
	if order.Status == domain.OrderFilled || order.Status == domain.OrderCanceled || order.Status == domain.OrderRejected {
		return order, nil
	}
	if asyncExchange, ok := g.exchange.(AsyncExchangeClient); ok && order.ExchangeOrderID != "" {
		if err := asyncExchange.CancelRemote(ctx, order.ExchangeOrderID); err != nil {
			return domain.Order{}, err
		}
	}
	updated, err := g.store.CancelOrder(orderID, reason, time.Now().UTC())
	if err != nil {
		return domain.Order{}, err
	}
	g.emit("execution_update", domain.ExecutionUpdate{
		Order:                updated,
		RequestTimestampMs:   updated.RequestAt.UnixMilli(),
		SendTimestampMs:      updated.SendAt.UnixMilli(),
		AckTimestampMs:       updated.AckAt.UnixMilli(),
		FirstFillTimestampMs: updated.FirstFillAt.UnixMilli(),
		LastFillTimestampMs:  updated.LastFillAt.UnixMilli(),
	})
	g.emit("order_update", updated)
	return updated, nil
}

func (g *Gateway) Replace(ctx context.Context, orderID string, newPrice float64, _ string) (domain.Order, error) {
	select {
	case <-ctx.Done():
		return domain.Order{}, ctx.Err()
	default:
	}
	updated, err := g.store.ReplaceOrderPrice(orderID, &newPrice)
	if err != nil {
		return domain.Order{}, err
	}
	if asyncExchange, ok := g.exchange.(AsyncExchangeClient); ok && updated.ExchangeOrderID != "" {
		if err := asyncExchange.ReplaceRemote(ctx, updated.ExchangeOrderID, newPrice); err != nil {
			return domain.Order{}, err
		}
	}
	g.emit("execution_update", domain.ExecutionUpdate{
		Order:                updated,
		RequestTimestampMs:   updated.RequestAt.UnixMilli(),
		SendTimestampMs:      updated.SendAt.UnixMilli(),
		AckTimestampMs:       updated.AckAt.UnixMilli(),
		FirstFillTimestampMs: updated.FirstFillAt.UnixMilli(),
		LastFillTimestampMs:  updated.LastFillAt.UnixMilli(),
	})
	g.emit("order_update", updated)
	return updated, nil
}

func (g *Gateway) SwitchStrategy(ctx context.Context, orderID string, strategy string, price *float64, reason string) (domain.Order, error) {
	select {
	case <-ctx.Done():
		return domain.Order{}, ctx.Err()
	default:
	}
	switch strategy {
	case "AGGRESSIVE", "DEFENSIVE":
		if price == nil {
			return g.Cancel(ctx, orderID, reason)
		}
		return g.Replace(ctx, orderID, *price, reason)
	case "PASSIVE":
		if price == nil {
			order, ok := g.store.Order(orderID)
			if !ok {
				return domain.Order{}, errors.New("order not found")
			}
			return order, nil
		}
		return g.Replace(ctx, orderID, *price, reason)
	default:
		order, ok := g.store.Order(orderID)
		if !ok {
			return domain.Order{}, errors.New("order not found")
		}
		return order, nil
	}
}

func (g *Gateway) emit(kind string, value any) {
	select {
	case g.updates <- api.Update{Type: kind, Time: time.Now().UTC(), Data: value}:
	default:
	}
}

func (g *Gateway) applyStep(update AsyncExchangeUpdate) (domain.Order, error) {
	orderID := update.OrderID
	if orderID == "" && update.IdempotencyKey != "" {
		if existing, ok := g.store.OrderByIdempotency(update.IdempotencyKey); ok {
			orderID = existing.ID
		}
	}
	if orderID == "" && update.ExchangeOrderID != "" {
		if existing, ok := g.store.OrderByExchangeID(update.ExchangeOrderID); ok {
			orderID = existing.ID
		}
	}
	if orderID == "" {
		return domain.Order{}, errors.New("missing order reference")
	}
	if update.ExchangeOrderID != "" {
		if _, err := g.store.BindExchangeOrderID(orderID, update.ExchangeOrderID); err != nil {
			return domain.Order{}, err
		}
	}
	switch update.Step.Status {
	case domain.OrderAck:
		if updated, ackErr := g.store.MarkOrderAck(orderID, update.Step.OccurredAt); ackErr == nil {
			g.emit("execution_update", executionUpdate(updated, nil, nil, nil))
			g.emit("order_update", updated)
			return updated, nil
		}
	case domain.OrderPartial, domain.OrderFilled:
		if update.Step.Ledger == nil || update.Step.Execution == nil {
			return domain.Order{}, nil
		}
		updated, position, applyErr := g.store.ApplyExternalFill(orderID, *update.Step.Ledger, *update.Step.Execution)
		if applyErr != nil {
			return domain.Order{}, applyErr
		}
		g.risk.ObserveExecutionEvent(*update.Step.Execution)
		g.emit("fill_ledger_entry", update.Step.Ledger)
		g.emit("execution_event", update.Step.Execution)
		g.emit("execution_update", executionUpdate(updated, update.Step.Ledger, update.Step.Execution, &position))
		g.emit("position_update", position)
		g.emit("order_update", updated)
		return updated, nil
	case domain.OrderCanceled:
		if updated, cancelErr := g.store.CancelOrder(orderID, update.Step.CancelReason, update.Step.OccurredAt); cancelErr == nil {
			g.emit("execution_update", executionUpdate(updated, nil, nil, nil))
			g.emit("order_update", updated)
			return updated, nil
		}
	case domain.OrderRejected:
		order, ok := g.store.Order(orderID)
		if !ok {
			return domain.Order{}, errors.New("order not found")
		}
		order.Status = domain.OrderRejected
		order.RejectReason = update.Step.CancelReason
		g.store.UpdateOrder(order)
		g.emit("order_rejected", order)
		return order, nil
	}
	order, ok := g.store.Order(orderID)
	if !ok {
		return domain.Order{}, errors.New("order not found")
	}
	return order, nil
}

func executionUpdate(order domain.Order, ledger *domain.FillLedgerEntry, executionEvent *domain.ExecutionEvent, position *domain.Position) domain.ExecutionUpdate {
	return domain.ExecutionUpdate{
		Order:                   order,
		Ledger:                  ledger,
		Execution:               executionEvent,
		Position:                position,
		RequestTimestampMs:      order.RequestAt.UnixMilli(),
		SendTimestampMs:         order.SendAt.UnixMilli(),
		AckTimestampMs:          order.AckAt.UnixMilli(),
		FirstFillTimestampMs:    order.FirstFillAt.UnixMilli(),
		LastFillTimestampMs:     order.LastFillAt.UnixMilli(),
		ExpectedRealizedMarkout: order.ExpectedRealizedMarkout,
		RegimeKind:              order.RegimeKind,
		RegimeVolatility:        order.RegimeVolatility,
		RegimeSpread:            order.RegimeSpread,
		RegimeTrendStrength:     order.RegimeTrendStrength,
	}
}

func newID() string {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return time.Now().UTC().Format("20060102150405.000000000")
	}
	return hex.EncodeToString(b[:])
}
