package domain

import "time"

type Side string

const (
	SideBuy  Side = "BUY"
	SideSell Side = "SELL"
)

type MarketEventType string

const (
	EventTrade MarketEventType = "TRADE"
	EventBook  MarketEventType = "BOOK"
)

type MarketEvent struct {
	Type      MarketEventType `json:"type"`
	Symbol    string          `json:"symbol"`
	Timestamp time.Time       `json:"timestamp"`
	Price     float64         `json:"price"`
	Volume    float64         `json:"volume"`
	Side      Side            `json:"side"`
	BestBid   float64         `json:"best_bid"`
	BestAsk   float64         `json:"best_ask"`
	BidVolume float64         `json:"bid_volume"`
	AskVolume float64         `json:"ask_volume"`
}

type FinalDecision string

const (
	DecisionExecute FinalDecision = "Execute"
	DecisionWait    FinalDecision = "Wait"
	DecisionSkip    FinalDecision = "Skip"
)

type ExecutionRequest struct {
	IdempotencyKey string        `json:"idempotency_key"`
	Symbol         string        `json:"symbol"`
	Side           Side          `json:"side"`
	Size           float64       `json:"size"`
	Price          *float64      `json:"price,omitempty"`
	Decision       FinalDecision `json:"decision"`
	SignalTime     time.Time     `json:"signal_time"`
	MaxSlippageBps float64       `json:"max_slippage_bps"`
	ReduceOnly     bool          `json:"reduce_only"`
}

type OrderStatus string

const (
	OrderNew      OrderStatus = "NEW"
	OrderSent     OrderStatus = "SENT"
	OrderPartial  OrderStatus = "PARTIAL"
	OrderFilled   OrderStatus = "FILLED"
	OrderCanceled OrderStatus = "CANCELED"
	OrderRejected OrderStatus = "REJECTED"
)

type Order struct {
	ID             string      `json:"id"`
	IdempotencyKey string      `json:"idempotency_key"`
	Symbol         string      `json:"symbol"`
	Side           Side        `json:"side"`
	Size           float64     `json:"size"`
	Filled         float64     `json:"filled"`
	Price          *float64    `json:"price,omitempty"`
	Status         OrderStatus `json:"status"`
	CreatedAt      time.Time   `json:"created_at"`
	UpdatedAt      time.Time   `json:"updated_at"`
	RejectReason   string      `json:"reject_reason,omitempty"`
}

type Position struct {
	Symbol   string    `json:"symbol"`
	Size     float64   `json:"size"`
	AvgPrice float64   `json:"avg_price"`
	Updated  time.Time `json:"updated"`
}

type RiskStatus struct {
	KillSwitch      bool            `json:"kill_switch"`
	DailyPnL        float64         `json:"daily_pnl"`
	MaxDailyLossUSD float64         `json:"max_daily_loss_usd"`
	CircuitBreakers map[string]bool `json:"circuit_breakers"`
}
