package bingx

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"net/http"
	"strconv"
	"strings"
	"time"

	"control-plane/internal/domain"
	"control-plane/internal/execution"
	"github.com/gorilla/websocket"
)

type listenKeyEnvelope struct {
	Code int `json:"code"`
	Msg  string `json:"msg"`
	Data struct {
		ListenKey string `json:"listenKey"`
	} `json:"data"`
}

type userStreamEnvelope struct {
	Event string          `json:"e"`
	Type  string          `json:"type"`
	Data  json.RawMessage `json:"data"`
}

type executionReportEvent struct {
	EventType        string `json:"e"`
	OrderID          string `json:"i"`
	ClientOrderID    string `json:"c"`
	Symbol           string `json:"s"`
	Side             string `json:"S"`
	Status           string `json:"X"`
	ExecutionType    string `json:"x"`
	OrderPrice       string `json:"p"`
	OrderQty         string `json:"q"`
	LastPrice        string `json:"L"`
	LastQty          string `json:"l"`
	CumulativeQty    string `json:"z"`
	FeeAmount        string `json:"n"`
	FeeAsset         string `json:"N"`
	Maker            bool   `json:"m"`
	TradeID          string `json:"t"`
	EventTime        int64  `json:"E"`
	TransactionTime  int64  `json:"T"`
	RejectReason     string `json:"r"`
}

func (c *BingXClient) runUserStream(ctx context.Context) {
	for ctx.Err() == nil {
		if err := c.consumeUserStream(ctx); err != nil && ctx.Err() == nil {
			slog.Warn("bingx user stream reconnecting", "err", err)
			time.Sleep(time.Second)
		}
	}
}

func (c *BingXClient) consumeUserStream(ctx context.Context) error {
	listenKey, err := c.initListenKey(ctx)
	if err != nil {
		return err
	}
	u := fmt.Sprintf("%s/market?listenKey=%s", c.wsURL, listenKey)
	conn, _, err := websocket.DefaultDialer.DialContext(ctx, u, http.Header{"X-BX-APIKEY": []string{c.apiKey}})
	if err != nil {
		return err
	}
	defer conn.Close()

	keepAliveTicker := time.NewTicker(20 * time.Minute)
	defer keepAliveTicker.Stop()

	for ctx.Err() == nil {
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-keepAliveTicker.C:
			if err := c.keepAliveListenKey(ctx, listenKey); err != nil {
				return err
			}
		default:
			_ = conn.SetReadDeadline(time.Now().Add(60 * time.Second))
			_, raw, err := conn.ReadMessage()
			if err != nil {
				return err
			}
			if update, ok := parseExecutionReport(raw); ok {
				select {
				case c.updates <- update:
				default:
				}
			}
		}
	}
	return ctx.Err()
}

func (c *BingXClient) initListenKey(ctx context.Context) (string, error) {
	var envelope listenKeyEnvelope
	if err := c.doSigned(ctx, "POST", "/openApi/user/auth/userDataStream", nil, nil, &envelope); err != nil {
		return "", err
	}
	c.mu.Lock()
	c.listenKey = envelope.Data.ListenKey
	c.lastListenInit = time.Now().UTC()
	c.mu.Unlock()
	return envelope.Data.ListenKey, nil
}

func (c *BingXClient) keepAliveListenKey(ctx context.Context, listenKey string) error {
	params := map[string]string{"listenKey": listenKey}
	return c.doSigned(ctx, "PUT", "/openApi/user/auth/userDataStream", params, nil, nil)
}

func parseExecutionReport(raw []byte) (execution.AsyncExchangeUpdate, bool) {
	var event executionReportEvent
	if err := json.Unmarshal(raw, &event); err == nil && (event.EventType == "executionReport" || event.OrderID != "") {
		return mapExecutionReport(event), true
	}
	var envelope userStreamEnvelope
	if err := json.Unmarshal(raw, &envelope); err != nil {
		return execution.AsyncExchangeUpdate{}, false
	}
	if !strings.EqualFold(envelope.Event, "executionReport") && !strings.EqualFold(envelope.Type, "executionReport") {
		return execution.AsyncExchangeUpdate{}, false
	}
	if err := json.Unmarshal(envelope.Data, &event); err != nil {
		return execution.AsyncExchangeUpdate{}, false
	}
	return mapExecutionReport(event), true
}

func mapExecutionReport(event executionReportEvent) execution.AsyncExchangeUpdate {
	lastQty, _ := strconv.ParseFloat(event.LastQty, 64)
	lastPrice, _ := strconv.ParseFloat(event.LastPrice, 64)
	cumulativeQty, _ := strconv.ParseFloat(event.CumulativeQty, 64)
	fee, _ := strconv.ParseFloat(event.FeeAmount, 64)
	status := mapOrderStatus(event.Status)
	var ledger *domain.FillLedgerEntry
	var executionEvent *domain.ExecutionEvent
	if lastQty > 0 {
		liquidity := domain.LiquidityTaker
		if event.Maker {
			liquidity = domain.LiquidityMaker
		}
		ledger = &domain.FillLedgerEntry{
			OrderID:         event.OrderID,
			FillID:          fallbackID(event.TradeID, event.OrderID),
			Symbol:          strings.ToUpper(event.Symbol),
			Side:            domain.Side(strings.ToUpper(event.Side)),
			Price:           lastPrice,
			Quantity:        lastQty,
			LiquidityFlag:   liquidity,
			FeeAmount:       fee,
			FeeAsset:        event.FeeAsset,
			RebateAmount:    0,
			FundingAmount:   0,
			EventTime:       msOrNow(event.TransactionTime),
			EventTimeUnixMs: event.TransactionTime,
		}
		executionEvent = &domain.ExecutionEvent{
			OrderID:          event.OrderID,
			FillID:           ledger.FillID,
			Symbol:           strings.ToUpper(event.Symbol),
			Status:           status,
			FilledQuantity:   lastQty,
			FillPrice:        lastPrice,
			LiquidityFlag:    ledger.LiquidityFlag,
			FeeAmount:        fee,
			FillQuality:      1.0,
			SlippageReal:     0,
			ExecutionLatency: 0,
			CompetitionFlag:  "EXTERNAL_AUTHORIZED",
			Simulated:        false,
			PartialFillRatio: cumulativeQty / maxQty(event.OrderQty),
		}
	}
	step := execution.ExchangeStep{
		Status:          status,
		OccurredAt:      msOrNow(event.EventTime),
		CancelReason:    event.RejectReason,
		ExchangeOrderID: event.OrderID,
		Ledger:          ledger,
		Execution:       executionEvent,
	}
	return execution.AsyncExchangeUpdate{
		OrderID:         "",
		ExchangeOrderID: event.OrderID,
		IdempotencyKey:  event.ClientOrderID,
		Step:            step,
	}
}

func maxQty(raw string) float64 {
	value, _ := strconv.ParseFloat(raw, 64)
	if value <= 0 {
		return 1e-9
	}
	return value
}
