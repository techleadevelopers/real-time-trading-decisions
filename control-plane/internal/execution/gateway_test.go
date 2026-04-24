package execution

import (
	"context"
	"testing"
	"time"

	"control-plane/internal/api"
	"control-plane/internal/config"
	"control-plane/internal/domain"
	"control-plane/internal/risk"
	"control-plane/internal/state"
)

func TestGatewaySubmitProducesAuthoritativeExternalExecution(t *testing.T) {
	store := state.NewStore()
	updates := make(chan api.Update, 64)
	riskSvc := risk.NewService(config.RiskConfig{
		MaxExposureUSD:     10_000,
		MaxPositionUSD:     10_000,
		MaxDailyLossUSD:    1_000,
		MaxSignalAge:       time.Second,
		LatencyRejectAfter: 250 * time.Millisecond,
	}, store)
	gateway := NewGateway(
		store,
		riskSvc,
		NewPaperExchange(nil, updates),
		updates,
		config.ExecutionConfig{},
	)
	price := 100.0
	order, err := gateway.Submit(context.Background(), domain.ExecutionRequest{
		IdempotencyKey:          "test-order-1",
		Symbol:                  "BTCUSDT",
		Side:                    domain.SideBuy,
		Size:                    2,
		Price:                   &price,
		Decision:                domain.DecisionExecute,
		SignalTime:              time.Now().UTC(),
		MaxSlippageBps:          5,
		ExpectedRealizedMarkout: 1.5,
	})
	if err != nil {
		t.Fatalf("submit failed: %v", err)
	}
	if order.Status != domain.OrderFilled {
		t.Fatalf("expected final order status FILLED, got %s", order.Status)
	}
	events := store.ExecutionEvents()
	if len(events) != 2 {
		t.Fatalf("expected 2 execution events from authoritative exchange layer, got %d", len(events))
	}
	if events[0].Status != domain.OrderPartial {
		t.Fatalf("expected first execution event to be PARTIAL, got %s", events[0].Status)
	}
	if events[1].Status != domain.OrderFilled {
		t.Fatalf("expected final execution event to be FILLED, got %s", events[1].Status)
	}
	if events[0].Simulated || events[1].Simulated {
		t.Fatalf("external execution events must not be marked simulated")
	}
	recon := store.Reconciliation()
	if !recon.Matched {
		t.Fatalf("expected reconciliation to match, got %#v", recon)
	}
}
