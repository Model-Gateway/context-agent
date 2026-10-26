package main

import (
	"log"
	"net/http"
	"os"

	"example.com/goservice/internal/handlers"
	"example.com/goservice/internal/store"
)

func main() {
	addr := os.Getenv("ADDR")
	if addr == "" {
		addr = ":8080"
	}

	st := store.New()
	mux := handlers.NewRouter(st)

	log.Printf("listening on %s", addr)
	if err := http.ListenAndServe(addr, mux); err != nil {
		log.Fatal(err)
	}
}
