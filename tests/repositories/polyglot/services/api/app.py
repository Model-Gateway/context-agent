"""Product-catalogue API for the polyglot fixture.

There is no web framework here. The public surface is three plain functions:

- ``format_price(cents)``  -> "$12.34"
- ``list_products()``      -> the catalogue as JSON-ready dicts
- ``handle_request(path)`` -> ``(status, body)`` for a GET request

``main()`` wraps ``handle_request`` in the standard-library HTTP server so the
front end in ``web/`` can talk to it over ``GET /products``. Everything runs on
the Python standard library.
"""

import json
from http.server import BaseHTTPRequestHandler, HTTPServer

DEFAULT_PORT = 8000

# The catalogue is static on purpose: this is a fixture, not a shop.
PRODUCTS = [
    {"id": 1, "name": "Notebook", "price_cents": 1234},
    {"id": 2, "name": "Pen", "price_cents": 199},
    {"id": 3, "name": "Desk lamp", "price_cents": 4500},
]


def format_price(cents):
    """Format an integer number of cents as a US-dollar string.

    >>> format_price(1234)
    '$12.34'
    >>> format_price(5)
    '$0.05'
    >>> format_price(-250)
    '-$2.50'

    No thousands separators are inserted: ``format_price(123456)`` is
    ``"$1234.56"``. The JavaScript ``format_price`` in ``web/index.mjs`` has
    the same name but *does* group thousands; see docs/ARCHITECTURE.md.
    """
    if isinstance(cents, bool) or not isinstance(cents, int):
        raise TypeError("cents must be an int, got %r" % type(cents).__name__)
    sign = "-" if cents < 0 else ""
    dollars, remainder = divmod(abs(cents), 100)
    return "%s$%d.%02d" % (sign, dollars, remainder)


def list_products():
    """Return every product as a JSON-ready dict with a formatted ``price``."""
    return [
        {
            "id": product["id"],
            "name": product["name"],
            "price_cents": product["price_cents"],
            "price": format_price(product["price_cents"]),
        }
        for product in PRODUCTS
    ]


def get_product(product_id):
    """Return a single product dict, or ``None`` when the id is unknown."""
    for product in list_products():
        if product["id"] == product_id:
            return product
    return None


def handle_request(path):
    """Dispatch a GET request by path.

    Returns ``(status_code, body)`` where ``body`` is a JSON-serialisable
    dict. Supported routes:

    - ``/health``          -> 200 ``{"status": "ok"}``
    - ``/products``        -> 200 ``{"products": [...]}``
    - ``/products/<id>``   -> 200 ``{"product": {...}}`` or 404
    - anything else        -> 404 ``{"error": "not found", "path": path}``
    """
    if path == "/health":
        return 200, {"status": "ok"}
    if path == "/products":
        return 200, {"products": list_products()}
    if path.startswith("/products/"):
        raw_id = path[len("/products/"):]
        if raw_id.isdigit():
            product = get_product(int(raw_id))
            if product is not None:
                return 200, {"product": product}
        return 404, {"error": "not found", "path": path}
    return 404, {"error": "not found", "path": path}


class _Handler(BaseHTTPRequestHandler):
    """Adapts ``handle_request`` to ``http.server``."""

    def do_GET(self):  # noqa: N802 - name mandated by BaseHTTPRequestHandler
        status, body = handle_request(self.path.split("?", 1)[0])
        payload = json.dumps(body).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *_args):
        return  # keep the fixture quiet


def main(port=DEFAULT_PORT):
    """Serve the API on ``port`` until interrupted."""
    server = HTTPServer(("127.0.0.1", port), _Handler)
    print("api listening on http://127.0.0.1:%d" % port)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
