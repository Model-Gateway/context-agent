import SwiftUI

/// One reasoning-effort level the composer offers (`model.reasoning_effort`).
struct EffortLevel: Identifiable, Equatable {
    /// The config value; "" is Auto (no value sent, the model decides).
    let value: String
    let title: String
    let systemImage: String
    let description: String

    var id: String { value }
    var isUltra: Bool { value == "ultra" }

    static let auto = EffortLevel(value: "", title: "Auto", systemImage: "wand.and.stars", description: "The model decides")
    static let ultra = EffortLevel(value: "ultra", title: "Ultra", systemImage: Icons.agents, description: "The model's strongest effort, plus sub-agents in parallel")
    /// Offered for a model models.dev does not know.
    static let standard = ["low", "medium", "high"]

    /// The values models.dev lists, with an icon and a few words each.
    private static let known: [EffortLevel] = [
        EffortLevel(value: "none", title: "None", systemImage: "circle.slash", description: "No reasoning"),
        EffortLevel(value: "minimal", title: "Minimal", systemImage: "gauge.with.dots.needle.0percent", description: "Barely any reasoning"),
        EffortLevel(value: "low", title: "Low", systemImage: "gauge.with.dots.needle.33percent", description: "Fastest replies"),
        EffortLevel(value: "medium", title: "Medium", systemImage: "gauge.with.dots.needle.50percent", description: "Balanced"),
        EffortLevel(value: "high", title: "High", systemImage: "gauge.with.dots.needle.67percent", description: "Deeper reasoning"),
        EffortLevel(value: "xhigh", title: "Extra High", systemImage: "gauge.with.dots.needle.100percent", description: "Deeper still"),
        EffortLevel(value: "max", title: "Max", systemImage: "flame", description: "The model's maximum"),
    ]

    /// Auto, the effort values the model accepts as models.dev lists them
    /// (`standard` when it does not know the model), then Ultra.
    static func levels(for values: [String]?) -> [EffortLevel] {
        [auto] + (values ?? standard).filter { $0 != ultra.value && !$0.isEmpty }.map { level(for: $0) } + [ultra]
    }

    /// The level for a config value; an unknown value gets a plain row of its own.
    static func level(for value: String?) -> EffortLevel {
        let value = value ?? ""
        return ([auto, ultra] + known).first { $0.value == value }
            ?? EffortLevel(value: value, title: value.prefix(1).uppercased() + value.dropFirst(), systemImage: Icons.effort, description: "The model's own setting")
    }

    /// Title colour: Ultra is `Theme.ultra`, the rest primary text.
    var tint: Color { isUltra ? Theme.ultra : Theme.textPrimary }
}

/// The composer's effort chip popover: Auto, the model's own effort values
/// (`EffortLevel.levels(for:)`), Ultra, one row each with its icon and a few
/// words, Ultra in purple; a current value the model does not offer gets a
/// row of its own. A click or Return applies it (`onSelect`) and closes;
/// ↑↓ move, Esc closes.
struct EffortPopover: View {
    let current: String?
    let offered: [EffortLevel]
    let onSelect: (String) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var highlighted: Int?
    @FocusState private var focused: Bool

    var body: some View {
        let levels = offered + (offered.contains { $0.value == (current ?? "") } ? [] : [EffortLevel.level(for: current)])
        VStack(alignment: .leading, spacing: 2) {
            ForEach(Array(levels.enumerated()), id: \.element.id) { index, level in
                row(level, isCurrent: level.value == (current ?? ""), isHighlighted: highlighted == index)
                    .onHover { inside in
                        if inside { highlighted = index }
                    }
            }
        }
        .padding(6)
        .frame(width: 280)
        .background(Theme.surface)
        .focusable()
        .focusEffectDisabled()
        .focused($focused)
        .onKeyPress(.upArrow) {
            move(-1, count: levels.count)
            return .handled
        }
        .onKeyPress(.downArrow) {
            move(1, count: levels.count)
            return .handled
        }
        .onKeyPress(.return) {
            if let highlighted, levels.indices.contains(highlighted) { choose(levels[highlighted]) }
            return .handled
        }
        .onKeyPress(.escape) {
            dismiss()
            return .handled
        }
        .onExitCommand { dismiss() }
        .task {
            highlighted = levels.firstIndex { $0.value == (current ?? "") }
            // Popover content is attached a beat after creation; focusing then sticks.
            try? await Task.sleep(for: .milliseconds(60))
            focused = true
        }
    }

    private func row(_ level: EffortLevel, isCurrent: Bool, isHighlighted: Bool) -> some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        return Button {
            choose(level)
        } label: {
            HStack(spacing: 10) {
                Image(systemName: level.systemImage)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(level.isUltra ? Theme.ultra : Theme.textSecondary)
                    .frame(width: 20)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 1) {
                    Text(level.title)
                        .font(.system(size: 13, weight: isCurrent || level.isUltra ? .semibold : .medium))
                        .foregroundStyle(level.tint)
                    Text(level.description)
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                        .lineLimit(1)
                }
                Spacer(minLength: 4)
                if isCurrent {
                    Image(systemName: "checkmark")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(level.isUltra ? Theme.ultra : Theme.accent)
                        .accessibilityHidden(true)
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 6)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(shape.fill(isHighlighted ? Theme.surfaceRaised : Color.clear))
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(level.title): \(level.description)")
        .accessibilityAddTraits(isCurrent ? .isSelected : [])
    }

    private func move(_ delta: Int, count: Int) {
        guard count > 0 else { return }
        highlighted = min(max((highlighted ?? -1) + delta, 0), count - 1)
    }

    private func choose(_ level: EffortLevel) {
        if level.value != (current ?? "") { onSelect(level.value) }
        dismiss()
    }
}

#Preview("Effort · Ultra") {
    EffortPopover(current: "ultra", offered: EffortLevel.levels(for: ["low", "medium", "high", "xhigh", "max"])) { _ in }
}

#Preview("Effort · Auto") {
    EffortPopover(current: nil, offered: EffortLevel.levels(for: nil)) { _ in }
}
