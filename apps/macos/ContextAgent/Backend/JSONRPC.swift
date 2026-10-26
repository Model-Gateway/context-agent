import Foundation
import SwiftUI

// JSON-RPC 2.0 over newline-delimited frames (docs/desktop-protocol.md).
// `RPCConnection` owns request ids, the pending continuations and the
// write side of the child's stdin; `BackendProcess` feeds it complete lines.

// MARK: - Wire types

/// One decoded line from the backend. A response carries `id` plus `result`
/// or `error`; a notification carries `method` and no `id`.
struct Frame: Decodable {
    var id: JSONValue?
    var method: String?
    var params: JSONValue?
    var result: JSONValue?
    var error: RPCErrorBody?
}

struct RPCErrorBody: Decodable {
    var code: Int
    var message: String
    var data: JSONValue?

    /// `data.kind`: the backend's `CtxError::code()` when present.
    var kind: String? { data?["kind"]?.stringValue }
}

/// What we write. `params` is omitted entirely when nil.
struct RPCRequest: Encodable {
    var jsonrpc = "2.0"
    var id: Int
    var method: String
    var params: JSONValue?
}

/// A server → client notification as handed to `BackendClient`. `raw` is the
/// whole frame so typed params can be decoded straight from the line bytes.
struct RPCNotification {
    var method: String
    var params: JSONValue?
    var raw: Data
}

// MARK: - RPCConnection

/// Serialises every write to the child and matches responses to callers.
/// Lines arrive through `receive(line:)` in the order the process wrote them
/// (the reader task awaits each call, so ordering is preserved).
actor RPCConnection {
    private let output: FileHandle
    private let onNotification: @Sendable (RPCNotification) -> Void
    private var nextId = 1
    private var pending: [Int: CheckedContinuation<JSONValue, Error>] = [:]
    private var closedError: BackendError?
    private let encoder: JSONEncoder = {
        let e = JSONEncoder()
        e.outputFormatting = [.withoutEscapingSlashes]
        return e
    }()
    private let decoder = JSONDecoder()

    /// - Parameters:
    ///   - output: the write end of the child's stdin.
    ///   - onNotification: called on the actor for every notification frame; must not block.
    init(output: FileHandle, onNotification: @escaping @Sendable (RPCNotification) -> Void) {
        self.output = output
        self.onNotification = onNotification
    }

    var pendingCount: Int { pending.count }
    var isClosed: Bool { closedError != nil }

    /// Sends a request and waits for its response. Cancelling the calling
    /// task fails the request with `.transport("request cancelled")`; the
    /// late response (if any) is then ignored.
    func call(_ method: String, params: JSONValue? = nil) async throws -> JSONValue {
        if let closedError { throw closedError }
        let id = nextId
        nextId += 1
        var data: Data
        do {
            data = try encoder.encode(RPCRequest(id: id, method: method, params: params))
        } catch {
            throw BackendError.decoding("could not encode \(method): \(error.localizedDescription)")
        }
        data.append(0x0A)
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<JSONValue, Error>) in
                if let closedError {
                    continuation.resume(throwing: closedError)
                    return
                }
                pending[id] = continuation
                do {
                    try output.write(contentsOf: data)
                } catch {
                    pending.removeValue(forKey: id)
                    continuation.resume(throwing: BackendError.transport("write to ctx failed: \(error.localizedDescription)"))
                }
            }
        } onCancel: {
            Task { await self.cancel(id: id) }
        }
    }

    /// Fails one pending request (used by task cancellation).
    func cancel(id: Int) {
        guard let continuation = pending.removeValue(forKey: id) else { return }
        continuation.resume(throwing: BackendError.transport("request cancelled"))
    }

    /// One complete stdout line (without its trailing LF), already UTF-8 decoded.
    func receive(line: String) {
        let data = Data(line.utf8)
        let frame: Frame
        do {
            frame = try decoder.decode(Frame.self, from: data)
        } catch {
            BackendLog.debug("dropped undecodable frame: \(error.localizedDescription) — \(line.prefix(200))")
            return
        }
        if let id = frame.id?.intValue {
            guard let continuation = pending.removeValue(forKey: id) else {
                BackendLog.debug("response for unknown id \(id)")
                return
            }
            if let error = frame.error {
                continuation.resume(throwing: BackendError.rpc(code: error.code, message: error.message, kind: error.kind))
            } else {
                continuation.resume(returning: frame.result ?? .null)
            }
        } else if let method = frame.method {
            onNotification(RPCNotification(method: method, params: frame.params, raw: data))
        } else {
            BackendLog.debug("frame with neither id nor method: \(line.prefix(200))")
        }
    }

    /// Fails every pending request and refuses new ones (process exit).
    func close(with error: BackendError) {
        closedError = error
        let waiting = pending
        pending.removeAll()
        for (_, continuation) in waiting {
            continuation.resume(throwing: error)
        }
    }
}
