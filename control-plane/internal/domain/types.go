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
	IdempotencyKey          string        `json:"idempotency_key"`
	Symbol                  string        `json:"symbol"`
	Side                    Side          `json:"side"`
	Size                    float64       `json:"size"`
	Price                   *float64      `json:"price,omitempty"`
	Decision                FinalDecision `json:"decision"`
	SignalTime              time.Time     `json:"signal_time"`
	MaxSlippageBps          float64       `json:"max_slippage_bps"`
	ReduceOnly              bool          `json:"reduce_only"`
	RequestTime             time.Time     `json:"request_timestamp,omitempty"`
	SendTime                time.Time     `json:"send_timestamp,omitempty"`
	ExpectedRealizedMarkout float64       `json:"expected_realized_markout,omitempty"`
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
	ID                   string        `json:"id"`
	IdempotencyKey       string        `json:"idempotency_key"`
	Symbol               string        `json:"symbol"`
	Side                 Side          `json:"side"`
	Size                 float64       `json:"size"`
	Filled               float64       `json:"filled"`
	Price                *float64      `json:"price,omitempty"`
	Status               OrderStatus   `json:"status"`
	CreatedAt            time.Time     `json:"created_at"`
	UpdatedAt            time.Time     `json:"updated_at"`
	RequestAt            time.Time     `json:"request_timestamp,omitempty"`
	SendAt               time.Time     `json:"send_timestamp,omitempty"`
	AckAt                time.Time     `json:"ack_timestamp,omitempty"`
	ExchangeAcceptAt     time.Time     `json:"exchange_accept_timestamp,omitempty"`
	FirstFillAt          time.Time     `json:"first_fill_timestamp,omitempty"`
	LastFillAt           time.Time     `json:"last_fill_timestamp,omitempty"`
	PartialFillRatio     float64       `json:"partial_fill_ratio"`
	WeightedAvgFillPrice float64       `json:"weighted_avg_fill_price"`
	SpreadAtExecution    float64       `json:"spread_at_execution"`
	SlippageRealBps      float64       `json:"slippage_real_bps"`
	QueueDelay           time.Duration `json:"queue_delay"`
	CancelReason         string        `json:"cancel_reason,omitempty"`
	RejectReason         string        `json:"reject_reason,omitempty"`
}

type MarkoutCurve struct {
	PnL100ms float64 `json:"markout_100ms"`
	PnL500ms float64 `json:"markout_500ms"`
	PnL1s    float64 `json:"markout_1s"`
	PnL5s    float64 `json:"markout_5s"`
}

type ExecutionEvent struct {
	OrderID                 string        `json:"order_id"`
	Symbol                  string        `json:"symbol"`
	FillQuality             float64       `json:"fill_quality"`
	SlippageReal            float64       `json:"slippage_real"`
	AdverseSelectionScore   float64       `json:"adverse_selection_score"`
	MarkoutCurve            MarkoutCurve  `json:"markout_curve"`
	ExecutionLatency        time.Duration `json:"execution_latency"`
	CompetitionFlag         string        `json:"competition_flag"`
	Simulated               bool          `json:"simulated"`
	PartialFillRatio        float64       `json:"partial_fill_ratio"`
	ExpectedRealizedMarkout float64       `json:"expected_realized_markout,omitempty"`
	RealizedPnL             float64       `json:"realized_pnl,omitempty"`
}

type Position struct {
	Symbol   string    `json:"symbol"`
	Size     float64   `json:"size"`
	AvgPrice float64   `json:"avg_price"`
	Updated  time.Time `json:"updated"`
}

type RiskStatus struct {
	KillSwitch               bool              `json:"kill_switch"`
	DailyPnL                 float64           `json:"daily_pnl"`
	MaxDailyLossUSD          float64           `json:"max_daily_loss_usd"`
	CircuitBreakers          map[string]bool   `json:"circuit_breakers"`
	SystemStressIndex        float64           `json:"system_stress_index"`
	MempoolPressure          float64           `json:"mempool_pressure"`
	ExecutionFragility       float64           `json:"execution_fragility"`
	ExposureRisk             float64           `json:"exposure_risk"`
	ActiveCircuitState       string            `json:"active_circuit_state"`
	CurrentRiskMultiplier    float64           `json:"current_risk_multiplier"`
	RejectionCountLastWindow uint64            `json:"rejection_count_last_window"`
	RegimeState              string            `json:"regime_state"`
	RealEvents               uint64            `json:"real_events"`
	ContaminatedEvents       uint64            `json:"contaminated_events"`
	FailureBreakdown         map[string]uint64 `json:"failure_breakdown"`
	RealizedPnLEMA           float64           `json:"realized_pnl_ema"`
	MarkoutDegradationScore  float64           `json:"markout_degradation_score"`
	AdverseSelectionEMA      float64           `json:"adverse_selection_ema"`
	SlippageVarianceEMA      float64           `json:"slippage_variance_ema"`
	ExecutionFailureRateEMA  float64           `json:"execution_failure_rate_ema"`
}
