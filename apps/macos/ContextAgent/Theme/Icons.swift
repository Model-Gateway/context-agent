import SwiftUI

/// Every SF Symbol name used in the app, in one place.
enum Icons {
    static let welcome = "hand.wave"
    static let newChat = "plus"
    static let projects = "folder.badge.plus"
    static let changes = "arrow.triangle.branch"
    static let project = "cube"
    static let inspector = "sidebar.right"
    static let settings = "gearshape"
    static let search = "magnifyingglass"
    static let filter = "line.3.horizontal.decrease"
    static let send = "arrow.up"
    static let stop = "stop.fill"
    static let queued = "clock"
    static let attach = "plus"
    static let model = "asterisk"
    static let copy = "doc.on.doc"
    static let check = "checkmark.circle"
    static let failed = "xmark.circle"
    static let stopped = "stop.circle"
    static let approval = "hand.raised"
    static let permissionAuto = "bolt"
    static let permissionBypass = "exclamationmark.shield"
    static let effort = "gauge.with.dots.needle.50percent"
    /// Sub-agents (the Ultra effort tier).
    static let agents = "point.3.connected.trianglepath.dotted"
    static let agentExplore = "magnifyingglass"
    static let agentImplement = "hammer"
    static let validationPass = "checkmark.seal"
    static let validationFail = "xmark.seal"
    static let chevronDown = "chevron.down"
    static let chevronRight = "chevron.right"
    static let folder = "folder"
    static let branch = "arrow.triangle.branch"
    static let pin = "pin"
    static let close = "xmark"
    static let refresh = "arrow.clockwise"
    static let warning = "exclamationmark.triangle"
    static let info = "info.circle"
    static let thinking = "brain"
    static let file = "doc.text"
    static let editor = "chevron.left.forwardslash.chevron.right"
    static let save = "square.and.arrow.down"
    static let terminal = "terminal"
    /// The Inspector Browser tab and the `browser` tool.
    static let browser = "globe"
    static let mcp = "puzzlepiece.extension"
    static let apps = "square.grid.2x2"
    static let run = "play"
    static let memory = "brain.head.profile"
    static let context = "text.magnifyingglass"
    static let prompt = "doc.plaintext"

    // Used by the shared components.
    static let copied = "checkmark"
    static let clear = "xmark.circle.fill"
    static let openFull = "arrow.up.forward.square"

    // Names the views used to write inline.
    static let chat = "bubble.left"
    static let pinned = "pin.fill"
    static let key = "key"
    static let doctor = "stethoscope"
    static let chart = "chart.bar"
    static let trash = "trash"
    static let follow = "arrow.down.to.line"
    static let chevronUp = "chevron.up"
    static let arrowDown = "arrow.down"
    static let addCustom = "plus.circle"
    static let fix = "wrench.and.screwdriver"
    static let refactor = "arrow.triangle.2.circlepath"
    static let pricing = "dollarsign.circle"
    static let byok = "key"
    static let spending = "gauge.with.dots.needle.33percent"
    static let billing = "creditcard"
    static let appearance = "circle.lefthalf.filled"
    static let radioOn = "largecircle.fill.circle"
    static let radioOff = "circle"
    static let externalLink = "arrow.up.forward"
    static let edit = "pencil"
    static let undo = "arrow.uturn.backward"
    // Changes tab git actions
    static let stage = "plus"
    static let unstage = "minus"
    static let discard = "arrow.uturn.backward"
    static let commit = "checkmark.circle"
    static let push = "arrow.up.circle"
    static let pull = "arrow.down.circle"
    static let fetch = "arrow.triangle.2.circlepath"
    static let generate = "sparkles"
    static let commits = "clock.arrow.circlepath"
    static let newBranch = "plus.square.on.square"
    // Account (ModelGateway)
    static let account = "person.crop.circle"
    static let signIn = "person.badge.key"
    static let signOut = "rectangle.portrait.and.arrow.right"
    static let gateway = "network"
    static let usage = "square.grid.3x3.fill"
    static let plan = "creditcard"
    static let contextCredit = "gift"
    static let invite = "person.2"
    static let worktree = "arrow.branch"
    /// A machine reached over SSH (Settings → Remote hosts, the sidebar).
    static let remoteHost = "server.rack"
    static let remoteProject = "externaldrive.connected.to.line.below"

    /// The glyph of a `suggest/starters` kind (empty chat suggestions).
    static func starter(_ kind: String) -> String {
        switch kind {
        case "fix": return fix
        case "explain": return context
        case "refactor": return refactor
        case "test": return validationPass
        case "docs": return file
        case "review": return "eye"
        case "feature": return generate
        case "cleanup": return "scissors"
        default: return "text.bubble"
        }
    }

    /// The file-type glyph for a path, by extension (file chips in chat).
    static func fileType(_ path: String) -> String {
        switch (path as NSString).pathExtension.lowercased() {
        case "html", "htm", "xhtml", "xml", "svg", "vue", "svelte", "jsx", "tsx", "astro":
            return "chevron.left.forwardslash.chevron.right"
        case "swift":
            return "swift"
        case "css", "scss", "sass", "less":
            return "paintbrush"
        case "md", "markdown", "mdx", "rst":
            return "doc.richtext"
        case "txt", "log":
            return "doc.plaintext"
        case "json", "jsonc":
            return "curlybraces.square"
        case "toml", "yaml", "yml", "ini", "cfg", "conf", "plist", "env", "lock":
            return "slider.horizontal.3"
        case "sh", "bash", "zsh", "fish", "ps1":
            return "terminal"
        case "png", "jpg", "jpeg", "gif", "webp", "heic", "avif", "ico", "bmp", "tiff":
            return "photo"
        case "csv", "tsv", "xlsx":
            return "tablecells"
        case "sql", "db", "sqlite":
            return "cylinder"
        case "rs", "py", "js", "mjs", "cjs", "ts", "go", "c", "h", "cc", "cpp", "hpp", "m", "mm",
             "java", "kt", "kts", "rb", "php", "cs", "scala", "lua", "dart", "zig", "ex", "exs", "hs":
            return "curlybraces"
        default:
            return file
        }
    }

    /// The Context mark, 22 pt `textPrimary` by default.
    struct LogoView: View {
        var size: CGFloat

        init(size: CGFloat = 22) {
            self.size = size
        }

        var body: some View {
            LogoShape()
                .fill(Theme.textPrimary)
                .frame(width: size, height: size)
                .accessibilityElement()
                .accessibilityLabel("Context")
        }
    }

    /// The mark's five facets: two tall arms and two lower wedges around an
    /// open hexagonal core. Traced on an 885-unit square; the right half
    /// mirrors the left about x = 442.5.
    struct LogoShape: Shape {
        private static let unit: CGFloat = 885
        private static let arm: [CGPoint] = [
            CGPoint(x: 420, y: 0), CGPoint(x: 261, y: 116), CGPoint(x: 261, y: 388.5),
            CGPoint(x: 34.5, y: 528), CGPoint(x: 8.5, y: 744.5), CGPoint(x: 330, y: 546.5),
            CGPoint(x: 331.5, y: 459), CGPoint(x: 420, y: 395),
        ]
        private static let wedge: [CGPoint] = [
            CGPoint(x: 420, y: 640), CGPoint(x: 348, y: 593), CGPoint(x: 34.5, y: 790),
            CGPoint(x: 241.5, y: 885), CGPoint(x: 420, y: 771.5),
        ]

        func path(in rect: CGRect) -> Path {
            let side = min(rect.width, rect.height)
            let scale = side / Self.unit
            let origin = CGPoint(x: rect.midX - side / 2, y: rect.midY - side / 2)
            var path = Path()
            for polygon in [Self.arm, Self.wedge] {
                for mirrored in [false, true] {
                    let points = polygon.map { point in
                        CGPoint(
                            x: origin.x + (mirrored ? Self.unit - point.x : point.x) * scale,
                            y: origin.y + point.y * scale
                        )
                    }
                    path.addLines(points)
                    path.closeSubpath()
                }
            }
            return path
        }
    }
}

#Preview("Icons") {
    let names: [String] = [
        Icons.welcome, Icons.newChat, Icons.projects, Icons.changes, Icons.project, Icons.inspector,
        Icons.settings, Icons.search, Icons.send, Icons.stop, Icons.queued, Icons.copy, Icons.check,
        Icons.failed, Icons.stopped, Icons.approval, Icons.validationPass, Icons.validationFail, Icons.chevronDown,
        Icons.chevronRight, Icons.folder, Icons.pin, Icons.close, Icons.refresh, Icons.warning, Icons.info,
        Icons.thinking, Icons.file, Icons.terminal, Icons.memory, Icons.context,
    ]
    return VStack(alignment: .leading, spacing: 12) {
        HStack(spacing: 12) {
            Icons.LogoView()
            Icons.LogoView(size: 32)
        }
        LazyVGrid(columns: Array(repeating: GridItem(.fixed(28)), count: 12), spacing: 8) {
            ForEach(names, id: \.self) { name in
                Image(systemName: name).font(.system(size: 16)).foregroundStyle(Theme.textSecondary)
            }
        }
    }
    .padding(24)
    .background(Theme.canvas)
}
