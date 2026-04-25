package bingx

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"

	"control-plane/internal/api"
	"control-plane/internal/config"
	"control-plane/internal/domain"
	"control-plane/internal/execution"
	"control-plane/internal/risk"
	"control-plane/internal/state"
)

func TestSignatureCorrectness(t *testing.T) {
	params := map[string]string{
		"symbol":    "BTCUSDT",
		"side":      "BUY",
		"timestamp": "1710000000000",
	}
	got := sign("secret", params)
	want := "6be93bfa7882981379dac9c2d834f2077f3cbaa90a31b136b965335bc46ef377"
	if got != want {
		t.Fatalf("unexpected signature: got %s want %s", got, want)
	}
}

func TestIdempotentOrderSubmission(t *testing.T) {
	var hits atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/openApi/swap/v2/trade/order" {
			hits.Add(1)
			_ = json.NewEncoder(w).Encode(orderResponseEnvelope{
				Data: bingxOrderReply{
					OrderID:       "ex-1",
					ClientOrderID: "dup-1",
					Status:        "NEW",
					UpdateTime:    time.Now().UnixMilli(),
				},
			})
			return
		}
		http.NotFound(w, r)
	}))
	defer server.Close()

	store := state.NewStore()
	updates := make(chan api.Update, 64)
	riskSvc := risk.NewService(config.RiskConfig{
		MaxExposureUSD:     10_000,
		MaxPositionUSD:     10_000,
		MaxDailyLossUSD:    1_000,
		MaxSignalAge:       time.Second,
		LatencyRejectAfter: 250 * time.Millisecond,
	}, store)
	client := New("key", "secret", server.URL, "ws://example.invalid")
	gateway := execution.NewGateway(store, riskSvc, client, updates, config.ExecutionConfig{})
	price := 100.0
	req := domain.ExecutionRequest{
		IdempotencyKey: "dup-1",
		Symbol:         "BTCUSDT",
		Side:           domain.SideBuy,
		Size:           1,
		Price:          &price,
		Decision:       domain.DecisionExecute,
		SignalTime:     time.Now().UTC(),
		ExpectedRealizedMarkout: 5,
	}
	if _, err := gateway.Submit(context.Background(), req); err != nil {
		t.Fatalf("submit 1 failed: %v", err)
	}
	if _, err := gateway.Submit(context.Background(), req); err != nil {
		t.Fatalf("submit 2 failed: %v", err)
	}
	if hits.Load() != 1 {
		t.Fatalf("expected one remote submit, got %d", hits.Load())
	}
}

func TestLifecycleMappingCorrectness(t *testing.T) {
	cases := map[string]domain.OrderStatus{
		"NEW":               domain.OrderSent,
		"PARTIALLY_FILLED":  domain.OrderPartial,
		"FILLED":            domain.OrderFilled,
		"CANCELED":          domain.OrderCanceled,
		"FAILED":            domain.OrderRejected,
	}
	for input, want := range cases {
		if got := mapOrderStatus(input); got != want {
			t.Fatalf("status %s mapped to %s want %s", input, got, want)
		}
	}
}

func TestWSEventParsingToExecutionEvent(t *testing.T) {
	raw := []byte(`{
		"e":"executionReport",
		"i":"123",
		"c":"cid-1",
		"s":"BTCUSDT",
		"S":"BUY",
		"X":"PARTIALLY_FILLED",
		"p":"100.0",
		"q":"2",
		"L":"100.5",
		"l":"1",
		"z":"1",
		"n":"0.1",
		"N":"USDT",
		"m":true,
		"t":"fill-1",
		"E":1710000000000,
		"T":1710000000000
	}`)
	update, ok := parseExecutionReport(raw)
	if !ok {
		t.Fatalf("expected ws parse success")
	}
	if update.Step.Status != domain.OrderPartial {
		t.Fatalf("expected PARTIAL, got %s", update.Step.Status)
	}
	if update.Step.Execution == nil || update.Step.Ledger == nil {
		t.Fatalf("expected execution and ledger generated")
	}
}

func TestReconciliationDetectsMismatch(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/openApi/swap/v2/user/balance":
			_ = json.NewEncoder(w).Encode(balanceEnvelope{Data: bingxBalanceRaw{Balance: "1000", AvailableBalance: "900"}})
		case "/openApi/swap/v2/user/positions":
			_ = json.NewEncoder(w).Encode(positionsEnvelope{Data: []bingxPositionRaw{{Symbol: "BTCUSDT", PositionAmt: "1", AvgPrice: "100", UpdateTime: time.Now().UnixMilli()}}})
		case "/openApi/swap/v2/trade/openOrders":
			_ = json.NewEncoder(w).Encode(bingxOpenOrdersEnvelope{Data: []bingxOrderReply{{OrderID: "ex-1", ClientOrderID: "cid-1", Symbol: "BTCUSDT", Side: "BUY", Price: "100", OrigQty: "1", Status: "NEW", UpdateTime: time.Now().UnixMilli(), Time: time.Now().UnixMilli()}}})
		case "/openApi/swap/v2/trade/allFillOrders":
			_ = json.NewEncoder(w).Encode(fillsEnvelope{Data: []bingxFillRaw{{OrderID: "ex-1", TradeID: "fill-1", Symbol: "BTCUSDT", Side: "BUY", Price: "100", Qty: "1", Commission: "0.1", Time: time.Now().UnixMilli()}}})
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	store := state.NewStore()
	store.SetPosition(domain.Position{Symbol: "BTCUSDT", Size: 2, AvgPrice: 95})
	client := New("key", "secret", server.URL, "ws://example.invalid")
	report, _, err := client.Reconcile(context.Background(), store)
	if err != nil {
		t.Fatalf("reconcile failed: %v", err)
	}
	if report.PositionDrift == 0 {
		t.Fatalf("expected position drift to be detected")
	}
	if len(store.ReconciliationReports()) == 0 {
		t.Fatalf("expected reconciliation report stored")
	}
}

func TestRetryLogicWorks(t *testing.T) {
	var hits atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if hits.Add(1); hits.Load() < 3 {
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = w.Write([]byte(`rate limit`))
			return
		}
		_ = json.NewEncoder(w).Encode(orderResponseEnvelope{
			Data: bingxOrderReply{
				OrderID:       "ex-1",
				ClientOrderID: "cid-1",
				Status:        "NEW",
				UpdateTime:    time.Now().UnixMilli(),
			},
		})
	}))
	defer server.Close()

	client := New("key", "secret", server.URL, "ws://example.invalid")
	_, err := client.PlaceOrder(context.Background(), domain.ExecutionRequest{
		IdempotencyKey: "cid-1",
		Symbol:         "BTCUSDT",
		Side:           domain.SideBuy,
		Size:           1,
		Decision:       domain.DecisionExecute,
	})
	if err != nil {
		t.Fatalf("expected retry to succeed: %v", err)
	}
	if hits.Load() != 3 {
		t.Fatalf("expected 3 attempts, got %d", hits.Load())
	}
}
