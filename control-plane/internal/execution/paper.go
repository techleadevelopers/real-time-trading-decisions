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
	order.Status = domain.OrderFilled
	order.UpdatedAt = time.Now().UTC()
	p.store.UpdateOrder(order)
	p.emit("position_update", position)
	return order, nil
}

func (p *PaperExchange) emit(kind string, value any) {
	select {
	case p.updates <- api.Update{Type: kind, Time: time.Now().UTC(), Data: value}:
	default:
	}
}
