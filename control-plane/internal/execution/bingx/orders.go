package bingx

import (
	"context"
	"fmt"
	"strconv"
	"strings"
	"time"

	"control-plane/internal/domain"
	"control-plane/internal/execution"
)

type orderResponseEnvelope struct {
	Code int             `json:"code"`
	Msg  string          `json:"msg"`
	Data bingxOrderReply `json:"data"`
}

type bingxOrderReply struct {
	OrderID       string `json:"orderId"`
	ClientOrderID string `json:"clientOrderId"`
	Status        string `json:"status"`
	Symbol        string `json:"symbol"`
	Side          string `json:"side"`
	Price         string `json:"price"`
	OrigQty       string `json:"origQty"`
	ExecutedQty   string `json:"executedQty"`
	UpdateTime    int64  `json:"updateTime"`
	Time          int64  `json:"time"`
}

type bingxOpenOrdersEnvelope struct {
	Code int              `json:"code"`
	Msg  string           `json:"msg"`
	Data []bingxOrderReply `json:"data"`
}

func (c *BingXClient) SendOrder(ctx context.Context, order domain.Order) ([]execution.ExchangeStep, error) {
	req := domain.ExecutionRequest{
		IdempotencyKey:          order.IdempotencyKey,
		Symbol:                  order.Symbol,
		Side:                    order.Side,
		Size:                    order.Size,
		Price:                   order.Price,
		Decision:                domain.DecisionExecute,
		SignalTime:              order.RequestAt,
		RequestTime:             order.RequestAt,
		SendTime:                order.SendAt,
		ExpectedRealizedMarkout: order.ExpectedRealizedMarkout,
		ReduceOnly:              false,
	}
	response, err := c.PlaceOrder(ctx, req)
	if err != nil {
		return nil, err
	}
	return []execution.ExchangeStep{{
		Status:          domain.OrderAck,
		OccurredAt:      msOrNow(response.UpdateTime),
		ExchangeOrderID: response.OrderID,
	}}, nil
}

func (c *BingXClient) PlaceOrder(ctx context.Context, req domain.ExecutionRequest) (bingxOrderReply, error) {
	params := map[string]string{
		"symbol":        strings.ToUpper(req.Symbol),
		"side":          string(req.Side),
		"type":          orderType(req),
		"quantity":      trimFloat(req.Size),
		"clientOrderId": req.IdempotencyKey,
	}
	if req.Price != nil {
		params["price"] = trimFloat(*req.Price)
		params["timeInForce"] = "GTC"
	}
	if req.ReduceOnly {
		params["reduceOnly"] = "true"
	}
	var envelope orderResponseEnvelope
	err := c.doSigned(ctx, "POST", "/openApi/swap/v2/trade/order", params, nil, &envelope)
	return envelope.Data, err
}

func (c *BingXClient) CancelOrder(ctx context.Context, orderID string) error {
	params := map[string]string{"orderId": orderID}
	return c.doSigned(ctx, "DELETE", "/openApi/swap/v2/trade/order", params, nil, nil)
}

func (c *BingXClient) CancelRemote(ctx context.Context, orderID string) error {
	return c.CancelOrder(ctx, orderID)
}

func (c *BingXClient) ReplaceRemote(ctx context.Context, orderID string, newPrice float64) error {
	params := map[string]string{
		"orderId": orderID,
		"price":   trimFloat(newPrice),
	}
	return c.doSigned(ctx, "PUT", "/openApi/swap/v2/trade/order", params, nil, nil)
}

func (c *BingXClient) GetOpenOrders(ctx context.Context, symbol string) ([]domain.Order, error) {
	params := map[string]string{"symbol": strings.ToUpper(symbol)}
	var envelope bingxOpenOrdersEnvelope
	if err := c.doSigned(ctx, "GET", "/openApi/swap/v2/trade/openOrders", params, nil, &envelope); err != nil {
		return nil, err
	}
	out := make([]domain.Order, 0, len(envelope.Data))
	for _, order := range envelope.Data {
		out = append(out, mapOrder(order))
	}
	return out, nil
}

func orderType(req domain.ExecutionRequest) string {
	if req.Price != nil {
		return "LIMIT"
	}
	return "MARKET"
}

func mapOrder(order bingxOrderReply) domain.Order {
	price, _ := strconv.ParseFloat(order.Price, 64)
	size, _ := strconv.ParseFloat(order.OrigQty, 64)
	filled, _ := strconv.ParseFloat(order.ExecutedQty, 64)
	return domain.Order{
		ID:              fallbackID(order.ClientOrderID, order.OrderID),
		ExchangeOrderID: order.OrderID,
		IdempotencyKey:  order.ClientOrderID,
		Symbol:          strings.ToUpper(order.Symbol),
		Side:            domain.Side(strings.ToUpper(order.Side)),
		Size:            size,
		Filled:          filled,
		Price:           floatPtr(price),
		Status:          mapOrderStatus(order.Status),
		CreatedAt:       msOrNow(order.Time),
		UpdatedAt:       msOrNow(order.UpdateTime),
		RequestAt:       msOrNow(order.Time),
		SendAt:          msOrNow(order.Time),
	}
}

func mapOrderStatus(status string) domain.OrderStatus {
	switch strings.ToUpper(status) {
	case "NEW":
		return domain.OrderSent
	case "PARTIALLY_FILLED":
		return domain.OrderPartial
	case "FILLED":
		return domain.OrderFilled
	case "CANCELED", "CANCELLED":
		return domain.OrderCanceled
	case "FAILED", "REJECTED":
		return domain.OrderRejected
	default:
		return domain.OrderNew
	}
}

func trimFloat(value float64) string {
	return strconv.FormatFloat(value, 'f', -1, 64)
}

func floatPtr(v float64) *float64 {
	if v == 0 {
		return nil
	}
	return &v
}

func fallbackID(primary, fallback string) string {
	if primary != "" {
		return primary
	}
	if fallback != "" {
		return fallback
	}
	return fmt.Sprintf("bingx-%d", time.Now().UTC().UnixNano())
}

func msOrNow(v int64) time.Time {
	if v <= 0 {
		return time.Now().UTC()
	}
	return time.UnixMilli(v).UTC()
}
