import SwiftUI

/// A GitHub-style contribution chart of one `UsageSeries`: 53 week columns
/// × 7 weekday rows of 10 pt squares ending today, shaded in four steps of
/// `accent` by the chosen metric; month labels above, Mon/Wed/Fri at the
/// left. Hovering a day shows its figures in the caption under the grid.
struct UsageHeatmap: View {
    var series: UsageSeries
    var metric: UsageMetric
    var quotaPerUnit: Double
    /// The caption's resting text shows the series total.
    var showsTotal = true

    @Environment(\.calendar) private var calendar
    @Environment(\.colorScheme) private var colorScheme
    @State private var hovered: Date?
    /// Kept at the trailing edge (today) whenever the visible width changes.
    @State private var position = ScrollPosition(edge: .trailing)

    var body: some View {
        let grid = HeatmapGrid(calendar: calendar, today: Date())
        let maxValue = series.buckets.values.map { metric.value($0, quotaPerUnit: quotaPerUnit) }.max() ?? 0
        VStack(alignment: .leading, spacing: 8) {
            // Narrower than the grid (the profile page beside a wide
            // inspector), it scrolls sideways, starting at today.
            ScrollView(.horizontal, showsIndicators: false) {
                Canvas { context, _ in
                    draw(grid: grid, maxValue: maxValue, in: &context)
                }
                .frame(width: grid.width, height: grid.height)
                .onContinuousHover { phase in
                    switch phase {
                    case .active(let point): hovered = grid.date(at: point)
                    case .ended: hovered = nil
                    }
                }
                .accessibilityElement()
                .accessibilityLabel("Usage chart, \(series.title): \(metric.format(series.total, quotaPerUnit: quotaPerUnit)) over the last year")
            }
            .scrollPosition($position)
            .scrollBounceBehavior(.basedOnSize, axes: .horizontal)
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { _ in
                position.scrollTo(edge: .trailing)
            }
            HStack(spacing: 12) {
                Text(verbatim: caption)
                    .font(Theme.Font.caption)
                    .foregroundStyle(hovered == nil ? Theme.textSecondary : Theme.textPrimary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Spacer(minLength: 8)
                legend
            }
        }
        .animation(nil, value: hovered)
    }

    // MARK: Drawing

    private func draw(grid: HeatmapGrid, maxValue: Double, in context: inout GraphicsContext) {
        for (column, text) in grid.monthLabels() {
            let origin = CGPoint(x: grid.rect(column: column, row: 0).minX, y: 0)
            context.draw(Text(text).font(.system(size: 10)).foregroundStyle(Theme.textTertiary), at: origin, anchor: .topLeading)
        }
        for row in 0..<7 {
            if let label = grid.weekdayLabel(row: row) {
                let rect = grid.rect(column: 0, row: row)
                context.draw(Text(label).font(.system(size: 10)).foregroundStyle(Theme.textTertiary), at: CGPoint(x: 0, y: rect.midY), anchor: .leading)
            }
        }
        for column in 0..<HeatmapGrid.weeks {
            for row in 0..<7 {
                guard let date = grid.date(column: column, row: row) else { continue }
                let rect = grid.rect(column: column, row: row)
                let path = Path(roundedRect: rect, cornerRadius: 2)
                let value = series.buckets[date].map { metric.value($0, quotaPerUnit: quotaPerUnit) } ?? 0
                context.fill(path, with: .color(color(for: value, maxValue: maxValue)))
                if hovered == date {
                    context.stroke(path, with: .color(Theme.textPrimary), lineWidth: 1)
                }
            }
        }
    }

    /// Empty days are `surfaceRaised`; the rest take one of four accent steps
    /// by their share of the busiest day.
    private func color(for value: Double, maxValue: Double) -> Color {
        guard value > 0, maxValue > 0 else { return Theme.surfaceRaised }
        let step = min(4, max(1, Int((value / maxValue * 4).rounded(.up))))
        return Self.level(step)
    }

    static func level(_ step: Int) -> Color {
        switch step {
        case 0: return Theme.surfaceRaised
        case 1: return Theme.accent.opacity(0.3)
        case 2: return Theme.accent.opacity(0.55)
        case 3: return Theme.accent.opacity(0.8)
        default: return Theme.accent
        }
    }

    // MARK: Caption and legend

    private var caption: String {
        if let hovered {
            let day = hovered.formatted(.dateTime.weekday(.abbreviated).month(.abbreviated).day().year())
            guard let totals = series.buckets[hovered], !totals.isEmpty else { return "\(day) · no usage" }
            return "\(day) · \(UsageFormat.usd(Double(totals.quota) / max(quotaPerUnit, 1))) · \(UsageFormat.compact(totals.tokens)) tokens · \(UsageFormat.count(totals.requests, singular: "request", plural: "requests"))"
        }
        guard showsTotal else { return "" }
        let totals = series.total
        if totals.isEmpty { return "No usage in the last year" }
        return "Last 12 months · \(UsageFormat.usd(Double(totals.quota) / max(quotaPerUnit, 1))) · \(UsageFormat.compact(totals.tokens)) tokens · \(UsageFormat.count(totals.requests, singular: "request", plural: "requests"))"
    }

    private var legend: some View {
        HStack(spacing: 3) {
            Text("Less")
                .font(.system(size: 10))
                .foregroundStyle(Theme.textTertiary)
                .padding(.trailing, 2)
            ForEach(0..<5, id: \.self) { step in
                RoundedRectangle(cornerRadius: 2, style: .continuous)
                    .fill(Self.level(step))
                    .frame(width: 10, height: 10)
            }
            Text("More")
                .font(.system(size: 10))
                .foregroundStyle(Theme.textTertiary)
                .padding(.leading, 2)
        }
        .accessibilityHidden(true)
    }
}

/// Geometry of the chart: column 0 is the week 52 weeks before this one,
/// row 0 the calendar's first weekday; days after today are not drawn.
struct HeatmapGrid {
    static let weeks = 53
    static let cell: CGFloat = 10
    static let gap: CGFloat = 3
    static var pitch: CGFloat { cell + gap }
    static let labelWidth: CGFloat = 30
    static let labelHeight: CGFloat = 16

    let calendar: Calendar
    /// Start of today.
    let today: Date
    /// The day drawn at column 0, row 0.
    let start: Date

    init(calendar: Calendar, today: Date) {
        self.calendar = calendar
        let day = calendar.startOfDay(for: today)
        self.today = day
        let weekday = calendar.component(.weekday, from: day)
        let offset = (weekday - calendar.firstWeekday + 7) % 7
        let weekStart = calendar.date(byAdding: .day, value: -offset, to: day) ?? day
        start = calendar.date(byAdding: .day, value: -7 * (Self.weeks - 1), to: weekStart) ?? weekStart
    }

    var width: CGFloat { Self.labelWidth + CGFloat(Self.weeks) * Self.pitch - Self.gap }
    var height: CGFloat { Self.labelHeight + 7 * Self.pitch - Self.gap }

    /// The day of a cell, or nil for a day still to come.
    func date(column: Int, row: Int) -> Date? {
        guard let date = calendar.date(byAdding: .day, value: column * 7 + row, to: start), date <= today else { return nil }
        return date
    }

    func rect(column: Int, row: Int) -> CGRect {
        CGRect(x: Self.labelWidth + CGFloat(column) * Self.pitch, y: Self.labelHeight + CGFloat(row) * Self.pitch, width: Self.cell, height: Self.cell)
    }

    /// The day under a point of the canvas (gaps count as the cell before them).
    func date(at point: CGPoint) -> Date? {
        guard point.x >= Self.labelWidth, point.y >= Self.labelHeight else { return nil }
        let column = Int((point.x - Self.labelWidth) / Self.pitch)
        let row = Int((point.y - Self.labelHeight) / Self.pitch)
        guard (0..<Self.weeks).contains(column), (0..<7).contains(row) else { return nil }
        return date(column: column, row: row)
    }

    /// Mon · Wed · Fri (rows 1, 3, 5 of a Sunday-first week; whichever
    /// weekdays land on those rows for other calendars).
    func weekdayLabel(row: Int) -> String? {
        guard row % 2 == 1 else { return nil }
        let symbols = calendar.shortWeekdaySymbols
        guard symbols.count == 7 else { return nil }
        let index = (calendar.firstWeekday - 1 + row) % 7
        return String(symbols[index].prefix(3))
    }

    /// A label on the first column of each month; the partial month at the
    /// left edge is labelled only when it keeps at least two columns.
    func monthLabels() -> [(column: Int, text: String)] {
        var labels: [(Int, String)] = []
        var previousMonth: Int?
        for column in 0..<Self.weeks {
            guard let first = calendar.date(byAdding: .day, value: column * 7, to: start) else { continue }
            let month = calendar.component(.month, from: first)
            if month != previousMonth {
                if previousMonth != nil || monthSpansAtLeastTwoColumns(from: column) {
                    let symbols = calendar.shortMonthSymbols
                    if symbols.count == 12 {
                        labels.append((column, symbols[month - 1]))
                    }
                }
                previousMonth = month
            }
        }
        // Two labels on neighbouring columns would overlap: drop the later one.
        var result: [(column: Int, text: String)] = []
        for (column, text) in labels {
            if let last = result.last, column - last.column < 3 { continue }
            result.append((column, text))
        }
        return result
    }

    private func monthSpansAtLeastTwoColumns(from column: Int) -> Bool {
        guard let first = calendar.date(byAdding: .day, value: column * 7, to: start),
              let next = calendar.date(byAdding: .day, value: (column + 1) * 7, to: start) else { return false }
        return calendar.component(.month, from: first) == calendar.component(.month, from: next)
    }
}

#Preview("Usage heatmap") {
    var series = UsageSeries(id: "preview", title: "All usage")
    let calendar = Calendar.current
    let today = calendar.startOfDay(for: Date())
    for back in stride(from: 0, to: 365, by: 1) where back % 3 != 1 {
        if let day = calendar.date(byAdding: .day, value: -back, to: today) {
            series.add(GatewayUsageRow(createdAt: Int64(day.timeIntervalSince1970), modelName: "gpt-4.1", project: "context-agent", count: (back % 7) + 1, quota: ((back * 37) % 900 + 50) * 500, tokenUsed: (back * 131) % 40_000 + 500), bucket: day)
        }
    }
    return UsageHeatmap(series: series, metric: .cost, quotaPerUnit: 500_000)
        .padding(24)
        .background(Theme.canvas)
}
