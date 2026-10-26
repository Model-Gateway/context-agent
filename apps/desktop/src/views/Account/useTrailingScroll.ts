import { useLayoutEffect, type RefObject } from 'react'

/// The charts' sideways scroller (`ScrollView(.horizontal)` with
/// `ScrollPosition(edge: .trailing)` and `onGeometryChange` in
/// UsageHeatmap.swift / UsageBarChart.swift): narrower than the chart (the
/// profile page beside a wide inspector), it scrolls, and it is kept at the
/// trailing edge (today) on mount and whenever its visible width changes.
export function useTrailingScroll(ref: RefObject<HTMLElement | null>): void {
  useLayoutEffect(() => {
    const element = ref.current
    if (!element) return
    let lastWidth = -1
    const pin = () => {
      const width = element.clientWidth
      if (width === lastWidth) return
      lastWidth = width
      element.scrollLeft = element.scrollWidth
    }
    pin()
    const observer = new ResizeObserver(pin)
    observer.observe(element)
    return () => observer.disconnect()
  }, [ref])
}
