import SwiftUI
import AppKit

/// The card that opens under a timeline row: `codeBackground`, 14 pt radius,
/// *Arguments* | *Output* segments top-left with the format label, a
/// `CopyButton` top-right, capped monospace lines ("+ N more lines · Open
/// full output" opens a sheet with a read-only `NSTextView` and Save…), a
/// truncation footer when the backend cut the output, and `apply_patch`
/// edits rendered as a mini diff.
struct ToolOutputView: View {
    enum Segment: String, CaseIterable, Identifiable {
        case arguments = "Arguments"
        case output = "Output"
        var id: String { rawValue }
    }

    static let maxLines = 400
    static let maxBytes = 32_768
    static let maxHeight: CGFloat = 320
    /// Blocks up to this many lines sit inline in the transcript; longer
    /// ones get their own `maxHeight` scroll view. Counting lines instead of
    /// measuring the rendered block keeps the card out of the measure →
    /// resize loop and leaves short cards without a nested scroll view at
    /// all (a nested scroll view captures the wheel from the transcript).
    static let inlineLineLimit = 18

    var call: ToolCallItem

    @State private var chosenSegment: Segment?
    @State private var showingFull = false

    init(call: ToolCallItem) {
        self.call = call
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            header
            cappedContent
            if segment == .output, let output = call.output, output.omittedCharacters > 0 {
                Text("+ \(output.omittedCharacters.formatted()) more characters")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                    .help("The runtime truncated this output before the model saw it")
            }
        }
        .padding(.horizontal, 14)
        .padding(.top, 10)
        .padding(.bottom, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.codeBackground, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(Theme.border, lineWidth: 1)
        )
        .sheet(isPresented: $showingFull) {
            FullOutputSheet(title: sheetTitle, text: currentText, suggestedFileName: suggestedFileName)
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(ToolMetadata.rowLabel(for: call)) details")
    }

    // MARK: - Segments

    /// Output when present, Arguments while running or when nothing came back.
    private var segment: Segment {
        if let chosenSegment {
            if chosenSegment == .output, call.output == nil { return .arguments }
            return chosenSegment
        }
        return call.output != nil ? .output : .arguments
    }

    private var isPatch: Bool { call.name == "apply_patch" && !patchEdits.isEmpty }

    private var patchEdits: [PatchEdit] {
        PatchEdit.edits(from: call.arguments)
    }

    private var argumentsText: String {
        if isPatch { return PatchEdit.diffText(patchEdits) }
        return call.arguments.prettyPrinted()
    }

    private var currentText: String {
        switch segment {
        case .arguments: return argumentsText
        case .output: return call.output?.content ?? ""
        }
    }

    private var formatLabel: String {
        switch segment {
        case .arguments: return isPatch ? "diff" : "JSON"
        case .output: return call.output?.format.rawValue ?? "text"
        }
    }

    private var highlight: SyntaxHighlighter.Language? {
        switch segment {
        case .arguments:
            return isPatch ? .diff : .json
        case .output:
            switch call.output?.format {
            case .json?: return .json
            case .diff?: return .diff
            default: return nil
            }
        }
    }

    private var sheetTitle: String {
        "\(ToolMetadata.rowLabel(for: call)) · \(segment.rawValue)"
    }

    private var suggestedFileName: String {
        let base = call.name.replacingOccurrences(of: "_", with: "-")
        let ext: String
        switch formatLabel {
        case "JSON": ext = "json"
        case "diff": ext = "diff"
        default: ext = "txt"
        }
        return "\(base)-\(segment.rawValue.lowercased()).\(ext)"
    }

    // MARK: - Header

    private var header: some View {
        HStack(spacing: 10) {
            segmentPicker
            Text(formatLabel)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
                .lineLimit(1)
            if segment == .arguments, isPatch, let path = ToolMetadata.pathArgument(for: call) {
                Text("\(path) · \(patchEdits.count) edit\(patchEdits.count == 1 ? "" : "s")")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            if segment == .output, call.output?.isError == true {
                Chip("error", style: .tinted(Theme.danger))
            }
            Spacer(minLength: 8)
            if call.status == .running {
                HStack(spacing: 6) {
                    BrailleSpinner(size: 11)
                    Text("Running")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                }
            } else if let ms = call.durationMs {
                Text(TranscriptFormat.durationMs(ms))
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                    .monospacedDigit()
            }
            CopyButton(text: currentText, label: "Copy \(segment.rawValue.lowercased())")
        }
        .frame(minHeight: 28)
    }

    private var segmentPicker: some View {
        HStack(spacing: 2) {
            ForEach(Segment.allCases) { candidate in
                segmentButton(candidate)
            }
        }
        .padding(2)
        .background(Theme.surfaceRaised, in: RoundedRectangle(cornerRadius: 7, style: .continuous))
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Tool details")
    }

    private func segmentButton(_ candidate: Segment) -> some View {
        let selected = candidate == segment
        let available = candidate == .arguments || call.output != nil
        return Button {
            chosenSegment = candidate
        } label: {
            Text(candidate.rawValue)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(selected ? Theme.textPrimary : Theme.textSecondary)
                .padding(.horizontal, 8)
                .padding(.vertical, 3)
                .background(
                    RoundedRectangle(cornerRadius: 5, style: .continuous)
                        .fill(selected ? Theme.surface : Color.clear)
                        .shadow(color: Color.black.opacity(selected ? 0.06 : 0), radius: 1, x: 0, y: 1)
                )
                .contentShape(RoundedRectangle(cornerRadius: 5, style: .continuous))
        }
        .buttonStyle(.plain)
        .disabled(!available)
        .opacity(available ? 1 : 0.45)
        .help(available ? candidate.rawValue : "No output yet")
        .accessibilityLabel(candidate.rawValue)
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }

    // MARK: - Content

    /// The lines inline, or in a `maxHeight` scroll view once there are
    /// more than `inlineLineLimit` of them.
    @ViewBuilder
    private var cappedContent: some View {
        if currentText.isEmpty {
            Text(emptyMessage)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
                .padding(.vertical, 2)
        } else {
            let list = LineList(text: currentText, maxLines: Self.maxLines, maxBytes: Self.maxBytes, highlight: highlight) {
                showingFull = true
            }
            if list.lineCount > Self.inlineLineLimit {
                ScrollView(.vertical) {
                    list.padding(.trailing, 4)
                }
                .frame(height: Self.maxHeight)
                .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                list
                    .padding(.trailing, 4)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    private var emptyMessage: String {
        switch segment {
        case .arguments: return "No arguments"
        case .output: return call.status == .running ? "Waiting for output…" : "No output"
        }
    }
}

// MARK: - apply_patch edits

/// One `{old_text, new_text}` entry of an `apply_patch` call (`old`/`new` are accepted too).
private struct PatchEdit: Identifiable {
    let id: Int
    let oldText: String
    let newText: String

    static func edits(from arguments: JSONValue) -> [PatchEdit] {
        guard let list = arguments["edits"]?.arrayValue else { return [] }
        return list.enumerated().map { index, entry in
            PatchEdit(
                id: index,
                oldText: entry["old_text"]?.stringValue ?? entry["old"]?.stringValue ?? "",
                newText: entry["new_text"]?.stringValue ?? entry["new"]?.stringValue ?? ""
            )
        }
    }

    /// Unified-diff-like text (`@@ edit 1 of 2 @@`, `-old`, `+new`) for the diff highlighter.
    static func diffText(_ edits: [PatchEdit]) -> String {
        var out: [String] = []
        for edit in edits {
            if edits.count > 1 {
                out.append("@@ edit \(edit.id + 1) of \(edits.count) @@")
            }
            if edit.oldText.isEmpty {
                out.append("@@ insert @@")
            } else {
                for line in lines(of: edit.oldText) { out.append("-" + line) }
            }
            if edit.newText.isEmpty {
                out.append("@@ delete @@")
            } else {
                for line in lines(of: edit.newText) { out.append("+" + line) }
            }
        }
        return out.joined(separator: "\n")
    }

    private static func lines(of text: String) -> [String] {
        var parts = text.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        if parts.count > 1, parts.last?.isEmpty == true { parts.removeLast() }
        return parts
    }
}

// MARK: - Full output sheet

/// "Open full output": the whole text in a read-only `NSTextView` with Copy, Save… and Close.
private struct FullOutputSheet: View {
    let title: String
    let text: String
    let suggestedFileName: String

    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Text(title)
                    .font(Theme.Font.heading)
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: 8)
                Text(sizeLabel)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                CopyButton(text: text, label: "Copy all")
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .hairline(.bottom)

            ReadOnlyTextView(text: text)
                .background(Theme.codeBackground)

            HStack(spacing: 10) {
                Spacer()
                Button("Save…") { save() }
                    .buttonStyle(.bordered)
                Button("Close") { dismiss() }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.cancelAction)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .hairline(.top)
        }
        .frame(width: 760, height: 540)
        .background(Theme.surface)
    }

    private var sizeLabel: String {
        let lineCount = text.split(separator: "\n", omittingEmptySubsequences: false).count
        let bytes = text.utf8.count
        let size = bytes < 1024 ? "\(bytes) B" : String(format: "%.1f KB", Double(bytes) / 1024)
        return "\(lineCount.formatted()) line\(lineCount == 1 ? "" : "s") · \(size)"
    }

    private func save() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = suggestedFileName
        panel.canCreateDirectories = true
        panel.title = "Save Output"
        let content = text
        Task { @MainActor in
            let response = await panel.begin()
            guard response == .OK, let url = panel.url else { return }
            try? content.write(to: url, atomically: true, encoding: .utf8)
        }
    }
}

/// Read-only, selectable monospace text in a scrolling `NSTextView`.
private struct ReadOnlyTextView: NSViewRepresentable {
    let text: String

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = NSTextView.scrollableTextView()
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = false
        scrollView.autohidesScrollers = true
        scrollView.borderType = .noBorder
        scrollView.drawsBackground = false
        if let textView = scrollView.documentView as? NSTextView {
            textView.isEditable = false
            textView.isSelectable = true
            textView.isRichText = false
            textView.usesFindBar = true
            textView.isIncrementalSearchingEnabled = true
            textView.drawsBackground = false
            textView.font = Theme.Font.nsMono
            textView.textColor = Theme.nsTextPrimary
            textView.textContainerInset = NSSize(width: 12, height: 12)
            textView.isAutomaticQuoteSubstitutionEnabled = false
            textView.isAutomaticDashSubstitutionEnabled = false
            textView.isAutomaticTextReplacementEnabled = false
            textView.string = text
        }
        return scrollView
    }

    func updateNSView(_ nsView: NSScrollView, context: Context) {
        guard let textView = nsView.documentView as? NSTextView, textView.string != text else { return }
        textView.string = text
    }
}

// MARK: - Previews

#Preview("Tool output") {
    let json = "{\n  \"taskId\": \"11\",\n  \"status\": \"completed\"\n}"
    var finished = ToolCallItem(callId: "call_1", name: "execute_command", arguments: .object(["command": .string("cargo test"), "timeout_secs": .number(120)]))
    finished.status = .succeeded
    finished.summary = "exit 0"
    finished.durationMs = 2140
    finished.output = ToolOutput(content: json, fullLength: json.count, isError: false, tool: "execute_command")

    var running = ToolCallItem(callId: "call_2", name: "read_file", arguments: .object(["path": .string("src/app.rs"), "start_line": .number(1), "end_line": .number(40)]))
    running.status = .running

    var patch = ToolCallItem(callId: "call_3", name: "apply_patch", arguments: .object([
        "path": .string("src/app.rs"),
        "edits": .array([
            .object(["old_text": .string("let x = 1;\nlet y = 2;"), "new_text": .string("let x = 10;\nlet y = 20;\nlet z = x + y;")]),
            .object(["old_text": .string("// TODO"), "new_text": .string("")]),
        ]),
    ]))
    patch.status = .succeeded
    patch.durationMs = 18

    let long = (1...60).map { "line \($0): compiling crate ctx-core … ok" }.joined(separator: "\n")
    var truncated = ToolCallItem(callId: "call_4", name: "execute_command", arguments: .object(["command": .string("cargo build")]))
    truncated.status = .failed
    truncated.summary = "exit 101"
    truncated.durationMs = 9_800
    truncated.output = ToolOutput(content: long, fullLength: long.count + 12_480, isError: true, tool: "execute_command")

    return ScrollView {
        VStack(alignment: .leading, spacing: 16) {
            ToolOutputView(call: finished)
            ToolOutputView(call: running)
            ToolOutputView(call: patch)
            ToolOutputView(call: truncated)
        }
        .padding(24)
        .frame(width: 640)
    }
    .frame(height: 900)
    .background(Theme.canvas)
    .environment(AppState())
}
