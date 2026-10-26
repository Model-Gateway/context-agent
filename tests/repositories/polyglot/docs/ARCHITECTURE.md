# Architecture

Three independently runnable pieces live in this repository. They share no
code, only an HTTP contract and a version number.

```
web/  --- GET /products (JSON) --->  services/api/
                                        ^
tools/  (release helper: owns the repo-wide version string)
```

## services/api (Python)

`services/api/app.py` is a dependency-free product catalogue. The logic is
three plain functions, so it can be tested without starting a server:

| Function                | Purpose                                             |
| ----------------------- | --------------------------------------------------- |
| `format_price(cents)`   | `1234` -> `"$12.34"`; no thousands separators        |
| `list_products()`       | catalogue as JSON-ready dicts, each with a `price`  |
| `handle_request(path)`  | GET dispatcher returning `(status, body)`           |

`python3 services/api/app.py` wraps `handle_request` in `http.server` and
listens on `http://127.0.0.1:8000`. Routes:

| Path              | Status | Body                                        |
| ----------------- | ------ | ------------------------------------------- |
| `/health`         | 200    | `{"status": "ok"}`                          |
| `/products`       | 200    | `{"products": [Product, ...]}`              |
| `/products/<id>`  | 200    | `{"product": Product}` (404 if unknown)     |
| anything else     | 404    | `{"error": "not found", "path": "<path>"}`  |

A `Product` is:

```json
{ "id": 1, "name": "Notebook", "price_cents": 1234, "price": "$12.34" }
```

`price_cents` is the source of truth; `price` is a convenience string
produced by the Python `format_price`.

## web (JavaScript)

`web/index.mjs` is the browser side. It never computes prices itself. On
page load it calls

```
GET {API_BASE}/products        Accept: application/json
```

through `fetchProducts(apiBase)`, which resolves to the `products` array from
the body above and rejects on a non-2xx status or a body without a
`products` array. `renderProductList(products)` then re-formats each
`price_cents` on the client with `formatPrice`.

### Two functions named `format_price`

Both `services/api/app.py` and `web/index.mjs` define a function named
`format_price`. They are different implementations with different output:

| Input    | Python `format_price` | JS `format_price` / `formatPrice` |
| -------- | --------------------- | --------------------------------- |
| `1234`   | `"$12.34"`            | `"$12.34"`                        |
| `123456` | `"$1234.56"`          | `"$1,234.56"`                     |

The JS version groups thousands; the Python version does not. That is why
`renderProductList` formats from `price_cents` instead of trusting the
API's `price` string. When searching the repo for `format_price`, check
which language you landed in.

## tools (Rust)

`tools/` is a tiny dependency-free binary crate. The monorepo has no single
package manifest, so the repo-wide version lives in `tools/src/main.rs`
(`REPO_VERSION`), and `tools version` prints it:

```
$ cargo run --manifest-path tools/Cargo.toml -- version
polyglot 0.1.0
$ cargo run --manifest-path tools/Cargo.toml -- version --json
{"name":"polyglot","version":"0.1.0","tool":"tools 0.1.0"}
```

Release scripts call it to stamp builds. It is the only Rust in the repo and
is deliberately kept out of the default `make test` so the common path needs
no compiler.

## Build and test

- `make test` runs the Python and JavaScript suites.
- `make test-rust` builds and tests `tools/`.
- `scripts/build.sh` does the same from a shell; pass `--all` to include Rust.
- `make clean` removes `__pycache__`, `tools/target` and `tools/Cargo.lock`.
