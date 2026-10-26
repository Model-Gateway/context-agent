import SwiftUI
import Foundation

/// A transient message in the window's top-right corner.
struct Toast: Identifiable, Equatable {
    enum Kind: Equatable {
        case info, success, warning, error

        var systemImage: String {
            switch self {
            case .info: return "info.circle"
            case .success: return "checkmark.circle"
            case .warning: return "exclamationmark.triangle"
            case .error: return "xmark.octagon"
            }
        }

        var tint: Color {
            switch self {
            case .info: return Theme.textSecondary
            case .success: return Theme.success
            case .warning: return Theme.warning
            case .error: return Theme.danger
            }
        }
    }

    let id: UUID
    let message: String
    let kind: Kind
    /// Longer text behind the "Details" action (stderr tail, RPC message…).
    let details: String?
    let date: Date

    var hasDetails: Bool { !(details ?? "").isEmpty }
}

/// Owns the visible toasts: identical messages are deduped within 10 s, every
/// toast auto-dismisses after 5 s, and "Details" routes through `onDetails`
/// (AppState shows the details in an alert).
@MainActor
@Observable
final class ToastCenter {
    static let dedupeInterval: TimeInterval = 10
    static let autoDismissInterval: TimeInterval = 5
    static let maxVisible = 4

    private(set) var toasts: [Toast] = []

    /// Called when the user presses "Details" on a toast.
    @ObservationIgnored var onDetails: ((Toast) -> Void)?

    @ObservationIgnored private var lastShown: [String: Date] = [:]
    @ObservationIgnored private var timers: [UUID: Task<Void, Never>] = [:]

    init() {}

    /// Shows a toast unless the same message was shown within the last 10 s.
    func show(_ message: String, kind: Toast.Kind = .info, details: String? = nil) {
        let trimmed = message.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        let now = Date()
        if let last = lastShown[trimmed], now.timeIntervalSince(last) < Self.dedupeInterval {
            // Refresh the visible copy's timer instead of stacking a duplicate.
            if let existing = toasts.first(where: { $0.message == trimmed }) {
                schedule(existing.id)
            }
            return
        }
        lastShown[trimmed] = now
        pruneLastShown(now: now)

        let toast = Toast(id: UUID(), message: trimmed, kind: kind, details: details, date: now)
        toasts.append(toast)
        if toasts.count > Self.maxVisible {
            let dropped = toasts.removeFirst()
            timers[dropped.id]?.cancel()
            timers[dropped.id] = nil
        }
        schedule(toast.id)
    }

    func dismiss(_ id: UUID) {
        timers[id]?.cancel()
        timers[id] = nil
        toasts.removeAll { $0.id == id }
    }

    func dismissAll() {
        for (_, task) in timers { task.cancel() }
        timers.removeAll()
        toasts.removeAll()
    }

    /// The "Details" action: hands the toast to `onDetails` and dismisses it.
    func showDetails(_ toast: Toast) {
        onDetails?(toast)
        dismiss(toast.id)
    }

    // MARK: - Private

    private func schedule(_ id: UUID) {
        timers[id]?.cancel()
        timers[id] = Task { [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(Self.autoDismissInterval * 1_000_000_000))
            guard !Task.isCancelled else { return }
            self?.dismiss(id)
        }
    }

    private func pruneLastShown(now: Date) {
        guard lastShown.count > 64 else { return }
        lastShown = lastShown.filter { now.timeIntervalSince($0.value) < Self.dedupeInterval }
    }
}
