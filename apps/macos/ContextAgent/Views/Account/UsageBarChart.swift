import SwiftUI

/// The Usage card's chart for the 24h, 7d and 30d ranges: one bar per hour
/// or day of `buckets` (oldest first), as tall as its share of the busiest
/// one by the chosen metric, with hour or date labels underneath. As wide
/// and tall as `UsageHeatmap`, with the same caption: hovering a bar shows
/// its figures, otherwise the series total.
struct UsageBarChart: View {
    var series: UsageSeries
    var range: UsageRange
    var buckets: [Date]
    var metric: UsageMetric
    var quotaPerUnit: Double
    /// The caption's resting text shows the series total.
    var showsTotal = true

    @Environment(\.calendar) private var calendar
    @State private var hovered: Int?
    /// Kept at the trailing edge (today) whenever the visible width changes.
    @State private var position = ScrollPosition(edge: .trailing)

    static var width: CGFloat { HeatmapGrid.labelWidth + CGFloat(HeatmapGrid.weeks) * HeatmapGrid.pitch - HeatmapGrid.gap }
    static var height: CGFloat { HeatmapGrid.labelHeight + 7 * HeatmapGrid.pitch - HeatmapGrid.gap }
    /// Room under the bars for the axis labels.
    private static let axisHeight: CGFloat = 16

    var body: some View {
        let values = buckets.map { series.buckets[$0].map { metric.value($0, quotaPerUnit: quotaPerUnit) } ?? 0 }
        let maxValue = values.max() ?? 0
        VStack(alignment: .leading, spacing: 8) {
            // Scrolls sideways when narrower, like `UsageHeatmap`.
            ScrollView(.horizontal, showsIndicators: false) {
                Canvas { context, _ in
                    draw(values: values, maxValue: maxValue, in: &context)
                }
                .frame(width: Self.width, height: Self.height)
                .onContinuousHover { phase in
                    switch phase {
                    case .active(let point): hovered = index(at: point)
                    case .ended: hovered = nil
                    }
                }
                .accessibilityElement()
                .accessibilityLabel("Usage chart, \(series.title): \(metric.format(series.total, quotaPerUnit: quotaPerUnit)) over the \(range.caption.lowercased())")
            }
            .scrollPosition($position)
            .scrollBounceBehavior(.basedOnSize, axes: .horizontal)
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { _ in
                position.scrollTo(edge: .trailing)
            }
            Text(verbatim: caption)
                .font(Theme.Font.caption)
                .foregroundStyle(hovered == nil ? Theme.textSecondary : Theme.textPrimary)
                .lineLimit(1)
                .truncationMode(.tail)
        }
        .animation(nil, value: hovered)
    }

    // MARK: Geometry

    private var pitch: CGFloat { buckets.isEmpty ? Self.width : Self.width / CGFloat(buckets.count) }
    /// A quarter of the pitch is gap; seven-day bars stop at 40 pt.
    private var barWidth: CGFloat { min(40, max(2, pitch - max(3, (pitch * 0.25).rounded()))) }
    private var plotHeight: CGFloat { Self.height - Self.axisHeight }

    private func barRect(_ index: Int, value: Double, maxValue: Double) -> CGRect {
        let x = CGFloat(index) * pitch + (pitch - barWidth) / 2
        // Empty buckets keep a 2 pt stub so the row of buckets stays visible.
        let height = value > 0 && maxValue > 0 ? max(3, plotHeight * CGFloat(value / maxValue)) : 2
        return CGRect(x: x, y: plotHeight - height, width: barWidth, height: height)
    }

    private func index(at point: CGPoint) -> Int? {
        guard !buckets.isEmpty, point.x >= 0, point.y >= 0, point.y <= Self.height else { return nil }
        let index = Int(point.x / pitch)
        return buckets.indices.contains(index) ? index : nil
    }

    // MARK: Drawing

    private func draw(values: [Double], maxValue: Double, in context: inout GraphicsContext) {
        if let hovered {
            let column = CGRect(x: CGFloat(hovered) * pitch, y: 0, width: pitch, height: plotHeight)
            context.fill(Path(roundedRect: column, cornerRadius: 3), with: .color(Theme.surfaceRaised.opacity(0.6)))
        }
        for (index, value) in values.enumerated() {
            let rect = barRect(index, value: value, maxValue: maxValue)
            let color: Color = value > 0 ? (hovered == nil || hovered == index ? Theme.accent : Theme.accent.opacity(0.55)) : Theme.surfaceRaised
            context.fill(Path(roundedRect: rect, cornerRadius: min(2, barWidth / 2)), with: .color(color))
        }
        for (index, text) in axisLabels() {
            let midX = CGFloat(index) * pitch + pitch / 2
            let anchor: UnitPoint = midX < 20 ? .topLeading : (midX > Self.width - 20 ? .topTrailing : .top)
            let x = anchor == .topLeading ? CGFloat(index) * pitch + (pitch - barWidth) / 2 : (anchor == .topTrailing ? CGFloat(index) * pitch + (pitch + barWidth) / 2 : midX)
            context.draw(Text(text).font(.system(size: 10)).foregroundStyle(Theme.textTertiary), at: CGPoint(x: x, y: plotHeight + 4), anchor: anchor)
        }
    }

    /// Every sixth hour ("12 AM", "6 AM"…), every day ("Mon"), or every
    /// seventh day counting back from today ("Oct 24").
    private func axisLabels() -> [(Int, String)] {
        buckets.enumerated().compactMap { index, date in
            switch range {
            case .day:
                guard calendar.component(.hour, from: date) % 6 == 0 else { return nil }
                return (index, date.formatted(.dateTime.hour()))
            case .week:
                return (index, date.formatted(.dateTime.weekday(.abbreviated)))
            case .month, .all:
                guard (buckets.count - 1 - index) % 7 == 0 else { return nil }
                return (index, date.formatted(.dateTime.month(.abbreviated).day()))
            }
        }
    }

    // MARK: Caption

    private var caption: String {
        if let hovered, buckets.indices.contains(hovered) {
            let date = buckets[hovered]
            let when = range == .day
                ? date.formatted(.dateTime.weekday(.abbreviated).hour().minute())
                : date.formatted(.dateTime.weekday(.abbreviated).month(.abbreviated).day())
            guard let totals = series.buckets[date], !totals.isEmpty else { return "\(when) · no usage" }
            return "\(when) · \(Self.figures(totals, quotaPerUnit: quotaPerUnit))"
        }
        guard showsTotal else { return "" }
        if series.total.isEmpty { return "No usage in the \(range.caption.lowercased())" }
        return "\(range.caption) · \(Self.figures(series.total, quotaPerUnit: quotaPerUnit))"
    }

    static func figures(_ totals: GatewayUsageTotals, quotaPerUnit: Double) -> String {
        "\(UsageFormat.usd(Double(totals.quota) / max(quotaPerUnit, 1))) · \(UsageFormat.compact(totals.tokens)) tokens · \(UsageFormat.count(totals.requests, singular: "request", plural: "requests"))"
    }
}

#Preview("Usage bars") {
    let now = Date()
    let calendar = Calendar.current
    var rows: [GatewayUsageRow] = []
    for back in 0..<(24 * 30) where back % 5 != 2 {
        let stamp = Int64(now.timeIntervalSince1970) - Int64(back) * 3600
        rows.append(GatewayUsageRow(createdAt: stamp, modelName: "gpt-4.1", project: "context-agent", count: back % 4 + 1, quota: ((back * 37) % 900 + 50) * 50, tokenUsed: (back * 131) % 4_000 + 50))
    }
    let report = GatewayUsageReport(startTimestamp: 0, endTimestamp: Int64(now.timeIntervalSince1970), rows: rows)
    return VStack(alignment: .leading, spacing: 24) {
        ForEach([UsageRange.day, .week, .month]) { range in
            let usage = UsageBreakdown(report: report, range: range, now: now, calendar: calendar)
            UsageBarChart(series: usage.total, range: range, buckets: usage.bucketStarts, metric: .cost, quotaPerUnit: 500_000)
        }
    }
    .padding(24)
    .background(Theme.canvas)
}
