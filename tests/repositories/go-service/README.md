# go-service

A minimal HTTP key/value service used as a test fixture for Context.
Standard library only.

- `main.go`                       – wires the store and handlers into an `http.Server`.
- `internal/store/store.go`       – thread-safe in-memory `Store`.
- `internal/handlers/handlers.go` – HTTP handlers (`/healthz`, `/items/{key}`).

Run the tests with:

```sh
go test ./...
```
