// Runtime twin of format.ts. Keep the two files in sync.

export function formatCurrency(cents, currency = "USD") {
  if (!Number.isFinite(cents)) {
    throw new TypeError("formatCurrency expects a finite number of cents");
  }
  const amount = cents / 100;
  return new Intl.NumberFormat("en-US", { style: "currency", currency }).format(amount);
}

export function formatDate(iso) {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    throw new TypeError(`Invalid date: ${iso}`);
  }
  return date.toISOString().slice(0, 10);
}

export function truncate(text, maxLength, ellipsis = "…") {
  if (maxLength < 0) {
    throw new RangeError("maxLength must be non-negative");
  }
  if (text.length <= maxLength) {
    return text;
  }
  return text.slice(0, Math.max(0, maxLength - ellipsis.length)) + ellipsis;
}

export function pluralize(count, singular, plural = `${singular}s`) {
  return `${count} ${count === 1 ? singular : plural}`;
}
