import SwiftUI
import AppKit

/// Which part of a long list (a transcript, an agent's tool calls) is
/// loaded into views: the newest `page` items at first, a page more as the
/// user scrolls up, never more than `limit` — loading older items unloads
/// the newest ones, and scrolling back down loads them again. While the
/// window ends at the newest item (`followsTail`) it grows with the list.
///
/// The unloaded items keep their place as empty space (`spacerAbove`,
/// `spacerBelow`) at the measured average row height, so the scroller
/// stands for the whole list and does not change size as pages load.
/// Indices are positions in the list; `TailWindowScrolling` applies the
/// window to a scroll view.
struct TailWindow: Equatable {
    var page = 30
    var limit = 90
    private(set) var lower = 0
    private(set) var upper = 0
    private(set) var followsTail = true
    /// Average height of a loaded row (spacing included); 0 until measured.
    private(set) var rowHeight: CGFloat = 0

    /// The newest `page` of a list of `count` items.
    init(count: Int, page: Int = 30, limit: Int = 90) {
        self.page = page
        self.limit = limit
        reset(count: count)
    }

    /// The loaded slice of a list of `count` items.
    func range(count: Int) -> Range<Int> {
        let upper = followsTail ? count : min(self.upper, count)
        return min(lower, upper)..<upper
    }

    /// Room for the unloaded items before the window.
    var spacerAbove: CGFloat { CGFloat(lower) * rowHeight }

    /// Room for the unloaded items after the window.
    func spacerBelow(count: Int) -> CGFloat { CGFloat(count - range(count: count).upperBound) * rowHeight }

    var span: Int { upper - lower }
    var hasOlder: Bool { lower > 0 }
    func hasNewer(count: Int) -> Bool { !followsTail && upper < count }

    /// The newest page, following the tail.
    mutating func reset(count: Int) {
        followsTail = true
        upper = count
        lower = max(0, count - page)
    }

    /// The list changed to `count` items. Removals (a rewind) and bulk
    /// loads (history) start over at the tail; items appended while
    /// following extend the window.
    mutating func sync(count: Int) {
        if count < upper || lower > count || (followsTail && count - upper > page) {
            reset(count: count)
        } else if followsTail {
            upper = count
        }
    }

    /// The loaded rows measured `height` together: the new average when it
    /// moved by more than a tenth (rows streaming in leave it alone).
    mutating func measure(height: CGFloat, rows: Int) -> Bool {
        guard rows > 0, height > 0 else { return false }
        let average = height / CGFloat(rows)
        guard rowHeight == 0 || abs(average - rowHeight) > rowHeight / 10 else { return false }
        rowHeight = average
        return true
    }

    mutating func loadOlder() {
        lower = max(0, lower - page)
    }

    mutating func loadNewer(count: Int) {
        upper = min(count, upper + page)
        if upper == count { followsTail = true }
    }

    var isOverLimit: Bool { span > limit }

    /// Unloads the newest items past `limit` (after loading older ones).
    mutating func trimNewest() {
        guard isOverLimit else { return }
        upper = lower + limit
        followsTail = false
    }

    /// Unloads the oldest `count` items (past `limit` after loading newer
    /// ones, or beyond the newest page while following).
    mutating func trimOldest(_ count: Int) {
        lower = min(upper - 1, lower + max(0, count))
    }

    /// Two pages around `index` (a find match, or where the user dragged
    /// the scroller to), following the tail when they reach it.
    mutating func place(around index: Int, count: Int) {
        lower = max(0, min(index - page, count - 2 * page))
        upper = min(count, lower + 2 * page)
        followsTail = upper == count
    }

    /// A page around `index` unless it is loaded already.
    mutating func reveal(_ index: Int, count: Int) {
        guard !range(count: count).contains(index) else { return }
        place(around: index, count: count)
    }
}

/// Drives a `TailWindow` from a vertical `ScrollView`'s geometry. Less than
/// `threshold` of loaded rows above the visible ones loads an older page;
/// less below (in a window that stops short of the newest item) loads a
/// newer one; the far end unloads past the window's limit. Following the
/// newest item, rows more than 1.5 × `threshold` above the visible ones
/// unload down to one page (the gap between the two keeps a load and an
/// unload from undoing each other). A visible area entirely in the empty
/// space of unloaded items (the scroller dragged there) loads the items
/// that belong there.
///
/// The visible rows stay put. Items added or removed below them change
/// nothing above; for items added or removed above them a `ScrollKeeper`
/// holds the distance from the bottom of the content fixed, adjusting the
/// scroll view in the same layout pass that changes its height (SwiftUI's
/// own size-change anchor is not applied while the user is scrolling). The
/// average row height only changes while the newest item is loaded, so the
/// space below (none) stays the same while a distance is held.
/// Each change waits for the content to settle before the next. When idle
/// the scroll view anchors size changes to the bottom while `followsBottom`
/// (new content keeps the newest row in view).
///
/// The view measures its loaded rows (`MeasuredRows`).
struct TailWindowScrolling: ViewModifier {
    @Binding var window: TailWindow
    var itemCount: Int
    /// Whether the view keeps the newest content in view; nil derives it
    /// from the scroll position (at the bottom or not).
    var followsBottom: Bool?
    /// The loaded rows' total height, as measured by the view.
    var measuredRows: MeasuredRows
    var threshold: CGFloat = 600

    @State private var geometry = Geometry()
    /// The latest inputs: `step` also runs from tasks that captured an
    /// older copy of this modifier.
    @State private var latest = Inputs()
    @State private var busy = false
    @State private var keeper = ScrollKeeper()
    /// When the content's height last changed: a change is settled once it
    /// has held for a while (rows such as messages with file chips size in
    /// over more than one pass).
    @State private var heightChangedAt = Date.distantPast

    struct Inputs: Equatable {
        var count = 0
        var rows = MeasuredRows()
        var followsBottom: Bool?
    }

    struct Geometry: Equatable {
        var top: CGFloat = 0
        var bottom: CGFloat = 0
        var height: CGFloat = 0
        var atBottom: Bool { height - bottom < 24 }
    }

    private var following: Bool { latest.followsBottom ?? geometry.atBottom }
    private var count: Int { latest.count }
    private var rows: MeasuredRows { latest.rows }

    func body(content: Content) -> some View {
        content
            .defaultScrollAnchor((followsBottom ?? geometry.atBottom) ? .bottom : .top, for: .sizeChanges)
            .background(ScrollKeeper.Locator(keeper: keeper))
            .onScrollGeometryChange(for: Geometry.self) { geometry in
                let visible = geometry.visibleRect
                return Geometry(top: visible.minY.rounded(), bottom: visible.maxY.rounded(), height: geometry.contentSize.height.rounded())
            } action: { old, new in
                geometry = new
                if old.height != new.height { heightChangedAt = Date() }
                step()
            }
            .onChange(of: Inputs(count: itemCount, rows: measuredRows, followsBottom: followsBottom), initial: true) { _, new in
                latest = new
                window.sync(count: new.count)
                step()
            }
    }

    private func step() {
        guard !busy else { return }
        let range = window.range(count: count)
        // The average row height changes the empty space above and below:
        // only while the newest item is loaded (no space below), holding
        // the bottom. A measurement taken before the window last changed
        // is stale.
        if window.followsTail, rows.count == range.count {
            var measured = window
            if measured.measure(height: rows.height, rows: rows.count) {
                run(keepingBottom: true) { window = measured }
                return
            }
        }
        let rowsTop = window.spacerAbove
        let rowsBottom = geometry.height - window.spacerBelow(count: count)
        if window.rowHeight > 0, geometry.bottom < rowsTop || geometry.top > rowsBottom {
            let index = min(max(0, Int(((geometry.top + geometry.bottom) / 2) / window.rowHeight)), max(0, count - 1))
            run(keepingBottom: false) { window.place(around: index, count: count) }
            return
        }
        let above = geometry.top - rowsTop
        let below = rowsBottom - geometry.bottom
        if above < threshold, window.hasOlder {
            run(keepingBottom: true) { window.loadOlder() } then: {
                if window.isOverLimit { run(keepingBottom: false) { window.trimNewest() } }
            }
        } else if below < threshold, window.hasNewer(count: count) {
            run(keepingBottom: false) { window.loadNewer(count: count) } then: {
                if window.isOverLimit { run(keepingBottom: true) { window.trimOldest(window.span - window.limit) } }
            }
        } else if window.followsTail, following, window.span > window.page, window.rowHeight > 0 {
            let spare = Int((above - threshold * 1.5) / window.rowHeight)
            let unload = min(window.span - window.page, spare)
            if unload > 0 {
                run(keepingBottom: true) { window.trimOldest(unload) }
            }
        }
    }

    /// Changes the window (holding the distance from the bottom when the
    /// change is above the visible rows), waits until the content's height
    /// has held for 100 ms (at most 600 ms), then runs `next` or looks for
    /// more work.
    private func run(keepingBottom: Bool, _ change: @escaping () -> Void, then next: (() -> Void)? = nil) {
        busy = true
        if keepingBottom { keeper.keepBottom() }
        change()
        Task { @MainActor in
            let started = Date()
            repeat {
                try? await Task.sleep(for: .milliseconds(16))
            } while Date().timeIntervalSince(heightChangedAt) < 0.1 && Date().timeIntervalSince(started) < 0.6
            keeper.release()
            busy = false
            if let next {
                next()
                if busy { return }
            }
            step()
        }
    }
}

/// Holds a scroll view's distance from the bottom of its content while
/// rows above the visible ones load or unload: when the document view's
/// height changes it moves the clip view by the same amount, synchronously
/// in that layout pass, and scrolling by the user in the meantime moves the
/// held distance with it.
@MainActor
final class ScrollKeeper {
    private weak var scrollView: NSScrollView?
    private var observers: [NSObjectProtocol] = []
    private var distanceFromBottom: CGFloat?
    private var adjusting = false

    func keepBottom() {
        guard let scrollView, let document = scrollView.documentView else { return }
        distanceFromBottom = document.frame.height - scrollView.contentView.bounds.origin.y
    }

    func release() {
        distanceFromBottom = nil
    }

    fileprivate func attach(_ scrollView: NSScrollView) {
        // Rows loading at measured rather than average heights resize the
        // knob: let it glide there (SwiftUI may put its own scroller back).
        SmoothScroller.install(in: scrollView)
        guard scrollView !== self.scrollView, let document = scrollView.documentView else { return }
        observers.forEach(NotificationCenter.default.removeObserver)
        self.scrollView = scrollView
        document.postsFrameChangedNotifications = true
        scrollView.contentView.postsBoundsChangedNotifications = true
        observers = [
            NotificationCenter.default.addObserver(forName: NSView.frameDidChangeNotification, object: document, queue: nil) { [weak self] _ in
                MainActor.assumeIsolated { self?.documentResized() }
            },
            NotificationCenter.default.addObserver(forName: NSView.boundsDidChangeNotification, object: scrollView.contentView, queue: nil) { [weak self] _ in
                MainActor.assumeIsolated { self?.scrolled() }
            },
        ]
    }

    private func documentResized() {
        guard let distanceFromBottom, let scrollView, let document = scrollView.documentView else { return }
        let clip = scrollView.contentView
        let target = max(-scrollView.contentInsets.top, document.frame.height - distanceFromBottom)
        guard abs(clip.bounds.origin.y - target) > 0.5 else { return }
        adjusting = true
        clip.scroll(to: NSPoint(x: clip.bounds.origin.x, y: target))
        scrollView.reflectScrolledClipView(clip)
        adjusting = false
    }

    /// The user scrolled while a distance is held: hold the new one.
    private func scrolled() {
        guard !adjusting, distanceFromBottom != nil, let scrollView, let document = scrollView.documentView else { return }
        distanceFromBottom = document.frame.height - scrollView.contentView.bounds.origin.y
    }

    /// Placed behind a `ScrollView`: finds the `NSScrollView` with its frame.
    struct Locator: NSViewRepresentable {
        let keeper: ScrollKeeper

        func makeNSView(context: Context) -> NSView { Probe(keeper: keeper) }
        func updateNSView(_ nsView: NSView, context: Context) { (nsView as? Probe)?.locate() }

        final class Probe: NSView {
            let keeper: ScrollKeeper
            init(keeper: ScrollKeeper) {
                self.keeper = keeper
                super.init(frame: .zero)
            }
            required init?(coder: NSCoder) { fatalError() }

            override func viewDidMoveToWindow() {
                super.viewDidMoveToWindow()
                locate()
            }

            override func layout() {
                super.layout()
                locate()
            }

            func locate() {
                guard let root = window?.contentView else { return }
                let frame = convert(bounds, to: nil)
                guard frame.width > 0, frame.height > 0 else { return }
                func find(_ view: NSView) -> NSScrollView? {
                    if let scroll = view as? NSScrollView {
                        let other = scroll.convert(scroll.bounds, to: nil)
                        if abs(other.minX - frame.minX) < 1, abs(other.minY - frame.minY) < 1, abs(other.width - frame.width) < 1, abs(other.height - frame.height) < 1 {
                            return scroll
                        }
                    }
                    for subview in view.subviews {
                        if let found = find(subview) { return found }
                    }
                    return nil
                }
                if let scroll = find(root) { keeper.attach(scroll) }
            }
        }
    }
}

extension View {
    /// See `TailWindowScrolling`; apply to the `ScrollView`.
    func tailWindow(_ window: Binding<TailWindow>, count: Int, rows: MeasuredRows, followsBottom: Bool? = nil) -> some View {
        modifier(TailWindowScrolling(window: window, itemCount: count, followsBottom: followsBottom, measuredRows: rows))
    }
}

/// The total height of a window's loaded rows (spacing included) and how
/// many rows that was.
struct MeasuredRows: Equatable {
    var height: CGFloat = 0
    var count = 0
}

/// The space standing for unloaded items above or below a window: next
/// to the loaded rows (`rowsEdge`) up to `placeholderHeight` of it shows
/// `placeholder` rows (a skeleton of what loads there as the user scrolls
/// on), the rest stays empty. The skeleton pulses only while on screen.
struct TailWindowSpacer<Placeholder: View>: View {
    var height: CGFloat
    /// The edge that meets the loaded rows.
    var rowsEdge: VerticalEdge
    /// One placeholder row's height, to know how many fill the space.
    var placeholderRowHeight: CGFloat
    var placeholderHeight: CGFloat = 960
    /// The `index`-th placeholder row (vary the shapes by it).
    @ViewBuilder var placeholder: (_ index: Int) -> Placeholder

    @State private var onScreen = false

    var body: some View {
        if height > 0 {
            let shown = min(height, placeholderHeight)
            let rows = Int((shown / max(placeholderRowHeight, 1)).rounded(.up)) + 1
            VStack(spacing: 0) {
                if rowsEdge == .bottom { Color.clear.frame(height: height - shown) }
                VStack(spacing: 0) {
                    ForEach(0..<rows, id: \.self) { index in
                        placeholder(rowsEdge == .bottom ? rows - 1 - index : index)
                    }
                }
                .frame(height: shown, alignment: rowsEdge == .bottom ? .bottom : .top)
                .clipped()
                .skeletonPulse(active: onScreen)
                .onScrollVisibilityChange(threshold: 0.01) { onScreen = $0 }
                if rowsEdge == .top { Color.clear.frame(height: height - shown) }
            }
            .frame(height: height)
        }
    }
}
