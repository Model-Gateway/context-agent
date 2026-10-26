import SwiftUI
import AppKit

/// Unified diff renderer: a file header per `diff --git` block, `@@` hunk
/// headers in `textTertiary`, additions tinted `success`, deletions `danger`,
/// monospace old/new line numbers in the gutter. The lines live in one
/// read-only `NSTextView` (`DiffBody`); hunks beyond the first 20 start
/// collapsed ("Show hunk"), and a banner says so when the backend truncated
/// the diff.
struct DiffView: View {
    /// Hunks from this index on start collapsed.
    static let expandedHunkLimit = 20
    /// Diffs up to this many bytes are parsed synchronously in `init`
    /// (no blank frame); larger ones parse off the main actor.
    private static let synchronousParseLimit = 64 * 1024

    private let diff: String
    private let truncated: Bool
    private let showFileHeaders: Bool
    private let initialModel: ParsedDiff?

    @State private var parsed: ParsedDiff?
    @State private var parsedSource: String?
    @State private var toggledHunks: Set<Int> = []

    /// `showFileHeaders: false` when the caller already names the file (the
    /// Changes pane header), so the path is not shown twice.
    init(diff: String, truncated: Bool = false, showFileHeaders: Bool = true) {
        self.diff = diff
        self.truncated = truncated
        self.showFileHeaders = showFileHeaders
        self.initialModel = diff.utf8.count <= Self.synchronousParseLimit ? ParsedDiff.parse(diff) : nil
    }

    var body: some View {
        VStack(spacing: 0) {
            if truncated {
                TruncatedBanner()
            }
            if let model = currentModel {
                if model.isEmpty {
                    EmptyPlaceholder(systemImage: Icons.changes, title: "No differences", subtitle: "The diff is empty.")
                } else {
                    DiffBody(model: model, source: diff, showFileHeaders: showFileHeaders, toggledHunks: $toggledHunks)
                }
            } else {
                loading
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.surface)
        .task(id: diff) { await parseIfNeeded() }
    }

    private var currentModel: ParsedDiff? {
        if let parsed, parsedSource == diff { return parsed }
        return initialModel
    }

    private var loading: some View {
        VStack(spacing: 10) {
            BrailleSpinner(size: 16)
            Text("Rendering diff…")
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textSecondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func parseIfNeeded() async {
        toggledHunks = []
        if initialModel != nil { return }
        let source = diff
        let model = await Task.detached(priority: .userInitiated) { ParsedDiff.parse(source) }.value
        guard !Task.isCancelled else { return }
        parsed = model
        parsedSource = source
    }
}

// MARK: - Body

/// The parsed diff in a read-only `NSTextView`: one text storage for the
/// whole diff (selection and ⌘F come for free, scrolling is AppKit's), a
/// ruler with the old / new line numbers and the +/− marker, and full-width
/// row tints drawn by the layout manager. A SwiftUI row per diff line made
/// a few thousand selectable text views; AppKit lays out only what is
/// visible. Clicking a `@@` header toggles its hunk.
private struct DiffBody: NSViewRepresentable {
    let model: ParsedDiff
    /// The diff text `model` was parsed from; identifies the model cheaply.
    let source: String
    let showFileHeaders: Bool
    @Binding var toggledHunks: Set<Int>

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeNSView(context: Context) -> DiffContainerView {
        let storage = NSTextStorage()
        let layoutManager = DiffLayoutManager()
        storage.addLayoutManager(layoutManager)
        let container = NSTextContainer(size: NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude))
        container.widthTracksTextView = false
        container.lineFragmentPadding = 6
        layoutManager.addTextContainer(container)

        let textView = DiffTextView(frame: .zero, textContainer: container)
        textView.isEditable = false
        textView.isSelectable = true
        // Rich text: plain-text mode replaces every paragraph style with
        // the default one and loses the spacing around file headers.
        textView.isRichText = true
        textView.usesFindBar = true
        textView.isIncrementalSearchingEnabled = true
        textView.drawsBackground = false
        textView.font = Theme.Font.nsMono
        textView.textColor = Theme.nsTextPrimary
        textView.textContainerInset = NSSize(width: 0, height: 6)
        textView.isHorizontallyResizable = true
        textView.isVerticallyResizable = true
        textView.autoresizingMask = [.width]
        textView.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        textView.minSize = .zero

        let scrollView = NSScrollView()
        scrollView.documentView = textView
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.borderType = .noBorder
        scrollView.drawsBackground = false

        // Not an `NSRulerView`: on macOS 26 the scroll view lays a ruler
        // over the clip view instead of beside it. The gutter is a sibling
        // over the scroll view's leading edge; the text is indented past it.
        let gutter = DiffGutterView(textView: textView)
        let host = DiffContainerView(scrollView: scrollView, gutter: gutter)

        context.coordinator.attach(textView: textView, layoutManager: layoutManager, gutter: gutter, container: host)
        apply(context.coordinator)
        return host
    }

    func updateNSView(_ container: DiffContainerView, context: Context) {
        apply(context.coordinator)
    }

    @MainActor
    private func apply(_ coordinator: Coordinator) {
        coordinator.onToggleHunk = { id in
            if toggledHunks.contains(id) { toggledHunks.remove(id) } else { toggledHunks.insert(id) }
        }
        let key = Coordinator.Key(showFileHeaders: showFileHeaders, toggled: toggledHunks)
        guard coordinator.applied != key || coordinator.appliedSource != source else { return }
        coordinator.applied = key
        coordinator.appliedSource = source
        let digits = max(2, String(model.maxLineNumber).count)
        let gutterWidth = DiffGutterView.width(digits: digits)
        let document = DiffDocument.build(model, showFileHeaders: showFileHeaders) { id in
            (id < DiffView.expandedHunkLimit) != toggledHunks.contains(id)
        }
        coordinator.show(document, digits: digits, gutterWidth: gutterWidth)
    }

    @MainActor
    final class Coordinator {
        /// One line fragment per row, in the text view's coordinates, so the
        /// gutter draws from a table instead of asking the layout manager
        /// (layout forced from another view's draw pass left the text view
        /// undrawn).
        static func fragments(of textView: NSTextView, rows: DiffRows) -> [DiffGutterView.Fragment] {
            guard let layoutManager = textView.layoutManager, let container = textView.textContainer else { return [] }
            layoutManager.ensureLayout(for: container)
            let inset = textView.textContainerInset.height
            var fragments: [DiffGutterView.Fragment] = []
            fragments.reserveCapacity(rows.rows.count)
            layoutManager.enumerateLineFragments(forGlyphRange: NSRange(location: 0, length: layoutManager.numberOfGlyphs)) { rect, _, _, glyphRange, _ in
                let character = layoutManager.characterIndexForGlyph(at: glyphRange.location)
                guard let index = rows.index(containing: character) else { return }
                fragments.append(DiffGutterView.Fragment(minY: rect.minY + inset, height: rect.height, row: index))
            }
            return fragments
        }

        struct Key: Equatable {
            var showFileHeaders: Bool
            var toggled: Set<Int>
        }

        var applied: Key?
        var appliedSource: String?
        var onToggleHunk: ((Int) -> Void)?
        weak var textView: DiffTextView?
        weak var layoutManager: DiffLayoutManager?
        weak var gutter: DiffGutterView?
        weak var container: DiffContainerView?
        private var observer: NSObjectProtocol?

        func attach(textView: DiffTextView, layoutManager: DiffLayoutManager, gutter: DiffGutterView, container: DiffContainerView) {
            self.textView = textView
            self.layoutManager = layoutManager
            self.gutter = gutter
            self.container = container
            textView.onToggleHunk = { [weak self] id in self?.onToggleHunk?(id) }
            // The gutter follows the clip view: redraw on scroll.
            let clipView = container.scrollView.contentView
            clipView.postsBoundsChangedNotifications = true
            observer = NotificationCenter.default.addObserver(forName: NSView.boundsDidChangeNotification, object: clipView, queue: .main) { [weak gutter] _ in
                gutter?.needsDisplay = true
            }
        }

        deinit {
            if let observer { NotificationCenter.default.removeObserver(observer) }
        }

        func show(_ document: DiffDocument, digits: Int, gutterWidth: CGFloat) {
            guard let textView, let storage = textView.textStorage else { return }
            let rows = DiffRows(rows: document.rows, lineStarts: document.lineStarts)
            layoutManager?.rows = rows
            textView.rows = rows
            // Lines start past the gutter (the padding applies at both ends
            // of a line; the trailing share only widens the scroll range).
            textView.textContainer?.lineFragmentPadding = gutterWidth + 6
            storage.setAttributedString(document.text)
            textView.needsDisplay = true
            gutter?.update(rows: rows, fragments: Self.fragments(of: textView, rows: rows), digits: digits)
            container?.needsLayout = true
        }
    }
}

// MARK: - Document

/// One paragraph of the diff text and how to draw its gutter and background.
private struct DiffRow {
    enum Kind {
        case fileHeader
        case note
        case hunkHeader(id: Int)
        case line(ParsedDiff.LineKind)
    }

    var kind: Kind
    var oldNumber: Int?
    var newNumber: Int?
    var marker: String

    /// Full-width tint behind the row; nil for plain rows.
    var background: NSColor? {
        switch kind {
        case .hunkHeader: return Theme.nsSurfaceRaised
        case .line(.addition): return Theme.nsSuccess.withAlphaComponent(0.09)
        case .line(.deletion): return Theme.nsDanger.withAlphaComponent(0.09)
        case .fileHeader, .note, .line: return nil
        }
    }

    var markerColor: NSColor {
        switch kind {
        case .line(.addition): return Theme.nsSuccess
        case .line(.deletion): return Theme.nsDanger
        default: return Theme.nsTextTertiary
        }
    }
}

/// The rows with the UTF-16 offset each one starts at, shared by the text
/// view (click → hunk), the layout manager (tints) and the ruler (numbers).
private final class DiffRows {
    let rows: [DiffRow]
    let lineStarts: [Int]

    init(rows: [DiffRow], lineStarts: [Int]) {
        self.rows = rows
        self.lineStarts = lineStarts
    }

    static let empty = DiffRows(rows: [], lineStarts: [])

    /// Index of the row containing the UTF-16 offset.
    func index(containing character: Int) -> Int? {
        guard !lineStarts.isEmpty else { return nil }
        var low = 0, high = lineStarts.count - 1
        while low < high {
            let mid = (low + high + 1) / 2
            if lineStarts[mid] <= character { low = mid } else { high = mid - 1 }
        }
        return low
    }
}

private struct DiffDocument {
    var text: NSAttributedString
    var rows: [DiffRow]
    var lineStarts: [Int]

    static func build(_ model: ParsedDiff, showFileHeaders: Bool, isExpanded: (Int) -> Bool) -> DiffDocument {
        let mono = Theme.Font.nsMono
        let headerFont = NSFont.systemFont(ofSize: 13, weight: .medium)
        let captionFont = NSFont.systemFont(ofSize: 12)
        let monoBold = NSFont.monospacedSystemFont(ofSize: 12, weight: .medium)

        let lineStyle = NSMutableParagraphStyle()
        let plain: [NSAttributedString.Key: Any] = [.font: mono, .foregroundColor: Theme.nsTextPrimary, .paragraphStyle: lineStyle]
        let added: [NSAttributedString.Key: Any] = [.font: mono, .foregroundColor: Theme.nsSuccess, .paragraphStyle: lineStyle]
        let removed: [NSAttributedString.Key: Any] = [.font: mono, .foregroundColor: Theme.nsDanger, .paragraphStyle: lineStyle]
        let muted: [NSAttributedString.Key: Any] = [.font: mono, .foregroundColor: Theme.nsTextTertiary, .paragraphStyle: lineStyle]
        let headerStyle = NSMutableParagraphStyle()
        headerStyle.paragraphSpacingBefore = 10
        headerStyle.paragraphSpacing = 6
        let header: [NSAttributedString.Key: Any] = [.font: headerFont, .foregroundColor: Theme.nsTextPrimary, .paragraphStyle: headerStyle]
        let headerMuted: [NSAttributedString.Key: Any] = [.font: captionFont, .foregroundColor: Theme.nsTextSecondary, .paragraphStyle: headerStyle]
        let note: [NSAttributedString.Key: Any] = [.font: captionFont, .foregroundColor: Theme.nsTextTertiary, .paragraphStyle: lineStyle]
        let hunkStyle = NSMutableParagraphStyle()
        hunkStyle.paragraphSpacingBefore = 2
        hunkStyle.paragraphSpacing = 2
        let hunk: [NSAttributedString.Key: Any] = [.font: mono, .foregroundColor: Theme.nsTextTertiary, .paragraphStyle: hunkStyle]
        let showHunk: [NSAttributedString.Key: Any] = [.font: captionFont, .foregroundColor: Theme.nsAccent, .paragraphStyle: hunkStyle]

        let text = NSMutableAttributedString()
        var rows: [DiffRow] = []
        var lineStarts: [Int] = []

        func append(_ parts: [(String, [NSAttributedString.Key: Any])], row: DiffRow) {
            if !rows.isEmpty { text.append(NSAttributedString(string: "\n", attributes: plain)) }
            lineStarts.append(text.length)
            for (string, attributes) in parts {
                text.append(NSAttributedString(string: string, attributes: attributes))
            }
            rows.append(row)
        }

        for (fileIndex, file) in model.files.enumerated() {
            if showFileHeaders {
                var parts: [(String, [NSAttributedString.Key: Any])] = [(file.displayPath, header)]
                if file.additions > 0 || file.deletions > 0 {
                    parts.append(("   ", header))
                    parts.append(("+\(file.additions)", [.font: monoBold, .foregroundColor: Theme.nsSuccess, .paragraphStyle: headerStyle]))
                    parts.append((" −\(file.deletions)", [.font: monoBold, .foregroundColor: Theme.nsDanger, .paragraphStyle: headerStyle]))
                }
                var notes: [String] = []
                if file.isNew { notes.append("new file") }
                if file.isDeleted { notes.append("deleted") }
                if let from = file.renamedFrom { notes.append("renamed from \(from)") }
                if file.isBinary { notes.append("binary") }
                if !notes.isEmpty { parts.append(("   " + notes.joined(separator: " · "), headerMuted)) }
                if fileIndex == 0 {
                    let firstStyle = NSMutableParagraphStyle()
                    firstStyle.paragraphSpacing = 6
                    parts = parts.map { string, attributes in
                        var attributes = attributes
                        attributes[.paragraphStyle] = firstStyle
                        return (string, attributes)
                    }
                }
                append(parts, row: DiffRow(kind: .fileHeader, marker: ""))
            }
            if file.hunks.isEmpty {
                let message: String
                if file.isBinary { message = "Binary file changed; no text diff." }
                else if file.renamedFrom != nil { message = "Renamed without content changes." }
                else { message = "No text changes." }
                append([(message, note)], row: DiffRow(kind: .note, marker: ""))
            }
            for hunkEntry in file.hunks {
                let expanded = isExpanded(hunkEntry.id)
                var parts: [(String, [NSAttributedString.Key: Any])] = [(hunkEntry.header, hunk)]
                if !expanded {
                    parts.append(("   Show hunk · \(hunkEntry.lines.count) line\(hunkEntry.lines.count == 1 ? "" : "s")", showHunk))
                }
                append(parts, row: DiffRow(kind: .hunkHeader(id: hunkEntry.id), marker: ""))
                guard expanded else { continue }
                for line in hunkEntry.lines {
                    let attributes: [NSAttributedString.Key: Any]
                    switch line.kind {
                    case .addition: attributes = added
                    case .deletion: attributes = removed
                    case .context: attributes = plain
                    case .noNewline: attributes = muted
                    }
                    append([(line.displayText, attributes)], row: DiffRow(kind: .line(line.kind), oldNumber: line.oldNumber, newNumber: line.newNumber, marker: line.marker))
                }
            }
        }
        return DiffDocument(text: text, rows: rows, lineStarts: lineStarts)
    }
}

// MARK: - AppKit pieces

/// The scroll view with the gutter laid over its leading edge.
fileprivate final class DiffContainerView: NSView {
    let scrollView: NSScrollView
    private let gutter: DiffGutterView

    init(scrollView: NSScrollView, gutter: DiffGutterView) {
        self.scrollView = scrollView
        self.gutter = gutter
        super.init(frame: .zero)
        addSubview(scrollView)
        addSubview(gutter, positioned: .above, relativeTo: scrollView)
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    override var isFlipped: Bool { true }

    override func layout() {
        super.layout()
        scrollView.frame = bounds
        let frame = NSRect(x: 0, y: 0, width: gutter.thickness, height: bounds.height)
        if gutter.frame != frame { gutter.frame = frame }
        gutter.needsDisplay = true
    }
}

/// Draws the row tints across the full width of the text view (the text
/// storage carries none, so selection and copy stay plain).
private final class DiffLayoutManager: NSLayoutManager {
    var rows: DiffRows = .empty

    override func drawBackground(forGlyphRange glyphsToShow: NSRange, at origin: NSPoint) {
        super.drawBackground(forGlyphRange: glyphsToShow, at: origin)
        guard let container = textContainers.first, let textView = container.textView else { return }
        let width = textView.bounds.width
        enumerateLineFragments(forGlyphRange: glyphsToShow) { [rows] rect, _, _, glyphRange, _ in
            let character = self.characterIndexForGlyph(at: glyphRange.location)
            guard let index = rows.index(containing: character), let color = rows.rows[index].background else { return }
            color.setFill()
            NSRect(x: 0, y: rect.minY + origin.y, width: width, height: rect.height).fill()
        }
    }
}

/// Read-only text view whose clicks on a `@@` header toggle the hunk.
private final class DiffTextView: NSTextView {
    var rows: DiffRows = .empty
    var onToggleHunk: ((Int) -> Void)?

    /// The hunk whose header line is under `point` (in the view's coordinates).
    private func hunkHeader(at point: NSPoint) -> Int? {
        guard let layoutManager, let textContainer, layoutManager.numberOfGlyphs > 0 else { return nil }
        let local = NSPoint(x: point.x - textContainerOrigin.x, y: point.y - textContainerOrigin.y)
        let glyph = layoutManager.glyphIndex(for: local, in: textContainer)
        let fragment = layoutManager.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil)
        guard local.y >= fragment.minY, local.y < fragment.maxY else { return nil }
        let character = layoutManager.characterIndexForGlyph(at: glyph)
        guard let index = rows.index(containing: character), case .hunkHeader(let id) = rows.rows[index].kind else { return nil }
        return id
    }
}

/// Old / new line numbers and the +/− marker beside each row, over the same
/// tint as the row so the band runs edge to edge. Sits over the leading edge
/// of the scroll view; the text is indented past it.
fileprivate final class DiffGutterView: NSView {
    struct Fragment {
        var minY: CGFloat
        var height: CGFloat
        var row: Int
    }

    private weak var textView: NSTextView?
    private var rows: DiffRows = .empty
    private var fragments: [Fragment] = []
    private var digits = 2

    private static let numberFont = NSFont.monospacedDigitSystemFont(ofSize: Theme.Font.nsMono.pointSize - 1, weight: .regular)
    private static let markerFont = Theme.Font.nsMono
    private static let digitWidth: CGFloat = ("0" as NSString).size(withAttributes: [.font: numberFont]).width
    private static let markerWidth: CGFloat = 16
    private static let columnGap: CGFloat = 6
    private static let leading: CGFloat = 10

    init(textView: NSTextView) {
        self.textView = textView
        super.init(frame: .zero)
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    override var isFlipped: Bool { true }

    static func width(digits: Int) -> CGFloat {
        leading + 2 * (CGFloat(digits) * digitWidth + columnGap) + markerWidth
    }

    var thickness: CGFloat { Self.width(digits: digits) }

    func update(rows: DiffRows, fragments: [Fragment], digits: Int) {
        self.rows = rows
        self.fragments = fragments
        self.digits = digits
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        // Inside the SwiftUI host the dirty rect can span the whole sibling
        // scroll view, not just this view: clip, or the fill below paints
        // over the text.
        bounds.clip()
        Theme.nsSurface.setFill()
        bounds.fill()
        guard let textView, !fragments.isEmpty else { return }
        let numberAttributes: [NSAttributedString.Key: Any] = [.font: Self.numberFont, .foregroundColor: Theme.nsTextTertiary]
        let columnWidth = CGFloat(digits) * Self.digitWidth
        let width = thickness
        // The dirty rect in the text view's coordinates picks the fragments to draw.
        let top = convert(NSPoint(x: 0, y: dirtyRect.minY), to: textView).y
        let bottom = convert(NSPoint(x: 0, y: dirtyRect.maxY), to: textView).y
        var low = 0, high = fragments.count - 1
        while low < high {
            let mid = (low + high) / 2
            if fragments[mid].minY + fragments[mid].height < top { low = mid + 1 } else { high = mid }
        }
        for fragment in fragments[low...] {
            if fragment.minY > bottom { break }
            let row = rows.rows[fragment.row]
            let y = convert(NSPoint(x: 0, y: fragment.minY), from: textView).y
            let band = NSRect(x: 0, y: y, width: width, height: fragment.height)
            if let color = row.background {
                color.setFill()
                band.fill()
            }
            func label(_ string: String, attributes: [NSAttributedString.Key: Any], right: CGFloat) {
                let text = NSAttributedString(string: string, attributes: attributes)
                let size = text.size()
                text.draw(at: NSPoint(x: right - size.width, y: y + (fragment.height - size.height) / 2))
            }
            var right = Self.leading + columnWidth
            if let old = row.oldNumber { label(String(old), attributes: numberAttributes, right: right) }
            right += Self.columnGap + columnWidth
            if let new = row.newNumber { label(String(new), attributes: numberAttributes, right: right) }
            if !row.marker.trimmingCharacters(in: .whitespaces).isEmpty {
                let marker = NSAttributedString(string: row.marker, attributes: [.font: Self.markerFont, .foregroundColor: row.markerColor])
                let size = marker.size()
                marker.draw(at: NSPoint(x: right + Self.columnGap + (Self.markerWidth - size.width) / 2, y: y + (fragment.height - size.height) / 2))
            }
        }
    }
}

// MARK: - Banner

private struct TruncatedBanner: View {
    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: Icons.warning)
                .font(.system(size: 12, weight: .medium))
            Text("Diff truncated — the backend caps diffs at 2 MB. Open the file in your editor for the full change.")
                .font(Theme.Font.secondary)
                .lineLimit(2)
        }
        .foregroundStyle(Theme.warning)
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.warning.opacity(0.1))
        .hairline(.bottom)
        .accessibilityElement(children: .combine)
    }
}

// MARK: - Parsing

/// A parsed unified diff. Plain data; built once per diff text.
struct ParsedDiff: Equatable {
    enum LineKind: Equatable {
        case context, addition, deletion, noNewline
    }

    struct Line: Identifiable, Equatable {
        let id: Int
        let kind: LineKind
        let oldNumber: Int?
        let newNumber: Int?
        /// Tabs expanded; empty lines carry a single space so rows keep their height.
        let displayText: String

        var marker: String {
            switch kind {
            case .context: return " "
            case .addition: return "+"
            case .deletion: return "−"
            case .noNewline: return "\\"
            }
        }
    }

    struct Hunk: Identifiable, Equatable {
        /// Global hunk index across files (collapse threshold).
        let id: Int
        let header: String
        var lines: [Line] = []
    }

    struct File: Identifiable, Equatable {
        let id: Int
        var oldPath: String?
        var newPath: String?
        var isNew = false
        var isDeleted = false
        var isBinary = false
        var renamedFrom: String?
        var hunks: [Hunk] = []
        var additions = 0
        var deletions = 0

        var displayPath: String { newPath ?? oldPath ?? "file" }
    }

    var files: [File] = []
    var additions = 0
    var deletions = 0
    var hunkCount = 0
    var maxLineNumber = 0
    /// Longest line in UTF-8 bytes (a cheap proxy for rendered width).
    var maxLineLength = 0

    var isEmpty: Bool { files.isEmpty }

    static func parse(_ text: String) -> ParsedDiff {
        var result = ParsedDiff()
        var parts = text.split(separator: "\n", omittingEmptySubsequences: false)
        if parts.last?.isEmpty == true { parts.removeLast() }

        var current: File?
        var hunk: Hunk?
        var oldLine = 0
        var newLine = 0
        var nextLineId = 0

        func closeHunk() {
            if let open = hunk {
                current?.hunks.append(open)
                hunk = nil
            }
        }
        func closeFile() {
            closeHunk()
            if let file = current {
                result.files.append(file)
                current = nil
            }
        }

        for raw in parts {
            var line = raw
            if line.hasSuffix("\r") { line = line.dropLast() }

            if line.hasPrefix("diff --git ") {
                closeFile()
                var file = File(id: result.files.count)
                let rest = line.dropFirst("diff --git ".count)
                if let split = rest.range(of: " b/") {
                    file.oldPath = stripPrefix(String(rest[..<split.lowerBound]), "a/")
                    file.newPath = String(rest[split.upperBound...])
                } else {
                    file.newPath = String(rest)
                }
                current = file
                continue
            }

            if line.hasPrefix("@@") {
                closeHunk()
                if current == nil { current = File(id: result.files.count) }
                let starts = hunkStarts(line)
                oldLine = starts.old
                newLine = starts.new
                hunk = Hunk(id: result.hunkCount, header: String(line))
                result.hunkCount += 1
                continue
            }

            if hunk == nil {
                // File header area (or a diff without `diff --git`).
                if current == nil {
                    if line.isEmpty { continue }
                    current = File(id: result.files.count)
                }
                if line.hasPrefix("--- ") {
                    let path = String(line.dropFirst(4))
                    if path == "/dev/null" { current?.isNew = true } else { current?.oldPath = stripPrefix(path, "a/") }
                } else if line.hasPrefix("+++ ") {
                    let path = String(line.dropFirst(4))
                    if path == "/dev/null" { current?.isDeleted = true } else { current?.newPath = stripPrefix(path, "b/") }
                } else if line.hasPrefix("new file mode") {
                    current?.isNew = true
                } else if line.hasPrefix("deleted file mode") {
                    current?.isDeleted = true
                } else if line.hasPrefix("rename from ") {
                    current?.renamedFrom = String(line.dropFirst("rename from ".count))
                } else if line.hasPrefix("rename to ") {
                    current?.newPath = String(line.dropFirst("rename to ".count))
                } else if line.hasPrefix("Binary files") || line.hasPrefix("GIT binary patch") {
                    current?.isBinary = true
                }
                continue
            }

            guard var open = hunk else { continue }
            let entry: Line
            if line.hasPrefix("\\") {
                entry = Line(id: nextLineId, kind: .noNewline, oldNumber: nil, newNumber: nil, displayText: display(line[...]))
            } else if line.hasPrefix("+") {
                entry = Line(id: nextLineId, kind: .addition, oldNumber: nil, newNumber: newLine, displayText: display(line.dropFirst()))
                newLine += 1
                result.additions += 1
                current?.additions += 1
            } else if line.hasPrefix("-") {
                entry = Line(id: nextLineId, kind: .deletion, oldNumber: oldLine, newNumber: nil, displayText: display(line.dropFirst()))
                oldLine += 1
                result.deletions += 1
                current?.deletions += 1
            } else {
                let body = line.hasPrefix(" ") ? line.dropFirst() : line[...]
                entry = Line(id: nextLineId, kind: .context, oldNumber: oldLine, newNumber: newLine, displayText: display(body))
                oldLine += 1
                newLine += 1
            }
            nextLineId += 1
            result.maxLineNumber = max(result.maxLineNumber, oldLine, newLine)
            result.maxLineLength = max(result.maxLineLength, entry.displayText.utf8.count)
            open.lines.append(entry)
            hunk = open
        }
        closeFile()
        return result
    }

    /// `@@ -12,7 +12,8 @@` → (12, 12); missing numbers default to 1.
    private static func hunkStarts(_ line: Substring) -> (old: Int, new: Int) {
        var old = 1
        var new = 1
        for token in line.split(separator: " ") {
            if token.hasPrefix("-"), old == 1, let value = Int(token.dropFirst().split(separator: ",").first ?? "") {
                old = value
            } else if token.hasPrefix("+"), new == 1, let value = Int(token.dropFirst().split(separator: ",").first ?? "") {
                new = value
            }
        }
        return (old, new)
    }

    private static func stripPrefix(_ path: String, _ prefix: String) -> String {
        path.hasPrefix(prefix) ? String(path.dropFirst(prefix.count)) : path
    }

    private static func display(_ text: Substring) -> String {
        if text.isEmpty { return " " }
        if text.contains("\t") { return text.replacingOccurrences(of: "\t", with: "    ") }
        return String(text)
    }
}

// MARK: - Preview

#Preview("Diff") {
    let sample = """
    diff --git a/src/app.rs b/src/app.rs
    index 3b18e51..a4c2f10 100644
    --- a/src/app.rs
    +++ b/src/app.rs
    @@ -1,6 +1,7 @@
     use std::fmt;
    +use std::collections::HashMap;

     pub struct App {
    -    name: String,
    +    name: String,
    +    settings: HashMap<String, String>,
     }
    @@ -40,7 +41,7 @@ impl App {
         pub fn run(&self) -> Result<(), Error> {
    -        println!("starting {}", self.name);
    +        println!("starting {} with {} settings", self.name, self.settings.len());
             Ok(())
         }
     }
    diff --git a/README.md b/README.md
    new file mode 100644
    index 0000000..e69de29
    --- /dev/null
    +++ b/README.md
    @@ -0,0 +1,2 @@
    +# Context
    +A coding agent with a context engine.
    \\ No newline at end of file
    diff --git a/logo.png b/logo.png
    Binary files a/logo.png and b/logo.png differ
    """
    return DiffView(diff: sample, truncated: true)
        .frame(width: 720, height: 520)
        .background(Theme.canvas)
}
