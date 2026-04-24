package execution

import (
	"context"
	"time"

	"control-plane/internal/api"
	"control-plane/internal/domain"
)

type PaperExchange struct{}

func NewPaperExchange(_ any, _ chan<- api.Update) *PaperExchange {
	return &PaperExchange{}
}

func (p *PaperExchange) SendOrder(ctx context.Context, order domain.Order) ([]ExchangeStep, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	default:
	}
	now := time.Now().UTC()
	reference := 1.0
	if order.Price != nil && *order.Price > 0 {
		reference = *order.Price
	}
	ackAt := now.Add(2 * time.Millisecond)
	steps := []ExchangeStep{
		{
			Status:     domain.OrderAck,
			OccurredAt: ackAt,
		},
	}

	fills := buildPaperFills(order, reference, ackAt)
	for index, fill := range fills {
		event := domain.ExecutionEvent{
			OrderID:                 order.ID,
			FillID:                  fill.FillID,
			Symbol:                  order.Symbol,
			Status:                  domain.OrderPartial,
			FilledQuantity:          fill.Quantity,
			FillPrice:               fill.Price,
			LiquidityFlag:           fill.LiquidityFlag,
			FeeAmount:               fill.FeeAmount,
			RebateAmount:            fill.RebateAmount,
			FundingAmount:           fill.FundingAmount,
			FillQuality:             fillQuality(fill, order),
			SlippageReal:            slippageBps(order, fill.Price),
			AdverseSelectionScore:   0,
			MarkoutCurve:            domain.MarkoutCurve{},
			ExecutionLatency:        fill.EventTime.Sub(order.SendAt),
			LatencyBreakdown:        latencyBreakdown(order, ackAt, fill.EventTime, index == len(fills)-1),
			CompetitionFlag:         "EXTERNAL_AUTHORIZED",
			Simulated:               false,
			PartialFillRatio:        fill.Quantity / max(order.Size, 1e-12),
			ExpectedRealizedMarkout: 0,
			RealizedPnL:             0,
		}
		status := domain.OrderPartial
		if index == len(fills)-1 {
			status = domain.OrderFilled
			event.Status = domain.OrderFilled
		}
		steps = append(steps, ExchangeStep{
			Status:     status,
			OccurredAt: fill.EventTime,
			Ledger:     &fill,
			Execution:  &event,
		})
	}
	return steps, nil
}

func buildPaperFills(order domain.Order, reference float64, ackAt time.Time) []domain.FillLedgerEntry {
	remaining := order.Size
	if remaining <= 0 {
		return nil
	}
	if order.Price != nil {
		halfQty := remaining * 0.5
		first := newLedgerFill(order, "fill-a", reference, halfQty, domain.LiquidityMaker, ackAt.Add(4*time.Millisecond))
		second := newLedgerFill(order, "fill-b", reference, remaining-halfQty, domain.LiquidityTaker, ackAt.Add(9*time.Millisecond))
		return []domain.FillLedgerEntry{first, second}
	}
	return []domain.FillLedgerEntry{
		newLedgerFill(order, "fill-a", reference, remaining, domain.LiquidityTaker, ackAt.Add(5*time.Millisecond)),
	}
}

func newLedgerFill(order domain.Order, fillID string, price float64, quantity float64, flag domain.LiquidityFlag, eventTime time.Time) domain.FillLedgerEntry {
	feeRate := 0.0004
	rebate := 0.0
	if flag == domain.LiquidityMaker {
		feeRate = 0.0002
		rebate = price * quantity * 0.00005
	}
	return domain.FillLedgerEntry{
		OrderID:       order.ID,
		FillID:        fillID,
		Symbol:        order.Symbol,
		Side:          order.Side,
		Price:         price,
		Quantity:      quantity,
		LiquidityFlag: flag,
		FeeAmount:     price * quantity * feeRate,
		FeeAsset:      "USDT",
		RebateAmount:  rebate,
		FundingAmount: 0,
		EventTime:     eventTime,
		EventTimeUnixMs: eventTime.UnixMilli(),
	}
}

func fillQuality(fill domain.FillLedgerEntry, order domain.Order) float64 {
	ratio := fill.Quantity / max(order.Size, 1e-12)
	quality := 0.75 + ratio*0.20
	if fill.LiquidityFlag == domain.LiquidityMaker {
		quality += 0.03
	}
	return min(quality, 1.0)
}

func latencyBreakdown(order domain.Order, ackAt, fillAt time.Time, final bool) domain.LatencyBreakdown {
	fullFillLatency := fillAt.Sub(order.SendAt)
	firstFillLatency := fullFillLatency
	if !final {
		firstFillLatency = fillAt.Sub(order.SendAt)
	}
	return domain.LatencyBreakdown{
		DecisionLatency:  order.SendAt.Sub(order.RequestAt),
		SendLatency:      0,
		AckLatency:       ackAt.Sub(order.SendAt),
		FirstFillLatency: firstFillLatency,
		FullFillLatency:  fullFillLatency,
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

func max(a, b float64) float64 {
	if a > b {
		return a
	}
	return b
}

func min(a, b float64) float64 {
	if a < b {
		return a
	}
	return b
}
