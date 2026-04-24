package state

import (
	"testing"
	"time"

	"control-plane/internal/domain"
)

func TestStoreReconciliationMatchesLedgerAndExecution(t *testing.T) {
	store := NewStore()
	price := 100.0
	order, _, err := store.ReserveOrder(domain.Order{
		ID:             "ord-1",
		IdempotencyKey: "idem-1",
		Symbol:         "BTCUSDT",
		Side:           domain.SideBuy,
		Size:           2,
		Price:          &price,
		RequestAt:      time.Now().UTC(),
	})
	if err != nil {
		t.Fatalf("reserve failed: %v", err)
	}
	if _, err := store.MarkOrderSent(order.ID, time.Now().UTC()); err != nil {
		t.Fatalf("mark sent failed: %v", err)
	}
	if _, err := store.MarkOrderAck(order.ID, time.Now().UTC()); err != nil {
		t.Fatalf("mark ack failed: %v", err)
	}
	fill := domain.FillLedgerEntry{
		OrderID:        order.ID,
		FillID:         "fill-1",
		Symbol:         order.Symbol,
		Side:           order.Side,
		Price:          100,
		Quantity:       2,
		LiquidityFlag:  domain.LiquidityTaker,
		FeeAmount:      0.08,
		FeeAsset:       "USDT",
		EventTime:      time.Now().UTC(),
		EventTimeUnixMs: time.Now().UTC().UnixMilli(),
	}
	event := domain.ExecutionEvent{
		OrderID:        order.ID,
		FillID:         "fill-1",
		Symbol:         order.Symbol,
		Status:         domain.OrderFilled,
		FilledQuantity: 2,
		FillPrice:      100,
		LiquidityFlag:  domain.LiquidityTaker,
		ExecutionLatency: 5 * time.Millisecond,
		Simulated:      false,
		PartialFillRatio: 1,
	}
	if _, _, err := store.ApplyExternalFill(order.ID, fill, event); err != nil {
		t.Fatalf("apply fill failed: %v", err)
	}
	recon := store.Reconciliation()
	if !recon.Matched {
		t.Fatalf("expected reconciliation match, got %#v", recon)
	}
	if recon.LedgerQtyByOrder[order.ID] != recon.ExchangeFillQtyByOrder[order.ID] {
		t.Fatalf("ledger quantity and execution quantity diverged")
	}
}
