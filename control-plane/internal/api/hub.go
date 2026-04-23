package api

import (
	"context"
	"encoding/json"
	"log/slog"
	"net/http"
	"sync"
	"time"

	"github.com/gorilla/websocket"
)

type Update struct {
	Type string    `json:"type"`
	Time time.Time `json:"time"`
	Data any       `json:"data"`
}

type Hub struct {
	in           <-chan Update
	writeTimeout time.Duration
	mu           sync.RWMutex
	clients      map[*websocket.Conn]struct{}
	upgrader     websocket.Upgrader
}

func NewHub(in <-chan Update, writeTimeout time.Duration) *Hub {
	return &Hub{
		in:           in,
		writeTimeout: writeTimeout,
		clients:      make(map[*websocket.Conn]struct{}),
		upgrader: websocket.Upgrader{
			CheckOrigin: func(r *http.Request) bool { return true },
		},
	}
}

func (h *Hub) Run(ctx context.Context) {
	for {
		select {
		case <-ctx.Done():
			h.closeAll()
			return
		case update := <-h.in:
			h.broadcast(update)
		}
	}
}

func (h *Hub) ServeWS(w http.ResponseWriter, r *http.Request) {
	conn, err := h.upgrader.Upgrade(w, r, nil)
	if err != nil {
		slog.Warn("websocket upgrade failed", "err", err)
		return
	}
	h.mu.Lock()
	h.clients[conn] = struct{}{}
	h.mu.Unlock()

	go func() {
		defer func() {
			h.mu.Lock()
			delete(h.clients, conn)
			h.mu.Unlock()
			_ = conn.Close()
		}()
		for {
			if _, _, err := conn.ReadMessage(); err != nil {
				return
			}
		}
	}()
}

func (h *Hub) broadcast(update Update) {
	payload, err := json.Marshal(update)
	if err != nil {
		return
	}
	h.mu.RLock()
	clients := make([]*websocket.Conn, 0, len(h.clients))
	for client := range h.clients {
		clients = append(clients, client)
	}
	h.mu.RUnlock()

	for _, client := range clients {
		_ = client.SetWriteDeadline(time.Now().Add(h.writeTimeout))
		if err := client.WriteMessage(websocket.TextMessage, payload); err != nil {
			h.mu.Lock()
			delete(h.clients, client)
			h.mu.Unlock()
			_ = client.Close()
		}
	}
}

func (h *Hub) closeAll() {
	h.mu.Lock()
	defer h.mu.Unlock()
	for client := range h.clients {
		_ = client.Close()
		delete(h.clients, client)
	}
}
