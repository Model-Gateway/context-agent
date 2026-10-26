import Foundation
import SwiftUI

// The `ctx serve` child: Process + three pipes, a strict-LF line reader for
// stdout, a stderr ring buffer and an awaitable exit signal.

/// Debug logging for the backend layer only; silent unless `CTX_DESKTOP_DEBUG`
/// is set in the app's environment. Views never log.
enum BackendLog {
    static let isEnabled: Bool = ProcessInfo.processInfo.environment["CTX_DESKTOP_DEBUG"] != nil

    static func debug(_ message: @autoclosure () -> String) {
        guard isEnabled else { return }
        FileHandle.standardError.write(Data(("[backend] " + message() + "\n").utf8))
    }
}

/// Splits a byte stream strictly on 0x0A. U+2028, NEL and friends never end
/// a line, which is why `AsyncLineSequence` is not used anywhere here.
private struct LineSplitter {
    /// The unfinished line: never holds an LF between calls.
    private var buffer = Data()

    /// Appends bytes and returns every complete line (without the LF).
    /// Only the new bytes are searched: rescanning the buffered part on
    /// every pipe read made a multi-megabyte frame (a long session's
    /// history) cost seconds to minutes and held up every frame behind it.
    mutating func append(_ data: Data) -> [Data] {
        var lines: [Data] = []
        data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            guard let base = raw.baseAddress else { return }
            var offset = 0
            while offset < raw.count, let hit = memchr(base + offset, 0x0A, raw.count - offset) {
                let end = UnsafeRawPointer(hit) - base
                if buffer.isEmpty {
                    lines.append(Data(bytes: base + offset, count: end - offset))
                } else {
                    buffer.append(base.assumingMemoryBound(to: UInt8.self) + offset, count: end - offset)
                    lines.append(buffer)
                    buffer = Data()
                }
                offset = end + 1
            }
            if offset < raw.count {
                buffer.append(base.assumingMemoryBound(to: UInt8.self) + offset, count: raw.count - offset)
            }
        }
        return lines
    }

    /// Whatever is left without a trailing LF (EOF).
    mutating func drain() -> Data? {
        defer { buffer.removeAll() }
        return buffer.isEmpty ? nil : buffer
    }
}

/// One-shot exit code: fires once, any number of waiters, cancellation-aware.
private final class ExitSignal: @unchecked Sendable {
    private let lock = NSLock()
    private var code: Int32?
    private var waiters: [UUID: CheckedContinuation<Int32?, Never>] = [:]

    var value: Int32? {
        lock.lock(); defer { lock.unlock() }
        return code
    }

    func fire(_ newCode: Int32) {
        lock.lock()
        guard code == nil else { lock.unlock(); return }
        code = newCode
        let waiting = waiters
        waiters.removeAll()
        lock.unlock()
        for (_, continuation) in waiting { continuation.resume(returning: newCode) }
    }

    /// Resolves with the exit code, or nil when the waiting task is cancelled.
    func wait() async -> Int32? {
        let id = UUID()
        return await withTaskCancellationHandler {
            await withCheckedContinuation { (continuation: CheckedContinuation<Int32?, Never>) in
                lock.lock()
                if let code {
                    lock.unlock()
                    continuation.resume(returning: code)
                    return
                }
                if Task.isCancelled {
                    lock.unlock()
                    continuation.resume(returning: nil)
                    return
                }
                waiters[id] = continuation
                lock.unlock()
            }
        } onCancel: {
            lock.lock()
            let waiter = waiters.removeValue(forKey: id)
            lock.unlock()
            waiter?.resume(returning: nil)
        }
    }
}

/// Wraps one `ctx serve` process. Not thread-safe for configuration: set the
/// callbacks before `launch()`; everything after that is safe from any thread.
final class BackendProcess: @unchecked Sendable {
    static let stderrRingCapacity = 500

    let binary: URL
    let arguments: [String]
    let environment: [String: String]

    /// Complete stdout lines, strictly LF-framed, UTF-8 decoded, in order.
    /// Finishes when stdout reaches EOF (or `forceCloseStdout()` is called).
    let stdoutLines: AsyncStream<String>
    /// Called off the main thread with each batch of stderr lines.
    var onStderrLines: (@Sendable ([String]) -> Void)?

    private let process = Process()
    private let stdinPipe = Pipe()
    private let stdoutPipe = Pipe()
    private let stderrPipe = Pipe()
    private let stdoutContinuation: AsyncStream<String>.Continuation
    private var stdoutSplitter = LineSplitter()
    private var stderrSplitter = LineSplitter()
    private let stderrLock = NSLock()
    private var stderrRing: [String] = []
    private let exitSignal = ExitSignal()
    private let stdoutClosedLock = NSLock()
    private var stdoutClosed = false

    static let ignoreSigpipeOnce: Void = {
        signal(SIGPIPE, SIG_IGN)
    }()

    init(binary: URL, arguments: [String], environment: [String: String]) {
        self.binary = binary
        self.arguments = arguments
        self.environment = environment
        let (stream, continuation) = AsyncStream<String>.makeStream(bufferingPolicy: .unbounded)
        stdoutLines = stream
        stdoutContinuation = continuation
    }

    /// The handle `RPCConnection` writes requests to.
    var stdinHandle: FileHandle { stdinPipe.fileHandleForWriting }

    var processIdentifier: Int32 { process.processIdentifier }
    var isRunning: Bool { process.isRunning }
    /// Exit code once the process has terminated (signal number for uncaught signals).
    var exitCode: Int32? { exitSignal.value }

    /// Spawns the child. Throws when the executable cannot be launched.
    func launch() throws {
        _ = BackendProcess.ignoreSigpipeOnce
        process.executableURL = binary
        process.arguments = arguments
        process.environment = environment
        process.standardInput = stdinPipe
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe

        stdoutPipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
            guard let self else { handle.readabilityHandler = nil; return }
            let data = handle.availableData
            if data.isEmpty {
                handle.readabilityHandler = nil
                if let rest = self.stdoutSplitter.drain() {
                    BackendLog.debug("stdout EOF with \(rest.count) unframed bytes dropped")
                }
                self.finishStdout()
                try? handle.close()
                return
            }
            for line in self.stdoutSplitter.append(data) {
                if let text = String(data: line, encoding: .utf8) {
                    self.stdoutContinuation.yield(text)
                } else {
                    BackendLog.debug("dropped non-UTF-8 stdout line (\(line.count) bytes)")
                }
            }
        }

        stderrPipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
            guard let self else { handle.readabilityHandler = nil; return }
            let data = handle.availableData
            if data.isEmpty {
                handle.readabilityHandler = nil
                if let rest = self.stderrSplitter.drain() {
                    self.recordStderr([String(decoding: rest, as: UTF8.self)])
                }
                try? handle.close()
                return
            }
            let lines = self.stderrSplitter.append(data).map { String(decoding: $0, as: UTF8.self) }
            if !lines.isEmpty { self.recordStderr(lines) }
        }

        process.terminationHandler = { [weak self] finished in
            let code = finished.terminationStatus
            BackendLog.debug("ctx terminated: status \(code) reason \(finished.terminationReason == .exit ? "exit" : "signal")")
            self?.exitSignal.fire(code)
        }

        do {
            try process.run()
        } catch {
            stdoutPipe.fileHandleForReading.readabilityHandler = nil
            stderrPipe.fileHandleForReading.readabilityHandler = nil
            finishStdout()
            throw error
        }
        BackendLog.debug("launched \(binary.path) \(arguments.joined(separator: " ")) pid \(process.processIdentifier)")
    }

    /// Resolves with the exit code once the process has terminated.
    func waitForExit() async -> Int32? {
        await exitSignal.wait()
    }

    /// Like `waitForExit()`, but gives up after `timeout` seconds (nil).
    func waitForExit(timeout: TimeInterval) async -> Int32? {
        if let code = exitSignal.value { return code }
        return await withTaskGroup(of: Int32?.self) { group in
            group.addTask { await self.exitSignal.wait() }
            group.addTask {
                try? await Task.sleep(nanoseconds: UInt64(max(0, timeout) * 1_000_000_000))
                return nil
            }
            let first = await group.next() ?? nil
            group.cancelAll()
            return first
        }
    }

    /// SIGTERM.
    func terminate() {
        guard process.isRunning else { return }
        process.terminate()
    }

    /// SIGKILL.
    func forceKill() {
        guard process.isRunning else { return }
        Darwin.kill(process.processIdentifier, SIGKILL)
    }

    /// Ends the stdout stream even if a grandchild still holds the pipe open.
    func forceCloseStdout() {
        stdoutPipe.fileHandleForReading.readabilityHandler = nil
        finishStdout()
        try? stdoutPipe.fileHandleForReading.close()
    }

    /// Closes our end of stdin; called after exit so nothing leaks.
    func closeStdin() {
        try? stdinPipe.fileHandleForWriting.close()
    }

    /// The last `lines` stderr lines, joined.
    func stderrTail(lines: Int = 40) -> String {
        stderrLock.lock(); defer { stderrLock.unlock() }
        return stderrRing.suffix(lines).joined(separator: "\n")
    }

    /// All buffered stderr lines (at most `stderrRingCapacity`).
    var stderrLines: [String] {
        stderrLock.lock(); defer { stderrLock.unlock() }
        return stderrRing
    }

    private func finishStdout() {
        stdoutClosedLock.lock()
        let alreadyClosed = stdoutClosed
        stdoutClosed = true
        stdoutClosedLock.unlock()
        if !alreadyClosed { stdoutContinuation.finish() }
    }

    private func recordStderr(_ lines: [String]) {
        stderrLock.lock()
        stderrRing.append(contentsOf: lines)
        if stderrRing.count > BackendProcess.stderrRingCapacity {
            stderrRing.removeFirst(stderrRing.count - BackendProcess.stderrRingCapacity)
        }
        stderrLock.unlock()
        onStderrLines?(lines)
    }
}
