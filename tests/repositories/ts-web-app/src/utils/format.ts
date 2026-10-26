/**
 * Pure formatting helpers. This file is the typed source of truth; keep
 * `format.mjs` in sync because that is what the Node test runner executes.
 */

export function formatCurrency(cents: number, currency = "USD"): string {
  if (!Number.isFinite(cents)) {
    throw new TypeError("formatCurrency expects a finite number of cents");
  }
  const amount = cents / 100;
  return new Intl.NumberFormat("en-US", { style: "currency", currency }).format(amount);
}

export function formatDate(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    throw new TypeError(`Invalid date: ${iso}`);
  }
  return date.toISOString().slice(0, 10);
}

export function truncate(text: string, maxLength: number, ellipsis = "…"): string {
  if (maxLength < 0) {
    throw new RangeError("maxLength must be non-negative");
  }
  if (text.length <= maxLength) {
    return text;
  }
  return text.slice(0, Math.max(0, maxLength - ellipsis.length)) + ellipsis;
}

export function pluralize(count: number, singular: string, plural = `${singular}s`): string {
  return `${count} ${count === 1 ? singular : plural}`;
}
