import AppKit
import QuartzCore

/// A vertical scroller whose knob glides to its new size and place when the
/// document's height changes (rows loading or unloading above or below the
/// visible ones, a row's estimated height replaced by its measured one, a
/// card expanding) instead of jumping there. Scrolling moves the knob as
/// usual, and dragging it is never smoothed.
///
/// The knob shown is the scroll view's (`scrolled`) plus an offset that eases
/// to zero. A change of the document's height starts a new offset that holds
/// the knob where it was, and so does every other change in the same turn of
/// the run loop: the scroll corrections that keep the visible rows in place
/// (`ScrollKeeper`, SwiftUI's size-change anchor) come in that turn, and the
/// knob should not show the uncorrected position in between.
final class SmoothScroller: NSScroller {
    override class var isCompatibleWithOverlayScrollers: Bool { true }

    private struct Knob {
        var value: Double = 0
        var proportion: CGFloat = 0

        static func - (lhs: Knob, rhs: Knob) -> Knob {
            Knob(value: lhs.value - rhs.value, proportion: lhs.proportion - rhs.proportion)
        }
    }

    private static let duration: CFTimeInterval = 0.32

    private var scrolled = Knob()
    private var startOffset = Knob()
    private var startedAt: CFTimeInterval = 0
    private var documentHeight: CGFloat = 0
    private var holding = false
    private var tracking = false
    private var link: CADisplayLink?

    /// Replaces `scrollView`'s vertical scroller with one of these, keeping
    /// its style and position.
    static func install(in scrollView: NSScrollView) {
        guard let old = scrollView.verticalScroller, !(old is SmoothScroller) else { return }
        let scroller = SmoothScroller(frame: old.frame)
        scroller.controlSize = old.controlSize
        scroller.knobStyle = old.knobStyle
        scroller.scrollerStyle = old.scrollerStyle
        scrollView.verticalScroller = scroller
        scrollView.reflectScrolledClipView(scrollView.contentView)
    }

    override var doubleValue: Double {
        get { super.doubleValue }
        set { update { $0.value = newValue } }
    }

    override var knobProportion: CGFloat {
        get { super.knobProportion }
        set { update { $0.proportion = newValue } }
    }

    override func mouseDown(with event: NSEvent) {
        settle()
        tracking = true
        defer { tracking = false }
        super.mouseDown(with: event)
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window == nil { settle() }
    }

    private func update(_ change: (inout Knob) -> Void) {
        let now = CACurrentMediaTime()
        let shown = displayed(at: now)
        change(&scrolled)
        let height = (superview as? NSScrollView)?.documentView?.frame.height ?? 0
        if height != documentHeight {
            if documentHeight > 0, !tracking, window != nil, !holding {
                holding = true
                RunLoop.main.perform { [weak self] in self?.holding = false }
            }
            documentHeight = height
        }
        if holding, !tracking {
            startOffset = shown - scrolled
            startedAt = now
        }
        apply(at: now)
    }

    private func offset(at time: CFTimeInterval) -> Knob {
        let progress = min(1, max(0, (time - startedAt) / Self.duration))
        let remaining = pow(1 - progress, 3)
        return Knob(value: startOffset.value * remaining, proportion: startOffset.proportion * CGFloat(remaining))
    }

    private func displayed(at time: CFTimeInterval) -> Knob {
        let offset = offset(at: time)
        return Knob(value: scrolled.value + offset.value, proportion: scrolled.proportion + offset.proportion)
    }

    private func apply(at time: CFTimeInterval) {
        let shown = displayed(at: time)
        super.doubleValue = min(1, max(0, shown.value))
        super.knobProportion = min(1, max(0, shown.proportion))
        let moving = time - startedAt < Self.duration && (startOffset.value != 0 || startOffset.proportion != 0)
        if moving, link == nil {
            let link = displayLink(target: self, selector: #selector(tick))
            link.add(to: .main, forMode: .common)
            self.link = link
        } else if !moving {
            link?.invalidate()
            link = nil
        }
    }

    @objc private func tick() {
        apply(at: CACurrentMediaTime())
    }

    /// Shows the scroll view's knob as it is.
    private func settle() {
        startOffset = Knob()
        apply(at: CACurrentMediaTime())
    }
}
