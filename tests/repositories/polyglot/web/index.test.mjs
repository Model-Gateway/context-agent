import test from "node:test";
import assert from "node:assert/strict";

import {
  API_BASE,
  fetchProducts,
  formatPrice,
  format_price,
  renderProductList,
} from "./index.mjs";

test("formatPrice formats whole and fractional dollars", () => {
  assert.equal(formatPrice(1234), "$12.34");
  assert.equal(formatPrice(100), "$1.00");
  assert.equal(formatPrice(5), "$0.05");
  assert.equal(formatPrice(0), "$0.00");
});

test("formatPrice groups thousands", () => {
  assert.equal(formatPrice(123456), "$1,234.56");
  assert.equal(formatPrice(100000000), "$1,000,000.00");
});

test("formatPrice keeps the sign before the symbol", () => {
  assert.equal(formatPrice(-250), "-$2.50");
});

test("formatPrice rejects non-integers", () => {
  assert.throws(() => formatPrice(12.34), TypeError);
  assert.throws(() => formatPrice("1234"), TypeError);
});

test("format_price is the snake_case twin of formatPrice", () => {
  assert.equal(format_price(1234), formatPrice(1234));
  // Differs from the Python format_price, which yields "$1234.56".
  assert.equal(format_price(123456), "$1,234.56");
});

test("fetchProducts issues GET /products and unwraps the products array", async () => {
  const calls = [];
  const fakeFetch = async (url, init) => {
    calls.push({ url, init });
    return {
      ok: true,
      status: 200,
      json: async () => ({
        products: [{ id: 1, name: "Notebook", price_cents: 1234, price: "$12.34" }],
      }),
    };
  };

  const products = await fetchProducts("http://api.test/", fakeFetch);

  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, "http://api.test/products");
  assert.equal(calls[0].init.method, "GET");
  assert.deepEqual(products, [
    { id: 1, name: "Notebook", price_cents: 1234, price: "$12.34" },
  ]);
});

test("fetchProducts defaults to API_BASE", async () => {
  let seen;
  await fetchProducts(undefined, async (url) => {
    seen = url;
    return { ok: true, status: 200, json: async () => ({ products: [] }) };
  });
  assert.equal(seen, `${API_BASE}/products`);
});

test("fetchProducts rejects on a non-2xx status", async () => {
  const fakeFetch = async () => ({ ok: false, status: 503, json: async () => ({}) });
  await assert.rejects(() => fetchProducts("http://api.test", fakeFetch), /status 503/);
});

test("fetchProducts rejects on an unexpected body", async () => {
  const fakeFetch = async () => ({ ok: true, status: 200, json: async () => ({ nope: [] }) });
  await assert.rejects(() => fetchProducts("http://api.test", fakeFetch), /unexpected body/);
});

test("renderProductList escapes names and re-formats prices", () => {
  const html = renderProductList([
    { id: 7, name: "<Lamp> & \"Shade\"", price_cents: 123456 },
  ]);
  assert.equal(
    html,
    '<ul class="products"><li data-id="7">&lt;Lamp&gt; &amp; &quot;Shade&quot; &mdash; $1,234.56</li></ul>',
  );
});
