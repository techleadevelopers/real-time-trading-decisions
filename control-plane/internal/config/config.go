package config

import (
	"log/slog"
	"os"
	"strconv"
	"strings"
	"time"
)

type Config struct {
	HTTP                  HTTPConfig
	MarketData            MarketDataConfig
	Pipeline              PipelineConfig
	Risk                  RiskConfig
	Execution             ExecutionConfig
	MarketDataBuffer      int
	UpdateBuffer          int
	WebSocketWriteTimeout time.Duration
	Log                   string
	BingX                 BingXConfig
}

type HTTPConfig struct {
	Addr string
}

type MarketDataConfig struct {
	Symbols          []string
	ReconnectBackoff time.Duration
	SendTimeout      time.Duration
}

type PipelineConfig struct {
	StaleAfter time.Duration
}

type RiskConfig struct {
	MaxExposureUSD     float64
	MaxPositionUSD     float64
	MaxDailyLossUSD    float64
	MaxSignalAge       time.Duration
	LatencyRejectAfter time.Duration
}

type ExecutionConfig struct {
	IdempotencyTTL time.Duration
}

type BingXConfig struct {
	APIKey string
	SecretKey string
	BaseURL string
	WSURL string
	Enabled bool
}

func Load() Config {
	return Config{
		HTTP: HTTPConfig{Addr: env("CONTROL_PLANE_ADDR", ":8088")},
		MarketData: MarketDataConfig{
			Symbols:          splitSymbols(env("CONTROL_PLANE_SYMBOLS", "btcusdt")),
			ReconnectBackoff: durationEnv("MD_RECONNECT_BACKOFF", time.Second),
			SendTimeout:      durationEnv("MD_SEND_TIMEOUT", 5*time.Millisecond),
		},
		Pipeline: PipelineConfig{StaleAfter: durationEnv("PIPELINE_STALE_AFTER", 1500*time.Millisecond)},
		Risk: RiskConfig{
			MaxExposureUSD:     floatEnv("RISK_MAX_EXPOSURE_USD", 10_000),
			MaxPositionUSD:     floatEnv("RISK_MAX_POSITION_USD", 2_500),
			MaxDailyLossUSD:    floatEnv("RISK_MAX_DAILY_LOSS_USD", 250),
			MaxSignalAge:       durationEnv("RISK_MAX_SIGNAL_AGE", 500*time.Millisecond),
			LatencyRejectAfter: durationEnv("RISK_LATENCY_REJECT_AFTER", 150*time.Millisecond),
		},
		Execution:             ExecutionConfig{IdempotencyTTL: durationEnv("EXEC_IDEMPOTENCY_TTL", 24*time.Hour)},
		BingX: BingXConfig{
			APIKey: env("BINGX_API_KEY", ""),
			SecretKey: env("BINGX_SECRET_KEY", ""),
			BaseURL: env("BINGX_BASE_URL", "https://open-api.bingx.com"),
			WSURL: env("BINGX_WS_URL", "wss://open-api-swap.bingx.com"),
			Enabled: strings.EqualFold(env("EXECUTION_EXCHANGE", "paper"), "bingx"),
		},
		MarketDataBuffer:      intEnv("MD_BUFFER", 8192),
		UpdateBuffer:          intEnv("UPDATE_BUFFER", 8192),
		WebSocketWriteTimeout: durationEnv("WS_WRITE_TIMEOUT", 2*time.Second),
		Log:                   env("LOG_LEVEL", "info"),
	}
}

func (c Config) LogLevel() slog.Level {
	switch strings.ToLower(c.Log) {
	case "debug":
		return slog.LevelDebug
	case "warn":
		return slog.LevelWarn
	case "error":
		return slog.LevelError
	default:
		return slog.LevelInfo
	}
}

func env(key, fallback string) string {
	if value := os.Getenv(key); value != "" {
		return value
	}
	return fallback
}

func intEnv(key string, fallback int) int {
	value, err := strconv.Atoi(env(key, ""))
	if err != nil {
		return fallback
	}
	return value
}

func floatEnv(key string, fallback float64) float64 {
	value, err := strconv.ParseFloat(env(key, ""), 64)
	if err != nil {
		return fallback
	}
	return value
}

func durationEnv(key string, fallback time.Duration) time.Duration {
	value := env(key, "")
	if value == "" {
		return fallback
	}
	parsed, err := time.ParseDuration(value)
	if err != nil {
		return fallback
	}
	return parsed
}

func splitSymbols(raw string) []string {
	parts := strings.Split(raw, ",")
	out := make([]string, 0, len(parts))
	for _, part := range parts {
		part = strings.ToLower(strings.TrimSpace(part))
		if part != "" {
			out = append(out, part)
		}
	}
	return out
}
