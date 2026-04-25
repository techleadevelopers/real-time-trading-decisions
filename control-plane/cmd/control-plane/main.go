package main

import (
	"context"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"control-plane/internal/api"
	"control-plane/internal/config"
	"control-plane/internal/execution"
	"control-plane/internal/execution/bingx"
	"control-plane/internal/marketdata"
	"control-plane/internal/pipeline"
	"control-plane/internal/risk"
	"control-plane/internal/state"
)

func main() {
	cfg := config.Load()
	logger := slog.New(slog.NewJSONHandler(os.Stdout, &slog.HandlerOptions{Level: cfg.LogLevel()}))
	slog.SetDefault(logger)

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	events := make(chan marketdata.Event, cfg.MarketDataBuffer)
	updates := make(chan api.Update, cfg.UpdateBuffer)

	store := state.NewStore()
	riskSvc := risk.NewService(cfg.Risk, store)
	hub := api.NewHub(updates, cfg.WebSocketWriteTimeout)
	var exchange execution.ExchangeClient = execution.NewPaperExchange(store, updates)
	if cfg.BingX.Enabled {
		client := bingx.New(cfg.BingX.APIKey, cfg.BingX.SecretKey, cfg.BingX.BaseURL, cfg.BingX.WSURL)
		report, account, err := client.Reconcile(ctx, store)
		if err != nil {
			logger.Error("bingx reconciliation failed", "err", err)
			os.Exit(1)
		}
		store.SetAccountState(account)
		if !report.Matched {
			logger.Warn("bingx reconciliation detected mismatches", "details", report.Details)
		}
		exchange = client
	}
	execGateway := execution.NewGateway(store, riskSvc, exchange, updates, cfg.Execution)
	if asyncExchange, ok := exchange.(execution.AsyncExchangeClient); ok {
		if err := asyncExchange.Start(ctx, func(update execution.AsyncExchangeUpdate) {
			if err := execGateway.ApplyAsyncUpdate(update); err != nil {
				slog.Warn("async exchange update failed", "err", err)
			}
			if account, err := asyncExchange.GetAccountState(context.Background()); err == nil {
				store.SetAccountState(account)
			}
		}); err != nil {
			logger.Error("failed to start exchange stream", "err", err)
			os.Exit(1)
		}
	}

	pipe := pipeline.New(events, updates, store, riskSvc, cfg.Pipeline)
	md := marketdata.NewBinanceGateway(cfg.MarketData, events)
	server := api.NewServer(cfg.HTTP, hub, store, riskSvc, execGateway)

	go hub.Run(ctx)
	go pipe.Run(ctx)
	go md.Run(ctx)

	httpServer := &http.Server{
		Addr:              cfg.HTTP.Addr,
		Handler:           server.Routes(),
		ReadHeaderTimeout: 3 * time.Second,
		ReadTimeout:       5 * time.Second,
		WriteTimeout:      10 * time.Second,
		IdleTimeout:       60 * time.Second,
	}

	go func() {
		logger.Info("control-plane listening", "addr", cfg.HTTP.Addr)
		if err := httpServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			logger.Error("http server failed", "err", err)
			stop()
		}
	}()

	<-ctx.Done()
	shutdownCtx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
	defer cancel()
	_ = httpServer.Shutdown(shutdownCtx)
	logger.Info("control-plane stopped")
}
