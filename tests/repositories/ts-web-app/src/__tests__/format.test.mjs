import { test } from "node:test";
import assert from "node:assert/strict";
import { formatCurrency, formatDate, truncate, pluralize } from "../utils/format.mjs";

test("formatCurrency renders cents as US dollars", () => {
  assert.equal(formatCurrency(1234), "$12.34");
  assert.equal(formatCurrency(0), "$0.00");
});

test("formatCurrency rejects non-finite input", () => {
  assert.throws(() => formatCurrency(Number.NaN), TypeError);
});

test("formatDate keeps only the calendar date", () => {
  assert.equal(formatDate("2026-10-20T15:04:05Z"), "2026-10-20");
});

test("formatDate rejects garbage", () => {
  assert.throws(() => formatDate("not a date"), TypeError);
});

test("truncate shortens long text with an ellipsis", () => {
  assert.equal(truncate("hello world", 8), "hello w…");
  assert.equal(truncate("short", 10), "short");
});

test("pluralize picks the right form", () => {
  assert.equal(pluralize(1, "project"), "1 project");
  assert.equal(pluralize(3, "project"), "3 projects");
  assert.equal(pluralize(2, "person", "people"), "2 people");
});
