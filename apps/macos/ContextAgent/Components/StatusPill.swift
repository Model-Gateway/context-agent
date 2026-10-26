import SwiftUI

/// A tinted pill that always carries the status word in text, never colour
/// alone: Completed / Blocked / Failed / Cancelled / Running / Idle.
struct StatusPill: View {
    var text: String
    var tint: Color
    private var isRunning = false

    init(_ status: TaskStatus) {
        switch status {
        case .completed:
            text = "Completed"
            tint = Theme.success
        case .blocked:
            text = "Blocked"
            tint = Theme.warning
        case .failed:
            text = "Failed"
            tint = Theme.danger
        case .cancelled:
            text = "Cancelled"
            tint = Theme.textSecondary
        }
    }

    init(running: Bool) {
        text = running ? "Running" : "Idle"
        tint = running ? Theme.accent : Theme.textTertiary
        isRunning = running
    }

    init(text: String, tint: Color) {
        self.text = text
        self.tint = tint
    }

    var body: some View {
        HStack(spacing: 5) {
            if isRunning {
                BrailleSpinner(size: 10, tint: tint)
            } else {
                Circle()
                    .fill(tint)
                    .frame(width: 6, height: 6)
                    .accessibilityHidden(true)
            }
            Text(text)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(tint)
                .lineLimit(1)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
        .background(tint.opacity(0.12), in: Capsule())
        .fixedSize()
        .accessibilityElement(children: .combine)
    }
}

#Preview("Status pills") {
    VStack(alignment: .leading, spacing: 10) {
        HStack(spacing: 8) {
            StatusPill(.completed)
            StatusPill(.blocked)
            StatusPill(.failed)
            StatusPill(.cancelled)
        }
        HStack(spacing: 8) {
            StatusPill(running: true)
            StatusPill(running: false)
            StatusPill(text: "PASSED", tint: Theme.success)
            StatusPill(text: "FAILED", tint: Theme.danger)
        }
    }
    .padding(24)
    .background(Theme.canvas)
}
