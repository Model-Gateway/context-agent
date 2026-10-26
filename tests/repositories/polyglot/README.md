# polyglot

A tiny monorepo mixing three languages, used as a test fixture for Context
Agent. Nothing here has third-party dependencies.

## Layout

- `services/api/` – Python product-catalogue API (`app.py`) with `unittest`
  tests (`test_app.py`). Exposes `format_price`, `list_products` and a
  `handle_request(path)` dispatcher; `python3 services/api/app.py` serves it
  over HTTP on port 8000.
- `web/`          – JavaScript front end (`index.mjs`) with `node --test`
  tests. `fetchProducts()` calls `GET /products` on the API.
- `tools/`        – dependency-free Rust binary crate that prints the
  repo-wide version string (`tools version`).
- `scripts/build.sh` – runs the test suites (`--all` adds Rust).
- `docs/ARCHITECTURE.md` – how the pieces talk to each other and the JSON
  shape on the wire.
- `Makefile`      – `make test` (Python + JS) and `make clean`.

## Running the tests

```sh
make test              # python3 -m unittest ... && node --test web/*.test.mjs
make test-rust         # cargo test for tools/ (needs a Rust toolchain)
scripts/build.sh       # same as make test
scripts/build.sh --all # everything, including Rust
make clean             # drop __pycache__, tools/target, tools/Cargo.lock
```

## Notes for fixture consumers

- `format_price` is defined twice: once in `services/api/app.py` (Python, no
  thousands separators) and once in `web/index.mjs` (JavaScript, groups
  thousands). They are not interchangeable; see `docs/ARCHITECTURE.md`.
- `web/index.mjs` also exports the camelCase `formatPrice`, which is the
  implementation the snake_case `format_price` delegates to.
- `tools/Cargo.toml` carries an empty `[workspace]` table so Cargo treats it
  as standalone rather than joining the enclosing Context workspace.
