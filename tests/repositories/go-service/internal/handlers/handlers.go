// Package handlers exposes the HTTP API on top of the store.
package handlers

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strings"

	"example.com/goservice/internal/store"
)

// NewRouter builds the service mux. Routes:
//
//	GET /healthz          -> {"status":"ok"}
//	GET /items/{key}      -> {"key":..., "value":...}
//	PUT /items/{key}      -> stores the raw request body as the value
func NewRouter(st *store.Store) *http.ServeMux {
	mux := http.NewServeMux()
	mux.HandleFunc("/healthz", HealthHandler)
	mux.HandleFunc("/items/", ItemsHandler(st))
	return mux
}

// HealthHandler reports liveness.
func HealthHandler(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
}

// ItemsHandler serves GET and PUT for a single key.
func ItemsHandler(st *store.Store) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		key := strings.TrimPrefix(r.URL.Path, "/items/")
		if key == "" {
			writeError(w, http.StatusBadRequest, "missing key")
			return
		}

		switch r.Method {
		case http.MethodGet:
			value, err := st.Get(key)
			if errors.Is(err, store.ErrNotFound) {
				writeError(w, http.StatusNotFound, "item not found")
				return
			}
			writeJSON(w, http.StatusOK, map[string]string{"key": key, "value": value})
		case http.MethodPut:
			body, err := io.ReadAll(r.Body)
			if err != nil {
				writeError(w, http.StatusBadRequest, "unreadable body")
				return
			}
			st.Put(key, string(body))
			writeJSON(w, http.StatusCreated, map[string]string{"key": key, "value": string(body)})
		default:
			writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		}
	}
}

func writeJSON(w http.ResponseWriter, status int, payload any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(payload)
}

func writeError(w http.ResponseWriter, status int, message string) {
	writeJSON(w, status, map[string]string{"error": message})
}
