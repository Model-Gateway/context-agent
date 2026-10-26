/**
 * Port of `RenderCache.swift`: a bounded memo table. The Swift version sits
 * on `NSCache` (evicts past `countLimit` and under memory pressure); this one
 * is a least-recently-used `Map` that evicts past `countLimit`. Transcript
 * rows are rebuilt whenever their inputs change or a chat is reopened;
 * memoizing their parsed and rendered markdown keeps that from re-parsing
 * text and re-resolving file references.
 *
 * Keys are compared the way `Map` compares them, so callers build string
 * keys (`root + '\0' + text`) rather than passing objects.
 */
export class RenderCache<K, V> {
  private readonly entries = new Map<K, V>()

  constructor(readonly countLimit: number) {}

  /** The cached value for `key`, or `compute()` stored under it. */
  value(key: K, compute: () => V): V {
    if (this.entries.has(key)) {
      const hit = this.entries.get(key) as V
      // Re-insert so the entry becomes the most recently used.
      this.entries.delete(key)
      this.entries.set(key, hit)
      return hit
    }
    const computed = compute()
    this.set(key, computed)
    return computed
  }

  /** The cached value for `key` without computing one. */
  get(key: K): V | undefined {
    if (!this.entries.has(key)) return undefined
    const hit = this.entries.get(key) as V
    this.entries.delete(key)
    this.entries.set(key, hit)
    return hit
  }

  /** Stores `value` under `key`, evicting the least recently used entry past the limit. */
  set(key: K, value: V): void {
    this.entries.delete(key)
    this.entries.set(key, value)
    while (this.entries.size > this.countLimit) {
      const oldest = this.entries.keys().next()
      if (oldest.done) break
      this.entries.delete(oldest.value)
    }
  }

  /** Forgets every entry (`RenderCache.removeAll()` in Swift). */
  removeAll(): void {
    this.entries.clear()
  }

  get size(): number {
    return this.entries.size
  }
}
