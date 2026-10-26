import AppKit
import SwiftTerm

/// One shell in the Inspector Terminal tab: the user's login shell on a
/// SwiftTerm `LocalProcessTerminalView`, started in the project root. The
/// view lives as long as the session, so switching tabs keeps scrollback and
/// running programs.
@MainActor
@Observable
final class TerminalSession: Identifiable {
    let id = UUID()
    /// "zsh 2" until the shell sets a title (OSC 0/2).
    let fallbackTitle: String
    private(set) var title: String?
    private(set) var currentDirectory: String?
    private(set) var isRunning = false

    @ObservationIgnored let view: LocalProcessTerminalView
    @ObservationIgnored private var delegate: Delegate?
    /// Called once the shell exits (also after `terminate()`).
    @ObservationIgnored var onExit: ((TerminalSession) -> Void)?

    var displayTitle: String { title ?? fallbackTitle }

    init(directory: String, number: Int) {
        let shell = Self.shellPath
        let name = (shell as NSString).lastPathComponent
        fallbackTitle = number > 1 ? "\(name) \(number)" : name
        currentDirectory = directory

        view = LocalProcessTerminalView(frame: NSRect(x: 0, y: 0, width: 320, height: 400))
        view.font = .monospacedSystemFont(ofSize: 11.5, weight: .regular)
        view.nativeBackgroundColor = NSColor.adaptive(light: Theme.Hex.canvasLight, dark: Theme.Hex.canvasDark)
        view.nativeForegroundColor = Theme.nsTextPrimary
        view.caretColor = NSColor.controlAccentColor
        view.optionAsMetaKey = false

        let delegate = Delegate(session: self)
        self.delegate = delegate
        view.processDelegate = delegate

        var environment = ProcessInfo.processInfo.environment
        environment["TERM"] = "xterm-256color"
        environment["COLORTERM"] = "truecolor"
        environment["TERM_PROGRAM"] = "ContextAgent"
        environment["LANG"] = environment["LANG"] ?? "en_US.UTF-8"
        environment["PWD"] = directory
        view.startProcess(
            executable: shell,
            args: [],
            environment: environment.map { "\($0.key)=\($0.value)" },
            execName: "-" + name,
            currentDirectory: directory
        )
        isRunning = true
    }

    private static var shellPath: String {
        if let entry = getpwuid(getuid()), let shell = entry.pointee.pw_shell {
            let path = String(cString: shell)
            if FileManager.default.isExecutableFile(atPath: path) { return path }
        }
        return "/bin/zsh"
    }

    /// Types `command` and Return; the pty buffers it until the shell reads.
    func run(_ command: String) {
        view.send(txt: command + "\n")
    }

    func terminate() {
        guard isRunning else { return }
        isRunning = false
        view.terminate()
    }

    fileprivate func titleChanged(_ title: String) {
        let trimmed = title.trimmingCharacters(in: .whitespacesAndNewlines)
        self.title = trimmed.isEmpty ? nil : trimmed
    }

    fileprivate func directoryChanged(_ directory: String?) {
        guard let directory else { return }
        // OSC 7 reports a file:// URL.
        currentDirectory = URL(string: directory)?.path ?? directory
    }

    fileprivate func exited() {
        isRunning = false
        onExit?(self)
        onExit = nil
    }

    /// SwiftTerm's delegate is a class protocol with non-isolated methods.
    private final class Delegate: LocalProcessTerminalViewDelegate {
        weak var session: TerminalSession?

        init(session: TerminalSession) {
            self.session = session
        }

        func sizeChanged(source: LocalProcessTerminalView, newCols: Int, newRows: Int) {}

        func setTerminalTitle(source: LocalProcessTerminalView, title: String) {
            DispatchQueue.main.async { [weak self] in
                MainActor.assumeIsolated { self?.session?.titleChanged(title) }
            }
        }

        func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {
            DispatchQueue.main.async { [weak self] in
                MainActor.assumeIsolated { self?.session?.directoryChanged(directory) }
            }
        }

        func processTerminated(source: TerminalView, exitCode: Int32?) {
            DispatchQueue.main.async { [weak self] in
                MainActor.assumeIsolated { self?.session?.exited() }
            }
        }
    }
}

/// A project's terminal tabs: the sessions in tab order and the selected one.
/// A shell that exits closes its tab.
@MainActor
@Observable
final class TerminalWorkspace {
    let directory: String
    private(set) var sessions: [TerminalSession] = []
    var selectedId: UUID?
    @ObservationIgnored private var opened = 0

    init(directory: String) {
        self.directory = directory
    }

    var selected: TerminalSession? {
        sessions.first { $0.id == selectedId } ?? sessions.last
    }

    /// Opens the first tab the first time the Terminal tab shows.
    func startIfNeeded() {
        guard opened == 0 else { return }
        newSession()
    }

    @discardableResult
    func newSession(running command: String? = nil) -> TerminalSession {
        opened += 1
        let session = TerminalSession(directory: directory, number: opened)
        if let command { session.run(command) }
        session.onExit = { [weak self] session in
            self?.remove(session)
        }
        sessions.append(session)
        selectedId = session.id
        return session
    }

    func close(_ session: TerminalSession) {
        session.terminate()
        remove(session)
    }

    func terminateAll() {
        sessions.forEach { $0.terminate() }
        sessions.removeAll()
        selectedId = nil
    }

    private func remove(_ session: TerminalSession) {
        guard let index = sessions.firstIndex(where: { $0.id == session.id }) else { return }
        sessions.remove(at: index)
        if selectedId == session.id {
            selectedId = sessions.isEmpty ? nil : sessions[min(index, sessions.count - 1)].id
        }
    }
}
