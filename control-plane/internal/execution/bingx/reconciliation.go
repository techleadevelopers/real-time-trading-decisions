package bingx

import (
	"context"
	"fmt"
	"time"

	"control-plane/internal/domain"
	"control-plane/internal/state"
)

func (c *BingXClient) Reconcile(ctx context.Context, store *state.Store) (domain.ReconciliationReport, domain.AccountState, error) {
	report := domain.ReconciliationReport{
		Matched:     true,
		GeneratedAt: time.Now().UTC(),
		Details:     []string{},
	}
	account, err := c.GetAccountState(ctx)
	if err != nil {
		return report, domain.AccountState{}, err
	}
	store.SetAccountState(account)

	positions, err := c.GetPositions(ctx)
	if err != nil {
		return report, account, err
	}
	for _, position := range positions {
		local := store.Position(position.Symbol)
		if local.Symbol != "" && (local.Size != position.Size || local.AvgPrice != position.AvgPrice) {
			report.PositionDrift++
			report.Details = append(report.Details, fmt.Sprintf("reconciled position drift on %s", position.Symbol))
		}
		store.SetPosition(position)
	}

	for _, symbol := range activeSymbols(positions, store.Orders()) {
		orders, err := c.GetOpenOrders(ctx, symbol)
		if err != nil {
			return report, account, err
		}
		for _, order := range orders {
			if _, ok := store.OrderByIdempotency(order.IdempotencyKey); !ok {
				report.OrphanOrders++
				report.Details = append(report.Details, fmt.Sprintf("reconciled orphan order %s", order.ExchangeOrderID))
			}
			store.UpsertOrder(order)
		}
		fills, err := c.GetFills(ctx, symbol, time.Now().Add(-24*time.Hour).UnixMilli())
		if err != nil {
			return report, account, err
		}
		for _, fill := range fills {
			recon := store.Reconciliation()
			if recon.LedgerQtyByOrder[fill.OrderID] == 0 {
				report.MissingFills++
				report.Details = append(report.Details, fmt.Sprintf("reconciled missing fill %s", fill.FillID))
				order, ok := store.OrderByExchangeID(fill.OrderID)
				if !ok {
					order = domain.Order{
						ID:              fallbackID(fill.OrderID, fill.OrderID),
						ExchangeOrderID: fill.OrderID,
						IdempotencyKey:  fill.OrderID,
						Symbol:          fill.Symbol,
						Side:            fill.Side,
						Size:            fill.Quantity,
						Status:          domain.OrderPartial,
						CreatedAt:       fill.EventTime,
						UpdatedAt:       fill.EventTime,
						RequestAt:       fill.EventTime,
						SendAt:          fill.EventTime,
					}
					store.UpsertOrder(order)
				}
				_, _, _ = store.ApplyExternalFill(order.ID, fill, domain.ExecutionEvent{
					OrderID:          fill.OrderID,
					FillID:           fill.FillID,
					Symbol:           fill.Symbol,
					Status:           domain.OrderPartial,
					FilledQuantity:   fill.Quantity,
					FillPrice:        fill.Price,
					LiquidityFlag:    fill.LiquidityFlag,
					FeeAmount:        fill.FeeAmount,
					FundingAmount:    fill.FundingAmount,
					ExecutionLatency: 0,
					CompetitionFlag:  "RECONCILED",
					Simulated:        false,
					PartialFillRatio: 1.0,
				})
			}
		}
	}

	if store.Reconciliation().Matched && report.PositionDrift == 0 {
		report.Matched = true
	}
	store.AppendReconciliationReport(report)
	return report, account, nil
}

func activeSymbols(positions []domain.Position, orders []domain.Order) []string {
	seen := make(map[string]struct{})
	out := make([]string, 0, len(positions)+len(orders))
	for _, position := range positions {
		if position.Symbol == "" {
			continue
		}
		if _, ok := seen[position.Symbol]; ok {
			continue
		}
		seen[position.Symbol] = struct{}{}
		out = append(out, position.Symbol)
	}
	for _, order := range orders {
		if order.Symbol == "" {
			continue
		}
		if _, ok := seen[order.Symbol]; ok {
			continue
		}
		seen[order.Symbol] = struct{}{}
		out = append(out, order.Symbol)
	}
	return out
}
