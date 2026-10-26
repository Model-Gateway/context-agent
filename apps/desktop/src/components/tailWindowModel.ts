/**
 * Port of the `TailWindow` struct in the macOS app
 * (Components/TailWindow.swift): which part of a long list (a transcript,
 * an agent's tool calls) is loaded into views. The newest `page` items at
 * first, a page more as the user scrolls up, never more than `limit`;
 * loading older items unloads the newest ones, and scrolling back down
 * loads them again. While the window ends at the newest item
 * (`followsTail`) it grows with the list.
 *
 * The unloaded items keep their place as empty space (`spacerAbove`,
 * `spacerBelow`) at the measured average row height, so the scroller
 * stands for the whole list and does not change size as pages load.
 * Indices are positions in the list. Mutable, like the Swift struct held
 * in `@State`; the `TailWindow` component owns one.
 */
export class TailWindowModel {
  page: number
  limit: number
  lower = 0
  upper = 0
  followsTail = true
  /** Average height of a loaded row (spacing included); 0 until measured. */
  rowHeight = 0

  /** The newest `page` of a list of `count` items. */
  constructor(count: number, page = 20, limit = 60) {
    this.page = page
    this.limit = limit
    this.reset(count)
  }

  /** The loaded slice of a list of `count` items, as `[lower, upper)`. */
  range(count: number): { lower: number; upper: number } {
    const upper = this.followsTail ? count : Math.min(this.upper, count)
    return { lower: Math.min(this.lower, upper), upper }
  }

  /** Room for the unloaded items before the window. */
  get spacerAbove(): number {
    return this.lower * this.rowHeight
  }

  /** Room for the unloaded items after the window. */
  spacerBelow(count: number): number {
    return (count - this.range(count).upper) * this.rowHeight
  }

  get span(): number {
    return this.upper - this.lower
  }

  get hasOlder(): boolean {
    return this.lower > 0
  }

  hasNewer(count: number): boolean {
    return !this.followsTail && this.upper < count
  }

  /** The newest page, following the tail. */
  reset(count: number): void {
    this.followsTail = true
    this.upper = count
    this.lower = Math.max(0, count - this.page)
  }

  /**
   * The list changed to `count` items. Removals (a rewind) and bulk loads
   * (history) start over at the tail; items appended while following
   * extend the window.
   */
  sync(count: number): void {
    if (count < this.upper || this.lower > count || (this.followsTail && count - this.upper > this.page)) {
      this.reset(count)
    } else if (this.followsTail) {
      this.upper = count
    }
  }

  /**
   * The loaded rows measured `height` together: adopts the new average
   * when it moved by more than a tenth (rows streaming in leave it alone).
   * Returns whether it changed.
   */
  measure(height: number, rows: number): boolean {
    if (!this.measureWouldChange(height, rows)) return false
    this.rowHeight = height / rows
    return true
  }

  /** Whether `measure(height, rows)` would adopt a new average (without doing it). */
  measureWouldChange(height: number, rows: number): boolean {
    if (rows <= 0 || height <= 0) return false
    const average = height / rows
    return this.rowHeight === 0 || Math.abs(average - this.rowHeight) > this.rowHeight / 10
  }

  loadOlder(): void {
    this.lower = Math.max(0, this.lower - this.page)
  }

  loadNewer(count: number): void {
    this.upper = Math.min(count, this.upper + this.page)
    if (this.upper === count) this.followsTail = true
  }

  get isOverLimit(): boolean {
    return this.span > this.limit
  }

  /** Unloads the newest items past `limit` (after loading older ones). */
  trimNewest(): void {
    if (!this.isOverLimit) return
    this.upper = this.lower + this.limit
    this.followsTail = false
  }

  /**
   * Unloads the oldest `count` items (past `limit` after loading newer
   * ones, or beyond the newest page while following).
   */
  trimOldest(count: number): void {
    this.lower = Math.min(this.upper - 1, this.lower + Math.max(0, count))
  }

  /**
   * Two pages around `index` (a find match, or where the user dragged the
   * scroller to), following the tail when they reach it.
   */
  place(index: number, count: number): void {
    this.lower = Math.max(0, Math.min(index - this.page, count - 2 * this.page))
    this.upper = Math.min(count, this.lower + 2 * this.page)
    this.followsTail = this.upper === count
  }

  /** A page around `index` unless it is loaded already. */
  reveal(index: number, count: number): void {
    const range = this.range(count)
    if (index >= range.lower && index < range.upper) return
    this.place(index, count)
  }

  /** Whether `index` is in the loaded slice of `count` items. */
  contains(index: number, count: number): boolean {
    const range = this.range(count)
    return index >= range.lower && index < range.upper
  }
}
