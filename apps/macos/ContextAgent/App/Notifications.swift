import SwiftUI
import AppKit
import Foundation
import UserNotifications

/// User notifications for finished tasks and pending approvals, plus the dock
/// badge. `UNUserNotificationCenter` aborts in a process without a bundle
/// identifier (e.g. `swift run` / tests), so everything is guarded by
/// `isAvailable`; authorization is requested lazily on the first post.
@MainActor
final class TaskNotifier: NSObject {
    static let isAvailable: Bool = Bundle.main.bundleIdentifier != nil

    enum Category {
        static let taskFinished = "dev.contextagent.task-finished"
        static let approval = "dev.contextagent.approval"
    }

    enum UserInfoKey {
        static let projectId = "projectId"
        static let sessionId = "sessionId"
    }

    private enum Authorization {
        case unknown, requesting, granted, denied
    }

    /// Set by AppState: selects the project + session the notification points at.
    var onOpen: ((_ projectId: String, _ sessionId: String) -> Void)?

    private var authorization: Authorization = .unknown
    private var queuedRequests: [UNNotificationRequest] = []

    override init() {
        super.init()
        guard Self.isAvailable else { return }
        UNUserNotificationCenter.current().delegate = self
    }

    // MARK: - Posting

    /// "Task completed · <chat title>" (or failed / blocked / cancelled).
    func notifyTaskFinished(status: TaskStatus?, error: String?, chatTitle: String, projectId: String, sessionId: String) {
        let word: String
        switch status {
        case .completed?: word = "completed"
        case .blocked?: word = "blocked"
        case .cancelled?: word = "cancelled"
        case .failed?, nil: word = "failed"
        }
        let title = "Task \(word) · \(chatTitle)"
        let body = error ?? ""
        post(title: title, body: body, category: Category.taskFinished, projectId: projectId, sessionId: sessionId, identifier: "task-\(sessionId)")
    }

    /// "Approval needed: <summary>".
    func notifyApprovalNeeded(summary: String, chatTitle: String, projectId: String, sessionId: String) {
        post(title: "Approval needed: \(summary)", body: chatTitle, category: Category.approval, projectId: projectId, sessionId: sessionId, identifier: "approval-\(sessionId)")
    }

    /// "Question: <first question>" from the agent's `ask_user` tool.
    func notifyQuestionAsked(question: String, chatTitle: String, projectId: String, sessionId: String) {
        post(title: "Question: \(question)", body: chatTitle, category: Category.approval, projectId: projectId, sessionId: sessionId, identifier: "question-\(sessionId)")
    }

    /// Dock badge: unseen chats + pending approvals.
    func updateBadge(_ count: Int) {
        NSApp?.dockTile.badgeLabel = count > 0 ? String(count) : nil
    }

    /// Removes delivered notifications for a session once the user looked at it.
    func clearDelivered(sessionId: String) {
        guard Self.isAvailable else { return }
        UNUserNotificationCenter.current().removeDeliveredNotifications(withIdentifiers: ["task-\(sessionId)", "approval-\(sessionId)", "question-\(sessionId)"])
    }

    // MARK: - Private

    private func post(title: String, body: String, category: String, projectId: String, sessionId: String, identifier: String) {
        guard Self.isAvailable else { return }
        let content = UNMutableNotificationContent()
        content.title = title
        if !body.isEmpty { content.body = String(body.prefix(200)) }
        content.sound = .default
        content.categoryIdentifier = category
        content.userInfo = [UserInfoKey.projectId: projectId, UserInfoKey.sessionId: sessionId]
        let request = UNNotificationRequest(identifier: identifier, content: content, trigger: nil)

        switch authorization {
        case .granted:
            deliver(request)
        case .denied:
            return
        case .requesting:
            queuedRequests.append(request)
        case .unknown:
            queuedRequests.append(request)
            requestAuthorization()
        }
    }

    private func requestAuthorization() {
        authorization = .requesting
        Task { [weak self] in
            let granted: Bool
            do {
                granted = try await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge])
            } catch {
                granted = false
            }
            await MainActor.run { self?.finishAuthorization(granted: granted) }
        }
    }

    private func finishAuthorization(granted: Bool) {
        authorization = granted ? .granted : .denied
        let queued = queuedRequests
        queuedRequests.removeAll()
        guard granted else { return }
        for request in queued { deliver(request) }
    }

    private func deliver(_ request: UNNotificationRequest) {
        UNUserNotificationCenter.current().add(request) { error in
            if let error { BackendLog.debug("notification not delivered: \(error.localizedDescription)") }
        }
    }

    private func open(projectId: String?, sessionId: String?) {
        guard let projectId, let sessionId else { return }
        onOpen?(projectId, sessionId)
    }
}

extension TaskNotifier: UNUserNotificationCenterDelegate {
    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse, withCompletionHandler completionHandler: @escaping () -> Void) {
        let info = response.notification.request.content.userInfo
        let projectId = info[UserInfoKey.projectId] as? String
        let sessionId = info[UserInfoKey.sessionId] as? String
        Task { @MainActor in
            self.open(projectId: projectId, sessionId: sessionId)
            completionHandler()
        }
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .sound])
    }
}
