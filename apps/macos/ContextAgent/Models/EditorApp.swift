import AppKit

/// An application that can open source files (Settings → General
/// "Default editor").
struct EditorApp: Identifiable, Hashable {
    let url: URL

    /// `nil` when nothing is installed at `url` any more.
    init?(url: URL) {
        guard FileManager.default.fileExists(atPath: url.path) else { return nil }
        self.url = url
    }

    var id: String { url.path }

    /// "Visual Studio Code", without the ".app".
    var name: String {
        let display = FileManager.default.displayName(atPath: url.path)
        return display.hasSuffix(".app") ? String(display.dropLast(4)) : display
    }

    var icon: NSImage {
        let image = NSWorkspace.shared.icon(forFile: url.path)
        image.size = NSSize(width: 16, height: 16)
        return image
    }

    /// Well-known editors, in menu order; only installed ones are listed.
    private static let knownBundleIds = [
        "com.microsoft.VSCode",
        "com.microsoft.VSCodeInsiders",
        "com.todesktop.230313mzl4w4u92", // Cursor
        "com.exafunction.windsurf",
        "dev.zed.Zed",
        "com.apple.dt.Xcode",
        "com.sublimetext.4",
        "com.panic.Nova",
        "com.barebones.bbedit",
        "com.jetbrains.intellij",
        "com.jetbrains.fleet",
        "com.vscodium",
        "com.apple.TextEdit",
    ]

    static func installed() -> [EditorApp] {
        var seen = Set<String>()
        return knownBundleIds.compactMap { id in
            guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: id),
                  seen.insert(url.path).inserted else { return nil }
            return EditorApp(url: url)
        }
    }
}
