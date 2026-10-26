/**
 * Browser-side helpers for the polyglot storefront.
 *
 * The front end never computes prices itself; it asks the Python API in
 * `services/api/` for the catalogue (see `fetchProducts`) and only formats
 * what comes back. No bundler, no framework, no dependencies: this file runs
 * as-is in a browser or in Node.
 */

/** Where the Python API listens when started with `python3 services/api/app.py`. */
export const API_BASE = "http://127.0.0.1:8000";

/**
 * Format an integer number of cents as a US-dollar string with thousands
 * separators, e.g. `123456` -> `"$1,234.56"`.
 *
 * @param {number} cents
 * @returns {string}
 */
export function formatPrice(cents) {
  if (!Number.isInteger(cents)) {
    throw new TypeError(`cents must be an integer, got ${typeof cents}`);
  }
  const sign = cents < 0 ? "-" : "";
  const abs = Math.abs(cents);
  const dollars = Math.trunc(abs / 100);
  const remainder = abs % 100;
  const grouped = String(dollars).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  return `${sign}$${grouped}.${String(remainder).padStart(2, "0")}`;
}

/**
 * Snake_case twin of `formatPrice`, kept so the front end can mirror the
 * Python API's naming (`services/api/app.py` also defines `format_price`).
 *
 * The two `format_price` functions are NOT equivalent: this one inserts
 * thousands separators (`"$1,234.56"`), the Python one does not
 * (`"$1234.56"`). See docs/ARCHITECTURE.md.
 *
 * @param {number} cents
 * @returns {string}
 */
export function format_price(cents) {
  return formatPrice(cents);
}

/**
 * Load the catalogue from the API.
 *
 * Issues `GET {apiBase}/products` and expects the JSON body
 *
 *     { "products": [ { "id", "name", "price_cents", "price" }, ... ] }
 *
 * exactly as produced by `handle_request("/products")` in
 * `services/api/app.py`. Resolves to the `products` array.
 *
 * @param {string} [apiBase] origin of the API, without a trailing slash
 * @param {typeof fetch} [fetchImpl] injectable for tests
 * @returns {Promise<Array<{id: number, name: string, price_cents: number, price: string}>>}
 */
export async function fetchProducts(apiBase = API_BASE, fetchImpl = globalThis.fetch) {
  if (typeof fetchImpl !== "function") {
    throw new TypeError("no fetch implementation available");
  }
  const url = `${apiBase.replace(/\/+$/, "")}/products`;
  const response = await fetchImpl(url, {
    method: "GET",
    headers: { Accept: "application/json" },
  });
  if (!response.ok) {
    throw new Error(`GET ${url} failed with status ${response.status}`);
  }
  const body = await response.json();
  if (!body || !Array.isArray(body.products)) {
    throw new Error(`GET ${url} returned an unexpected body`);
  }
  return body.products;
}

/**
 * Render products as a plain HTML list. Prices are re-formatted on the
 * client from `price_cents` so the thousands separators are consistent
 * with the rest of the page, regardless of what the API's `price` says.
 *
 * @param {Array<{id: number, name: string, price_cents: number}>} products
 * @returns {string}
 */
export function renderProductList(products) {
  const items = products
    .map((p) => `<li data-id="${p.id}">${escapeHtml(p.name)} &mdash; ${formatPrice(p.price_cents)}</li>`)
    .join("");
  return `<ul class="products">${items}</ul>`;
}

function escapeHtml(text) {
  return String(text)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
