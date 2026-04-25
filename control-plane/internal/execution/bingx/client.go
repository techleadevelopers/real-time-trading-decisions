package bingx

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"time"

	"control-plane/internal/domain"
	"control-plane/internal/execution"
)

type BingXClient struct {
	apiKey    string
	secretKey string
	baseURL   string
	wsURL     string
	http      *http.Client

	mu             sync.RWMutex
	updates        chan execution.AsyncExchangeUpdate
	listenKey      string
	lastListenInit time.Time
	timeOffsetMs   int64
}

func New(apiKey, secretKey, baseURL, wsURL string) *BingXClient {
	if baseURL == "" {
		baseURL = "https://open-api.bingx.com"
	}
	if wsURL == "" {
		wsURL = "wss://open-api-swap.bingx.com"
	}
	return &BingXClient{
		apiKey:    apiKey,
		secretKey: secretKey,
		baseURL:   strings.TrimRight(baseURL, "/"),
		wsURL:     strings.TrimRight(wsURL, "/"),
		http: &http.Client{
			Timeout: 10 * time.Second,
		},
		updates: make(chan execution.AsyncExchangeUpdate, 1024),
	}
}

func (c *BingXClient) Updates() <-chan execution.AsyncExchangeUpdate {
	return c.updates
}

func (c *BingXClient) Start(ctx context.Context, handler func(execution.AsyncExchangeUpdate)) error {
	go func() {
		for {
			select {
			case <-ctx.Done():
				return
			case update := <-c.updates:
				handler(update)
			}
		}
	}()
	go c.runUserStream(ctx)
	return nil
}

func (c *BingXClient) GetAccountState(ctx context.Context) (domain.AccountState, error) {
	balance, err := c.GetBalance(ctx)
	if err != nil {
		return domain.AccountState{}, err
	}
	positions, err := c.GetPositions(ctx)
	if err != nil {
		return domain.AccountState{}, err
	}
	account := balance
	var usedMargin float64
	var unrealized float64
	var leverage float64
	for _, position := range positions {
		usedMargin += abs(position.Size * position.AvgPrice)
	}
	account.UsedMargin = usedMargin
	account.UnrealizedPnL = unrealized
	account.Leverage = leverage
	account.UpdatedAt = time.Now().UTC()
	return account, nil
}

func (c *BingXClient) doSigned(
	ctx context.Context,
	method string,
	path string,
	params map[string]string,
	body any,
	out any,
) error {
	if params == nil {
		params = map[string]string{}
	}
	params["timestamp"] = strconv.FormatInt(time.Now().UTC().UnixMilli()+c.timeOffsetMs, 10)
	params["recvWindow"] = "5000"
	params["signature"] = sign(c.secretKey, params)
	query := canonicalQuery(params)
	endpoint := fmt.Sprintf("%s%s?%s&signature=%s", c.baseURL, path, query, url.QueryEscape(params["signature"]))

	var bodyBytes []byte
	if body != nil {
		raw, err := json.Marshal(body)
		if err != nil {
			return err
		}
		bodyBytes = raw
	}

	var lastErr error
	for attempt := 0; attempt < 4; attempt++ {
		var payload io.Reader
		if len(bodyBytes) > 0 {
			payload = bytes.NewReader(bodyBytes)
		}
		req, err := http.NewRequestWithContext(ctx, method, endpoint, payload)
		if err != nil {
			return err
		}
		req.Header.Set("X-BX-APIKEY", c.apiKey)
		req.Header.Set("Content-Type", "application/json")
		resp, err := c.http.Do(req)
		if err != nil {
			lastErr = err
			if retryable(err, 0, nil) {
				time.Sleep(backoff(attempt))
				continue
			}
			return err
		}
		data, readErr := io.ReadAll(resp.Body)
		resp.Body.Close()
		if readErr != nil {
			return readErr
		}
		if retryable(nil, resp.StatusCode, data) {
			lastErr = errors.New(string(data))
			time.Sleep(backoff(attempt))
			continue
		}
		if resp.StatusCode >= http.StatusBadRequest {
			return errors.New(string(data))
		}
		if out == nil || len(data) == 0 {
			return nil
		}
		return json.Unmarshal(data, out)
	}
	return lastErr
}

func retryable(err error, status int, body []byte) bool {
	if err != nil {
		return true
	}
	if status == http.StatusTooManyRequests || status >= http.StatusInternalServerError {
		return true
	}
	lower := strings.ToLower(string(body))
	return strings.Contains(lower, "timestamp") || strings.Contains(lower, "signature")
}

func backoff(attempt int) time.Duration {
	if attempt < 0 {
		attempt = 0
	}
	return time.Duration(100*(1<<attempt)) * time.Millisecond
}

func abs(v float64) float64 {
	if v < 0 {
		return -v
	}
	return v
}
