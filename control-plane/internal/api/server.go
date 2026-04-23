package api

import (
	"context"
	"encoding/json"
	"net/http"
	"time"

	"control-plane/internal/config"
	"control-plane/internal/domain"
	"control-plane/internal/risk"
	"control-plane/internal/state"
)

type Submitter interface {
	Submit(context.Context, domain.ExecutionRequest) (domain.Order, error)
}

type Server struct {
	cfg       config.HTTPConfig
	hub       *Hub
	store     *state.Store
	risk      *risk.Service
	submitter Submitter
}

func NewServer(cfg config.HTTPConfig, hub *Hub, store *state.Store, riskSvc *risk.Service, submitter Submitter) *Server {
	return &Server{cfg: cfg, hub: hub, store: store, risk: riskSvc, submitter: submitter}
}

func (s *Server) Routes() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /health", s.health)
	mux.HandleFunc("GET /status", s.status)
	mux.HandleFunc("GET /positions", s.positions)
	mux.HandleFunc("GET /risk", s.riskStatus)
	mux.HandleFunc("POST /kill-switch", s.killSwitch)
	mux.HandleFunc("POST /execution/requests", s.executionRequest)
	mux.HandleFunc("GET /ws", s.hub.ServeWS)
	return withJSON(mux)
}

func (s *Server) health(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{"ok": true, "time": time.Now().UTC()})
}

func (s *Server) status(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{
		"ok":        true,
		"risk":      s.risk.Status(),
		"positions": s.store.Positions(),
		"orders":    s.store.Orders(),
	})
}

func (s *Server) positions(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, s.store.Positions())
}

func (s *Server) riskStatus(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, s.risk.Status())
}

func (s *Server) killSwitch(w http.ResponseWriter, r *http.Request) {
	var req struct {
		Enabled bool `json:"enabled"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeJSON(w, http.StatusBadRequest, map[string]string{"error": "bad_json"})
		return
	}
	s.risk.SetKillSwitch(req.Enabled)
	writeJSON(w, http.StatusOK, s.risk.Status())
}

func (s *Server) executionRequest(w http.ResponseWriter, r *http.Request) {
	var req domain.ExecutionRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeJSON(w, http.StatusBadRequest, map[string]string{"error": "bad_json"})
		return
	}
	if req.SignalTime.IsZero() {
		req.SignalTime = time.Now().UTC()
	}
	ctx, cancel := context.WithTimeout(r.Context(), 750*time.Millisecond)
	defer cancel()
	order, err := s.submitter.Submit(ctx, req)
	if err != nil {
		writeJSON(w, http.StatusPreconditionFailed, map[string]any{"error": err.Error(), "order": order})
		return
	}
	writeJSON(w, http.StatusAccepted, order)
}

func withJSON(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("X-Content-Type-Options", "nosniff")
		next.ServeHTTP(w, r)
	})
}

func writeJSON(w http.ResponseWriter, status int, value any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(value)
}
