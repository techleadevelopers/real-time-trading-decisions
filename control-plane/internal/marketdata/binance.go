package marketdata

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"net/url"
	"strconv"
	"strings"
	"time"

	"control-plane/internal/config"
	"control-plane/internal/domain"
	"github.com/gorilla/websocket"
)

type Event = domain.MarketEvent

type BinanceGateway struct {
	cfg config.MarketDataConfig
	out chan<- Event
}

func NewBinanceGateway(cfg config.MarketDataConfig, out chan<- Event) *BinanceGateway {
	return &BinanceGateway{cfg: cfg, out: out}
}

func (g *BinanceGateway) Run(ctx context.Context) {
	if len(g.cfg.Symbols) == 0 {
		slog.Warn("market data disabled: no symbols configured")
		return
	}
	for ctx.Err() == nil {
		if err := g.runOnce(ctx); err != nil && ctx.Err() == nil {
			slog.Warn("market data reconnecting", "err", err)
			timer := time.NewTimer(g.cfg.ReconnectBackoff)
			select {
			case <-ctx.Done():
				timer.Stop()
			case <-timer.C:
			}
		}
	}
}

func (g *BinanceGateway) runOnce(ctx context.Context) error {
	streams := make([]string, 0, len(g.cfg.Symbols)*2)
	for _, symbol := range g.cfg.Symbols {
		s := strings.ToLower(symbol)
		streams = append(streams, s+"@aggTrade", s+"@depth5@100ms")
	}
	u := url.URL{Scheme: "wss", Host: "stream.binance.com:9443", Path: "/stream", RawQuery: "streams=" + strings.Join(streams, "/")}
	conn, _, err := websocket.DefaultDialer.DialContext(ctx, u.String(), nil)
	if err != nil {
		return err
	}
	defer conn.Close()
	slog.Info("binance websocket connected", "url", u.String())

	for ctx.Err() == nil {
		_, msg, err := conn.ReadMessage()
		if err != nil {
			return err
		}
		var envelope combinedEnvelope
		if err := json.Unmarshal(msg, &envelope); err != nil {
			slog.Debug("market data parse failed", "err", err)
			continue
		}
		if event, ok := envelope.toEvent(); ok {
			g.offer(ctx, event)
		}
	}
	return ctx.Err()
}

func (g *BinanceGateway) offer(ctx context.Context, event Event) {
	select {
	case g.out <- event:
	default:
		timer := time.NewTimer(g.cfg.SendTimeout)
		select {
		case g.out <- event:
		case <-timer.C:
			slog.Warn("market data dropped under backpressure", "symbol", event.Symbol, "type", event.Type)
		case <-ctx.Done():
		}
		timer.Stop()
	}
}

type combinedEnvelope struct {
	Stream string          `json:"stream"`
	Data   json.RawMessage `json:"data"`
}

func (e combinedEnvelope) toEvent() (Event, bool) {
	if strings.Contains(e.Stream, "@aggTrade") {
		var t aggTrade
		if json.Unmarshal(e.Data, &t) != nil {
			return Event{}, false
		}
		price, _ := strconv.ParseFloat(t.Price, 64)
		qty, _ := strconv.ParseFloat(t.Quantity, 64)
		side := domain.SideBuy
		if t.BuyerIsMaker {
			side = domain.SideSell
		}
		return Event{Type: domain.EventTrade, Symbol: strings.ToUpper(t.Symbol), Timestamp: ms(t.EventTime), Price: price, Volume: qty, Side: side}, true
	}
	if strings.Contains(e.Stream, "@depth") {
		var d depth
		if json.Unmarshal(e.Data, &d) != nil {
			return Event{}, false
		}
		bidPrice, bidQty := top(d.Bids)
		askPrice, askQty := top(d.Asks)
		symbol := strings.ToUpper(strings.Split(e.Stream, "@")[0])
		return Event{Type: domain.EventBook, Symbol: symbol, Timestamp: time.Now().UTC(), BestBid: bidPrice, BestAsk: askPrice, BidVolume: bidQty, AskVolume: askQty}, true
	}
	return Event{}, false
}

type aggTrade struct {
	EventTime    int64  `json:"E"`
	Symbol       string `json:"s"`
	Price        string `json:"p"`
	Quantity     string `json:"q"`
	BuyerIsMaker bool   `json:"m"`
}

type depth struct {
	Bids [][]string `json:"b"`
	Asks [][]string `json:"a"`
}

func top(levels [][]string) (float64, float64) {
	if len(levels) == 0 || len(levels[0]) < 2 {
		return 0, 0
	}
	p, _ := strconv.ParseFloat(levels[0][0], 64)
	q, _ := strconv.ParseFloat(levels[0][1], 64)
	return p, q
}

func ms(value int64) time.Time {
	if value <= 0 {
		return time.Now().UTC()
	}
	return time.UnixMilli(value).UTC()
}

func (g *BinanceGateway) String() string {
	return fmt.Sprintf("binance_gateway(symbols=%v)", g.cfg.Symbols)
}
