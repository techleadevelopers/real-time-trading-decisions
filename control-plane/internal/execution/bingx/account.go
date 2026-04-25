package bingx

import (
	"context"
	"strconv"
	"strings"
	"time"

	"control-plane/internal/domain"
)

type balanceEnvelope struct {
	Code int             `json:"code"`
	Msg  string          `json:"msg"`
	Data bingxBalanceRaw `json:"data"`
}

type bingxBalanceRaw struct {
	Balance          string `json:"balance"`
	AvailableBalance string `json:"availableMargin"`
	UnrealizedPnL    string `json:"unrealizedProfit"`
}

type positionsEnvelope struct {
	Code int                `json:"code"`
	Msg  string             `json:"msg"`
	Data []bingxPositionRaw `json:"data"`
}

type bingxPositionRaw struct {
	Symbol       string `json:"symbol"`
	PositionAmt  string `json:"positionAmt"`
	AvgPrice     string `json:"avgPrice"`
	Leverage     string `json:"leverage"`
	UnrealizedPnL string `json:"unrealizedProfit"`
	UpdateTime   int64  `json:"updateTime"`
}

type fillsEnvelope struct {
	Code int            `json:"code"`
	Msg  string         `json:"msg"`
	Data []bingxFillRaw `json:"data"`
}

type bingxFillRaw struct {
	OrderID       string `json:"orderId"`
	ClientOrderID string `json:"clientOrderId"`
	TradeID       string `json:"tradeId"`
	Symbol        string `json:"symbol"`
	Side          string `json:"side"`
	Price         string `json:"price"`
	Qty           string `json:"qty"`
	Commission    string `json:"commission"`
	RealizedPnL   string `json:"realizedPnl"`
	Maker         bool   `json:"maker"`
	Time          int64  `json:"time"`
}

func (c *BingXClient) GetPositions(ctx context.Context) ([]domain.Position, error) {
	var envelope positionsEnvelope
	if err := c.doSigned(ctx, "GET", "/openApi/swap/v2/user/positions", nil, nil, &envelope); err != nil {
		return nil, err
	}
	out := make([]domain.Position, 0, len(envelope.Data))
	for _, position := range envelope.Data {
		size, _ := strconv.ParseFloat(position.PositionAmt, 64)
		avgPrice, _ := strconv.ParseFloat(position.AvgPrice, 64)
		out = append(out, domain.Position{
			Symbol:   strings.ToUpper(position.Symbol),
			Size:     size,
			AvgPrice: avgPrice,
			Updated:  msOrNow(position.UpdateTime),
		})
	}
	return out, nil
}

func (c *BingXClient) GetBalance(ctx context.Context) (domain.AccountState, error) {
	var envelope balanceEnvelope
	if err := c.doSigned(ctx, "GET", "/openApi/swap/v2/user/balance", nil, nil, &envelope); err != nil {
		return domain.AccountState{}, err
	}
	balance, _ := strconv.ParseFloat(envelope.Data.Balance, 64)
	available, _ := strconv.ParseFloat(envelope.Data.AvailableBalance, 64)
	unrealized, _ := strconv.ParseFloat(envelope.Data.UnrealizedPnL, 64)
	return domain.AccountState{
		Balance:          balance,
		AvailableBalance: available,
		UnrealizedPnL:    unrealized,
		UpdatedAt:        time.Now().UTC(),
	}, nil
}

func (c *BingXClient) GetFills(ctx context.Context, symbol string, since int64) ([]domain.FillLedgerEntry, error) {
	params := map[string]string{
		"symbol": strings.ToUpper(symbol),
		"startTs": strconv.FormatInt(since, 10),
	}
	var envelope fillsEnvelope
	if err := c.doSigned(ctx, "GET", "/openApi/swap/v2/trade/allFillOrders", params, nil, &envelope); err != nil {
		return nil, err
	}
	out := make([]domain.FillLedgerEntry, 0, len(envelope.Data))
	for _, fill := range envelope.Data {
		price, _ := strconv.ParseFloat(fill.Price, 64)
		qty, _ := strconv.ParseFloat(fill.Qty, 64)
		fee, _ := strconv.ParseFloat(fill.Commission, 64)
		liquidity := domain.LiquidityTaker
		if fill.Maker {
			liquidity = domain.LiquidityMaker
		}
		out = append(out, domain.FillLedgerEntry{
			OrderID:         fill.OrderID,
			FillID:          fill.TradeID,
			Symbol:          strings.ToUpper(fill.Symbol),
			Side:            domain.Side(strings.ToUpper(fill.Side)),
			Price:           price,
			Quantity:        qty,
			LiquidityFlag:   liquidity,
			FeeAmount:       fee,
			FeeAsset:        "USDT",
			RebateAmount:    0,
			FundingAmount:   0,
			EventTime:       msOrNow(fill.Time),
			EventTimeUnixMs: fill.Time,
		})
	}
	return out, nil
}
