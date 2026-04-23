package execution

import (
	"context"
	"time"

	"control-plane/internal/api"
	"control-plane/internal/domain"
	"control-plane/internal/state"
)

type PaperExchange struct {
	store   *state.Store
	updates chan<- api.Update
}

func NewPaperExchange(store *state.Store, updates chan<- api.Update) *PaperExchange {
	return &PaperExchange{store: store, updates: updates}
}

func (p *PaperExchange) SendOrder(ctx context.Context, order domain.Order) (domain.Order, error) {
	select {
	case <-ctx.Done():
		return order, ctx.Err()
	default:
	}
	now := time.Now().UTC()
	if order.SendAt.IsZero() {
		order.SendAt = now
	}
	order.AckAt = now
	order.ExchangeAcceptAt = now
	order.Status = domain.OrderSent
	p.store.UpdateOrder(order)
	fillPrice := 0.0
	if order.Price != nil {
		fillPrice = *order.Price
	}
	if fillPrice <= 0 {
		fillPrice = 1
	}
	position := p.store.ApplyFill(order, order.Size, fillPrice)
	order.Filled = order.Size
	order.PartialFillRatio = 1
	order.WeightedAvgFillPrice = fillPrice
	order.FirstFillAt = time.Now().UTC()
	order.LastFillAt = order.FirstFillAt
	order.SlippageRealBps = slippageBps(order, fillPrice)
	order.QueueDelay = order.LastFillAt.Sub(order.SendAt)
	order.Status = domain.OrderFilled
	order.UpdatedAt = time.Now().UTC()
	p.store.UpdateOrder(order)
	p.emit("position_update", position)
	p.emit("execution_event", domain.ExecutionEvent{
		OrderID:          order.ID,
		Symbol:           order.Symbol,
		FillQuality:      1,
		SlippageReal:     order.SlippageRealBps,
		MarkoutCurve:     domain.MarkoutCurve{},
		ExecutionLatency: order.QueueDelay,
		CompetitionFlag:  "STRUCTURAL_TEST_ONLY",
		Simulated:        true,
	})
	return order, nil
}

func (p *PaperExchange) emit(kind string, value any) {
	select {
	case p.updates <- api.Update{Type: kind, Time: time.Now().UTC(), Data: value}:
	default:
	}
}

func slippageBps(order domain.Order, fillPrice float64) float64 {
	if order.Price == nil || *order.Price <= 0 {
		return 0
	}
	side := 1.0
	if order.Side == domain.SideSell {
		side = -1
	}
	return ((fillPrice - *order.Price) / *order.Price) * 10_000 * side
}
