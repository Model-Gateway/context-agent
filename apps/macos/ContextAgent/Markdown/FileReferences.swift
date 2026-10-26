import SwiftUI
import AppKit

// File names in rendered markdown ("`index.html` — layout …") become small
// chips styled like the outcome card's "Modified files" chips (a neutral
// `Chip`: raised grey fill, file icon, secondary text): click opens the
// file in the Editor tab, ⌘-click opens it in the default editor, and the
// context menu offers those plus Show in Changes, Reveal in Finder and
// Copy Path.
// `MarkdownInline` finds the references; `InlineText` draws the chips over
// the text with `Text.LayoutKey`, so they wrap with the surrounding words.

/// Resolves file names to files in the project and performs the path
/// actions on them. `ChatView` provides one; without it `MarkdownView`
/// renders file names as plain text. Equal when the root is: the closures
/// are rebuilt with every `ChatView` body, and an environment value that
/// never compares equal would redraw every message in the chat.
struct FileReferenceContext: Equatable {
    /// Repository root.
    var root: String
    /// Repository-relative paths with changes: a bare `index.html` also
    /// matches a changed `web/index.html`.
    var changedPaths: () -> [String]
    var perform: (String, PathAction) -> Void

    static func == (lhs: Self, rhs: Self) -> Bool { lhs.root == rhs.root }

    /// The repository-relative (or absolute) path of the file `candidate`
    /// names, or `nil` when it is not a path or no such file exists.
    func resolve(_ candidate: String) -> String? {
        guard let path = Self.normalized(candidate) else { return nil }
        if path.hasPrefix("/") {
            return Self.isFile(path) ? path : nil
        }
        if Self.isFile((root as NSString).appendingPathComponent(path)) {
            return path
        }
        let suffix = "/" + path
        let changed = changedPaths().filter { $0.hasSuffix(suffix) }
        return changed.count == 1 ? changed[0] : nil
    }

    /// Drops `./`, a `:line[:column]` or `#L42` suffix and expands `~`;
    /// rejects anything with spaces, a scheme or code punctuation, and words
    /// with neither a slash nor an extension.
    private static func normalized(_ candidate: String) -> String? {
        var path = candidate.trimmingCharacters(in: .whitespaces)
        guard !path.isEmpty, path.count < 512,
              path.rangeOfCharacter(from: rejected) == nil,
              !path.contains("://") else { return nil }
        if let range = path.range(of: #"(:\d+){1,2}$|#L\d+(-L?\d+)?$"#, options: .regularExpression) {
            path.removeSubrange(range)
        }
        if path.hasPrefix("./") { path.removeFirst(2) }
        if path.hasPrefix("~/") { path = (path as NSString).expandingTildeInPath }
        guard path.contains("/") || !(path as NSString).pathExtension.isEmpty,
              !path.hasSuffix("/") else { return nil }
        return path
    }

    private static let rejected = CharacterSet.whitespacesAndNewlines.union(CharacterSet(charactersIn: "()[]{}<>=,;'\"*?|$`\\"))

    private static func isFile(_ path: String) -> Bool {
        var isDirectory: ObjCBool = false
        return FileManager.default.fileExists(atPath: path, isDirectory: &isDirectory) && !isDirectory.boolValue
    }
}

extension EnvironmentValues {
    @Entry var fileReferences: FileReferenceContext? = nil
}

/// Marks the runs of one file chip inside a combined `Text`.
struct FileReferenceAttribute: TextAttribute {
    let index: Int
}

/// Inline markdown with its file references: `text` carries a
/// `FileReferenceAttribute` on each chip, `paths[index]` is its file.
struct RenderedInline {
    var text: Text
    var paths: [String]
}

extension MarkdownInline {
    /// `render(_:font:)` with file references turned into chips (code spans,
    /// relative links and bare `name.ext` words that resolve to a file).
    /// Memoized per project root: resolving references checks the disk, and
    /// rows re-render whenever they scroll back into view.
    static func rendered(_ markdown: String, font: Font, files: FileReferenceContext?) -> RenderedInline {
        guard let files else { return RenderedInline(text: Text(render(markdown, font: font)), paths: []) }
        return renderedCache.value(for: RenderedKey(text: markdown, font: font, root: files.root)) {
            uncachedRendered(markdown, font: font, files: files)
        }
    }

    /// Forgets resolved references; call when the working tree changed
    /// (a file now exists, or a bare name now matches a changed path).
    static func invalidateFileReferences() {
        renderedCache.removeAll()
    }

    private struct RenderedKey: Hashable {
        var text: String
        var font: Font
        var root: String
    }

    private static let renderedCache = RenderCache<RenderedKey, RenderedInline>(countLimit: 2048)

    private static func uncachedRendered(_ markdown: String, font: Font, files: FileReferenceContext) -> RenderedInline {
        let attributed = render(markdown, font: font)
        let references = fileReferences(in: attributed, files: files)
        guard !references.isEmpty else { return RenderedInline(text: Text(attributed), paths: []) }

        var text = Text(verbatim: "")
        var cursor = attributed.startIndex
        for (index, reference) in references.enumerated() {
            if cursor < reference.range.lowerBound {
                text = Text("\(text)\(Text(AttributedString(attributed[cursor..<reference.range.lowerBound])))")
            }
            let name = String(attributed[reference.range].characters)
            text = Text("\(text)\(chip(name: name, path: reference.path, index: index))")
            cursor = reference.range.upperBound
        }
        if cursor < attributed.endIndex {
            text = Text("\(text)\(Text(AttributedString(attributed[cursor...])))")
        }
        return RenderedInline(text: text, paths: references.map(\.path))
    }

    /// Matches the neutral `Chip` the "Modified files" section uses; no-break
    /// spaces pad the name inside the chip without letting a line break
    /// separate the icon from it.
    private static func chip(name: String, path: String, index: Int) -> Text {
        let icon = Text(Image(systemName: Icons.file)).font(.system(size: 10, weight: .medium))
        return Text("\u{00A0}\(icon)\u{202F}\(Text(verbatim: name))\u{00A0}")
            .font(.system(size: 12, weight: .medium))
            .foregroundStyle(Theme.textSecondary)
            .customAttribute(FileReferenceAttribute(index: index))
    }

    private struct Reference {
        var range: Range<AttributedString.Index>
        var path: String
    }

    private static func fileReferences(in attributed: AttributedString, files: FileReferenceContext) -> [Reference] {
        var references: [Reference] = []
        for run in attributed.runs {
            let range = run.range
            let string = String(attributed[range].characters)
            if let intent = run.inlinePresentationIntent, intent.contains(.code) {
                if let path = files.resolve(string) {
                    references.append(Reference(range: range, path: path))
                }
            } else if let link = run.link {
                if link.scheme == nil || link.isFileURL, let path = files.resolve(link.isFileURL ? link.path : link.relativePath) {
                    references.append(Reference(range: range, path: path))
                }
            } else {
                for match in barePath.matches(in: string, range: NSRange(string.startIndex..., in: string)) {
                    guard let matchRange = Range(match.range, in: string),
                          let path = files.resolve(String(string[matchRange])) else { continue }
                    let lower = attributed.characters.index(range.lowerBound, offsetBy: string.distance(from: string.startIndex, to: matchRange.lowerBound))
                    let upper = attributed.characters.index(lower, offsetBy: string.distance(from: matchRange.lowerBound, to: matchRange.upperBound))
                    references.append(Reference(range: lower..<upper, path: path))
                }
            }
        }
        return references
    }

    /// `index.html`, `src/app.rs:42`, `~/notes.md` in running text; the
    /// extension starts with a letter so "v1.2" and "e.g" stay text (and
    /// only names that resolve to a file become chips).
    private static let barePath = try! NSRegularExpression(
        pattern: #"(?<![\w./~-])(?:~?/)?(?:[\w.-]+/)*[\w-][\w.-]*\.[A-Za-z][A-Za-z0-9]{0,9}(?::\d+(?::\d+)?)?(?![\w/-])"#
    )
}

// MARK: - Chips

/// One chip's frame on one line (a chip that wraps has two).
private struct FileChipFrame: Identifiable {
    let id: Int
    let index: Int
    let rect: CGRect
}

private enum FileChipLayout {
    static func frames(_ layouts: [Text.LayoutKey.AnchoredLayout], in proxy: GeometryProxy) -> [FileChipFrame] {
        var frames: [FileChipFrame] = []
        func append(_ index: Int, _ rect: CGRect) {
            frames.append(FileChipFrame(id: frames.count, index: index, rect: rect.insetBy(dx: -1, dy: -1.5)))
        }
        for anchored in layouts {
            let origin = proxy[anchored.origin]
            for line in anchored.layout {
                var current: (index: Int, rect: CGRect)?
                for run in line {
                    guard let attribute = run[FileReferenceAttribute.self] else {
                        if let open = current { append(open.index, open.rect) }
                        current = nil
                        continue
                    }
                    let rect = run.typographicBounds.rect.offsetBy(dx: origin.x, dy: origin.y)
                    if let open = current, open.index == attribute.index {
                        current = (open.index, open.rect.union(rect))
                    } else {
                        if let open = current { append(open.index, open.rect) }
                        current = (attribute.index, rect)
                    }
                }
                if let open = current { append(open.index, open.rect) }
            }
        }
        return frames
    }
}

extension View {
    /// Draws the ghost-button chips behind the file references of a
    /// `RenderedInline` text and puts their click / ⌘-click / context-menu
    /// targets over it.
    func fileReferenceChips(paths: [String], files: FileReferenceContext?, hovered: Binding<Int?>) -> some View {
        modifier(FileReferenceChipsModifier(paths: paths, files: files, hovered: hovered))
    }
}

private struct FileReferenceChipsModifier: ViewModifier {
    let paths: [String]
    let files: FileReferenceContext?
    @Binding var hovered: Int?

    func body(content: Content) -> some View {
        if let files, !paths.isEmpty {
            content
                .backgroundPreferenceValue(Text.LayoutKey.self) { layouts in
                    GeometryReader { proxy in
                        ForEach(FileChipLayout.frames(layouts, in: proxy)) { frame in
                            background(isHovered: hovered == frame.index)
                                .frame(width: frame.rect.width, height: frame.rect.height)
                                .position(x: frame.rect.midX, y: frame.rect.midY)
                        }
                    }
                }
                .overlayPreferenceValue(Text.LayoutKey.self) { layouts in
                    GeometryReader { proxy in
                        ForEach(FileChipLayout.frames(layouts, in: proxy)) { frame in
                            target(path: paths[frame.index], index: frame.index, files: files)
                                .frame(width: frame.rect.width, height: frame.rect.height)
                                .position(x: frame.rect.midX, y: frame.rect.midY)
                        }
                    }
                }
        } else {
            content
        }
    }

    private func background(isHovered: Bool) -> some View {
        RoundedRectangle(cornerRadius: 6, style: .continuous)
            .fill(isHovered ? Theme.border : Theme.surfaceRaised)
    }

    private func target(path: String, index: Int, files: FileReferenceContext) -> some View {
        Button {
            files.perform(path, NSEvent.modifierFlags.contains(.command) ? .openWithEditor : .openInEditor)
        } label: {
            Color.clear.contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { inside in
            if inside {
                hovered = index
                NSCursor.pointingHand.push()
            } else {
                if hovered == index { hovered = nil }
                NSCursor.pop()
            }
        }
        .contextMenu {
            Button("Open in Editor") { files.perform(path, .openInEditor) }
            Button("Show in Changes") { files.perform(path, .showInChanges) }
            Button("Open with Default Editor") { files.perform(path, .openWithEditor) }
            Divider()
            Button("Reveal in Finder") { files.perform(path, .revealInFinder) }
            Button("Copy Path") { files.perform(path, .copyPath) }
        }
        .help("\(path) — click to open in the Editor, ⌘-click to open with the default editor")
        .accessibilityLabel("File \(path)")
        .accessibilityHint("Opens the file in the Editor tab")
    }
}
