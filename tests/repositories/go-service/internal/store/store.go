// Package store provides a tiny thread-safe in-memory key/value store.
package store

import (
	"errors"
	"sync"
)

// ErrNotFound is returned when a key does not exist.
var ErrNotFound = errors.New("key not found")

// Store is an in-memory key/value store guarded by a mutex.
type Store struct {
	mu    sync.RWMutex
	items map[string]string
}

// New returns an empty Store.
func New() *Store {
	return &Store{items: make(map[string]string)}
}

// Get returns the value for key or ErrNotFound.
func (s *Store) Get(key string) (string, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	value, ok := s.items[key]
	if !ok {
		return "", ErrNotFound
	}
	return value, nil
}

// Put stores value under key, replacing any previous value.
func (s *Store) Put(key, value string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.items[key] = value
}

// Len returns the number of stored keys.
func (s *Store) Len() int {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return len(s.items)
}
