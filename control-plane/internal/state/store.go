package state

import (
	"errors"
	"sync"
	"time"

	"control-plane/internal/domain"
)

var ErrDuplicate = errors.New("duplicate idempotency key")

type Store struct {
	mu          sync.RWMutex
	positions   map[string]domain.Position
	orders      map[string]domain.Order
	idempotency map[string]string
	ledger      []domain.FillLedgerEntry
	executions  []domain.ExecutionEvent
}

func NewStore() *Store {
	return &Store{
		positions:   make(map[string]domain.Position),
		orders:      make(map[string]domain.Order),
		idempotency: make(map[string]string),
	}
}

func (s *Store) ReserveOrder(order domain.Order) (domain.Order, bool, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if existingID, ok := s.idempotency[order.IdempotencyKey]; ok {
		return s.orders[existingID], true, ErrDuplicate
	}
	now := time.Now().UTC()
	order.CreatedAt = now
	order.UpdatedAt = now
	if order.RequestAt.IsZero() {
		order.RequestAt = now
	}
	order.Status = domain.OrderNew
	s.orders[order.ID] = order
	s.idempotency[order.IdempotencyKey] = order.ID
	return order, false, nil
}

func (s *Store) UpdateOrder(order domain.Order) {
	s.mu.Lock()
	defer s.mu.Unlock()
	order.UpdatedAt = time.Now().UTC()
	s.orders[order.ID] = order
}

func (s *Store) MarkOrderSent(orderID string, sentAt time.Time) (domain.Order, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	order, ok := s.orders[orderID]
	if !ok {
		return domain.Order{}, errors.New("order not found")
	}
	order.Status = domain.OrderSent
	order.SendAt = sentAt
	order.UpdatedAt = time.Now().UTC()
	s.orders[order.ID] = order
	return order, nil
}

func (s *Store) MarkOrderAck(orderID string, ackAt time.Time) (domain.Order, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	order, ok := s.orders[orderID]
	if !ok {
		return domain.Order{}, errors.New("order not found")
	}
	order.Status = domain.OrderAck
	order.AckAt = ackAt
	order.ExchangeAcceptAt = ackAt
	order.UpdatedAt = time.Now().UTC()
	s.orders[order.ID] = order
	return order, nil
}

func (s *Store) CancelOrder(orderID, reason string, canceledAt time.Time) (domain.Order, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	order, ok := s.orders[orderID]
	if !ok {
		return domain.Order{}, errors.New("order not found")
	}
	order.Status = domain.OrderCanceled
	order.CancelReason = reason
	order.LastFillAt = canceledAt
	order.UpdatedAt = time.Now().UTC()
	s.orders[order.ID] = order
	return order, nil
}

func (s *Store) ApplyExternalFill(orderID string, fill domain.FillLedgerEntry, event domain.ExecutionEvent) (domain.Order, domain.Position, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	order, ok := s.orders[orderID]
	if !ok {
		return domain.Order{}, domain.Position{}, errors.New("order not found")
	}
	now := time.Now().UTC()
	fillQty := fill.Quantity
	fillPrice := fill.Price
	previousFilled := order.Filled
	order.Filled += fillQty
	if order.Filled > 0 {
		order.WeightedAvgFillPrice = ((order.WeightedAvgFillPrice * previousFilled) + (fillPrice * fillQty)) / order.Filled
	}
	order.PartialFillRatio = order.Filled / max(order.Size, 1e-12)
	if order.FirstFillAt.IsZero() && fillQty > 0 {
		order.FirstFillAt = fill.EventTime
	}
	if fillQty > 0 {
		order.LastFillAt = fill.EventTime
	}
	if order.Filled >= order.Size {
		order.Status = domain.OrderFilled
	} else {
		order.Status = domain.OrderPartial
	}
	order.SlippageRealBps = event.SlippageReal
	order.QueueDelay = event.ExecutionLatency
	order.UpdatedAt = now
	s.orders[order.ID] = order

	pos := s.positions[order.Symbol]
	signed := fillQty
	if order.Side == domain.SideSell {
		signed = -fillQty
	}
	newSize := pos.Size + signed
	oldNotional := pos.AvgPrice * abs(pos.Size)
	fillNotional := fillPrice * fillQty
	if abs(newSize) < 1e-12 {
		pos = domain.Position{Symbol: order.Symbol, Updated: now}
	} else if pos.Size == 0 || sign(pos.Size) == sign(signed) {
		pos.AvgPrice = (oldNotional + fillNotional) / abs(newSize)
		pos.Size = newSize
		pos.Symbol = order.Symbol
		pos.Updated = now
	} else {
		pos.Size = newSize
		pos.Updated = now
	}
	s.positions[order.Symbol] = pos

	s.ledger = append(s.ledger, fill)
	s.executions = append(s.executions, event)
	return order, pos, nil
}

func (s *Store) Order(orderID string) (domain.Order, bool) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	order, ok := s.orders[orderID]
	return order, ok
}

func (s *Store) Ledger() []domain.FillLedgerEntry {
	s.mu.RLock()
	defer s.mu.RUnlock()
	out := make([]domain.FillLedgerEntry, len(s.ledger))
	copy(out, s.ledger)
	return out
}

func (s *Store) ExecutionEvents() []domain.ExecutionEvent {
	s.mu.RLock()
	defer s.mu.RUnlock()
	out := make([]domain.ExecutionEvent, len(s.executions))
	copy(out, s.executions)
	return out
}

func (s *Store) Reconciliation() domain.ReconciliationStatus {
	s.mu.RLock()
	defer s.mu.RUnlock()
	ledgerByOrder := make(map[string]float64, len(s.orders))
	exchangeByOrder := make(map[string]float64, len(s.orders))
	for _, entry := range s.ledger {
		ledgerByOrder[entry.OrderID] += entry.Quantity
	}
	for _, event := range s.executions {
		exchangeByOrder[event.OrderID] += event.FilledQuantity
	}
	matched := len(ledgerByOrder) == len(exchangeByOrder)
	if matched {
		for orderID, qty := range exchangeByOrder {
			if abs(qty-ledgerByOrder[orderID]) > 1e-9 {
				matched = false
				break
			}
		}
	}
	return domain.ReconciliationStatus{
		OrdersTracked:          len(s.orders),
		LedgerEntries:          len(s.ledger),
		ExecutionEvents:        len(s.executions),
		ExchangeFillQtyByOrder: exchangeByOrder,
		LedgerQtyByOrder:       ledgerByOrder,
		Matched:                matched,
	}
}

func (s *Store) Positions() []domain.Position {
	s.mu.RLock()
	defer s.mu.RUnlock()
	out := make([]domain.Position, 0, len(s.positions))
	for _, position := range s.positions {
		out = append(out, position)
	}
	return out
}

func (s *Store) Position(symbol string) domain.Position {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return s.positions[symbol]
}

func (s *Store) Orders() []domain.Order {
	s.mu.RLock()
	defer s.mu.RUnlock()
	out := make([]domain.Order, 0, len(s.orders))
	for _, order := range s.orders {
		out = append(out, order)
	}
	return out
}

func (s *Store) GrossExposureUSD(mark map[string]float64) float64 {
	s.mu.RLock()
	defer s.mu.RUnlock()
	var total float64
	for symbol, position := range s.positions {
		price := mark[symbol]
		if price == 0 {
			price = position.AvgPrice
		}
		total += abs(position.Size * price)
	}
	return total
}

func abs(v float64) float64 {
	if v < 0 {
		return -v
	}
	return v
}

func sign(v float64) float64 {
	if v < 0 {
		return -1
	}
	if v > 0 {
		return 1
	}
	return 0
}

func max(a, b float64) float64 {
	if a > b {
		return a
	}
	return b
}
