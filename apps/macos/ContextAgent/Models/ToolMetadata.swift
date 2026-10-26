import SwiftUI
import Foundation

/// Chip label for a tool's category in the timeline.
enum ToolCategory: String {
    case file, search, shell, git, memory, validation, context, agents, browser, mcp
}

/// Presentation metadata for the backend's tools (docs/desktop-protocol.md
/// "Tool names"): icon, human label, category chip and the verbs used in
/// timeline rows ("Reading src/app.rs") and summaries ("Read 3 files").
struct ToolMetadata {
    let icon: String
    let label: String
    let category: ToolCategory
    /// Present participle: "Reading".
    let verb: String
    /// Past tense: "Read".
    let pastVerb: String

    /// The 15 built-in tools, the sub-agent tools under Ultra effort, and
    /// the desktop app's `browser`.
    static let knownToolNames: [String] = [
        "read_file", "search_code", "find_symbol", "find_references", "list_files",
        "apply_patch", "write_file", "execute_command", "run_validation",
        "git_status", "git_diff", "retrieve_memory", "inspect_context", "web_search", "web_fetch", "spawn_agents", "send_input", "wait_agents", "read_agent", "close_agent", "browser", "ask_user",
    ]

    static func info(for toolName: String) -> ToolMetadata {
        switch toolName {
        case "read_file":
            return ToolMetadata(icon: "doc.text", label: "Read file", category: .file, verb: "Reading", pastVerb: "Read")
        case "search_code":
            return ToolMetadata(icon: "magnifyingglass", label: "Search code", category: .search, verb: "Searching", pastVerb: "Searched")
        case "find_symbol":
            return ToolMetadata(icon: "function", label: "Find symbol", category: .search, verb: "Finding", pastVerb: "Found")
        case "find_references":
            return ToolMetadata(icon: "arrow.triangle.branch", label: "Find references", category: .search, verb: "Finding", pastVerb: "Found")
        case "list_files":
            return ToolMetadata(icon: "folder", label: "List files", category: .file, verb: "Listing", pastVerb: "Listed")
        case "apply_patch":
            return ToolMetadata(icon: "pencil.line", label: "Apply patch", category: .file, verb: "Editing", pastVerb: "Edited")
        case "write_file":
            return ToolMetadata(icon: "square.and.pencil", label: "Write file", category: .file, verb: "Writing", pastVerb: "Wrote")
        case "execute_command":
            return ToolMetadata(icon: "terminal", label: "Run command", category: .shell, verb: "Running", pastVerb: "Ran")
        case "run_validation":
            return ToolMetadata(icon: "checkmark.seal", label: "Run validation", category: .validation, verb: "Validating", pastVerb: "Validated")
        case "git_status":
            return ToolMetadata(icon: "arrow.triangle.pull", label: "Git status", category: .git, verb: "Inspecting", pastVerb: "Inspected")
        case "git_diff":
            return ToolMetadata(icon: "arrow.triangle.pull", label: "Git diff", category: .git, verb: "Inspecting", pastVerb: "Inspected")
        case "retrieve_memory":
            return ToolMetadata(icon: "brain.head.profile", label: "Retrieve memory", category: .memory, verb: "Recalling", pastVerb: "Recalled")
        case "inspect_context":
            return ToolMetadata(icon: "text.magnifyingglass", label: "Inspect context", category: .context, verb: "Inspecting", pastVerb: "Inspected")
        case "web_search":
            return ToolMetadata(icon: "globe", label: "Search the web", category: .search, verb: "Searching", pastVerb: "Searched")
        case "web_fetch":
            return ToolMetadata(icon: "safari", label: "Fetch page", category: .search, verb: "Fetching", pastVerb: "Fetched")
        case "spawn_agents":
            return ToolMetadata(icon: Icons.agents, label: "Deploy agents", category: .agents, verb: "Deploying", pastVerb: "Deployed")
        case "send_input":
            return ToolMetadata(icon: Icons.agents, label: "Message agent", category: .agents, verb: "Messaging", pastVerb: "Messaged")
        case "wait_agents":
            return ToolMetadata(icon: Icons.agents, label: "Wait for agents", category: .agents, verb: "Waiting for", pastVerb: "Waited for")
        case "read_agent":
            return ToolMetadata(icon: Icons.agents, label: "Check on agent", category: .agents, verb: "Checking on", pastVerb: "Checked on")
        case "close_agent":
            return ToolMetadata(icon: Icons.agents, label: "Stop agent", category: .agents, verb: "Stopping", pastVerb: "Stopped")
        case "browser":
            return ToolMetadata(icon: Icons.browser, label: "Browser", category: .browser, verb: "Browsing", pastVerb: "Browsed")
        case "ask_user":
            return ToolMetadata(icon: "questionmark.bubble", label: "Ask you", category: .context, verb: "Asking", pastVerb: "Asked")
        case ApprovalRequest.discountDropTool:
            return ToolMetadata(icon: Icons.spending, label: "Lower discount", category: .shell, verb: "Continuing", pastVerb: "Continued")
        default:
            if let (server, tool) = mcpParts(toolName) {
                return ToolMetadata(icon: Icons.mcp, label: "\(server) › \(tool)", category: .mcp, verb: "Calling", pastVerb: "Called")
            }
            return ToolMetadata(icon: "wrench", label: humanize(toolName), category: .shell, verb: "Calling", pastVerb: "Called")
        }
    }

    /// `mcp__<server>__<tool>` → the server and tool names (docs/desktop-protocol.md "Tool names").
    static func mcpParts(_ toolName: String) -> (server: String, tool: String)? {
        guard toolName.hasPrefix("mcp__") else { return nil }
        let rest = toolName.dropFirst(5)
        guard let range = rest.range(of: "__") else { return nil }
        let server = String(rest[..<range.lowerBound])
        let tool = String(rest[range.upperBound...])
        guard !server.isEmpty, !tool.isEmpty else { return nil }
        return (server, tool)
    }

    /// The call's path-like argument (`path`, then `directory`, then `cwd`),
    /// for context menus (Show in Changes, Reveal in Finder, …).
    static func pathArgument(for call: ToolCallItem) -> String? {
        for key in ["path", "directory", "cwd"] {
            if let value = string(call.arguments, key) { return value }
        }
        return nil
    }

    /// Timeline row label: verb + target. Commands are wrapped in backticks:
    /// "Reading src/app.rs", "Running `cargo test`", "Searching “needle”".
    static func rowLabel(for call: ToolCallItem) -> String {
        let meta = info(for: call.name)
        let args = call.arguments

        switch call.name {
        case "read_file", "write_file", "apply_patch":
            if let path = pathArgument(for: call) { return "\(meta.verb) \(path)" }
        case "list_files":
            let pattern = string(args, "pattern")
            let directory = string(args, "directory")
            switch (pattern, directory) {
            case (let pattern?, let directory?): return "Listing \(pattern) in \(directory)"
            case (let pattern?, nil): return "Listing \(pattern)"
            case (nil, let directory?): return "Listing \(directory)"
            case (nil, nil): return "Listing files"
            }
        case "search_code":
            if let query = string(args, "query") { return "Searching “\(abbreviate(query))”" }
        case "find_symbol":
            if let name = string(args, "name") { return "Finding \(abbreviate(name))" }
        case "find_references":
            if let name = string(args, "name") { return "Finding references to \(abbreviate(name))" }
        case "execute_command":
            if let command = string(args, "command") { return "Running `\(abbreviate(command))`" }
        case "run_validation":
            if let command = string(args, "command") { return "Validating `\(abbreviate(command))`" }
            if let kind = string(args, "kind"), kind != "all" { return "Validating \(kind)" }
            return "Validating"
        case "git_status":
            return "Inspecting git status"
        case "git_diff":
            if let path = pathArgument(for: call) { return "Inspecting diff of \(path)" }
            return "Inspecting git diff"
        case "retrieve_memory":
            if let query = string(args, "query") { return "Recalling “\(abbreviate(query))”" }
            if args["recent_tasks"]?.boolValue == true { return "Recalling recent tasks" }
            if let task = string(args, "task_id") { return "Recalling task \(abbreviate(task))" }
            if let category = string(args, "category") { return "Recalling \(category.replacingOccurrences(of: "_", with: " "))" }
            return "Recalling memory"
        case "inspect_context":
            if let what = string(args, "what") { return "Inspecting context \(what)" }
            return "Inspecting context"
        case "web_search":
            if let query = string(args, "query") { return "Searching the web for “\(abbreviate(query))”" }
            return "Searching the web"
        case "web_fetch":
            if let url = string(args, "url") { return "Fetching \(abbreviate(url))" }
            return "Fetching a page"
        case "browser":
            return browserRowLabel(args)
        case "ask_user":
            let questions = (args["questions"]?.arrayValue ?? []).compactMap { $0["question"]?.stringValue }
            if questions.count == 1 { return "Asking “\(abbreviate(questions[0]))”" }
            return questions.isEmpty ? "Asking you" : "Asking you \(questions.count) questions"
        case "spawn_agents":
            let agents = args["agents"]?.arrayValue ?? []
            let names = agents.compactMap { $0["name"]?.stringValue }
            if agents.isEmpty { return "Deploying agents" }
            return "Deploying \(agents.count) agent\(agents.count == 1 ? "" : "s")" + (names.isEmpty ? "" : ": " + abbreviate(names.joined(separator: ", ")))
        case "send_input":
            if let id = string(args, "id") { return "Messaging agent \(id)" }
            return "Messaging an agent"
        case "wait_agents":
            let ids = (args["ids"]?.arrayValue ?? []).compactMap { $0.stringValue }
            return ids.isEmpty ? "Waiting for agents" : "Waiting for " + abbreviate(ids.joined(separator: ", "))
        case "read_agent":
            if let id = string(args, "id") { return "Checking on agent \(id)" }
            return "Listing agents"
        case "close_agent":
            if let id = string(args, "id") { return "Stopping agent \(id)" }
            return "Stopping an agent"
        default:
            break
        }

        if let (server, tool) = mcpParts(call.name) { return "Calling \(tool) on \(server)" }
        if let target = call.target { return "\(meta.verb) \(abbreviate(target))" }
        return knownToolNames.contains(call.name) ? meta.label : "\(meta.verb) \(meta.label.lowercased())"
    }

    /// Lines added / removed by a `write_file` or `apply_patch` call, from its
    /// arguments, for the "+12 −3" at the right of its row. `apply_patch`
    /// edits drop the lines `old_text` and `new_text` share at either end, so
    /// the context the model quoted is not counted; `write_file` counts every
    /// line as added (an overwrite's old content is not in the call).
    static func lineChanges(for call: ToolCallItem) -> (added: Int, removed: Int)? {
        switch call.name {
        case "write_file":
            guard let content = call.arguments["content"]?.stringValue else { return nil }
            return (lines(of: content).count, 0)
        case "apply_patch":
            guard let edits = call.arguments["edits"]?.arrayValue, !edits.isEmpty else { return nil }
            var added = 0
            var removed = 0
            for edit in edits {
                var old = lines(of: edit["old_text"]?.stringValue ?? edit["old"]?.stringValue ?? "")[...]
                var new = lines(of: edit["new_text"]?.stringValue ?? edit["new"]?.stringValue ?? "")[...]
                while let a = old.first, let b = new.first, a == b { old.removeFirst(); new.removeFirst() }
                while let a = old.last, let b = new.last, a == b { old.removeLast(); new.removeLast() }
                added += new.count
                removed += old.count
            }
            return (added, removed)
        default:
            return nil
        }
    }

    /// "Opening example.com", "Clicking [3]", "Typing into [2]" …
    private static func browserRowLabel(_ args: JSONValue) -> String {
        let ref = args["ref"]?.intValue.map { "[\($0)]" } ?? "an element"
        switch string(args, "action") {
        case "navigate":
            if let url = string(args, "url") {
                let shown = url.replacingOccurrences(of: #"^https?://"#, with: "", options: .regularExpression)
                return "Opening \(abbreviate(shown, limit: 60))"
            }
            return "Opening a page"
        case "back": return "Going back"
        case "forward": return "Going forward"
        case "reload": return "Reloading the page"
        case "read": return "Reading the page"
        case "snapshot":
            if let query = string(args, "query") { return "Finding “\(abbreviate(query))” on the page" }
            return "Listing the page's elements"
        case "click": return "Clicking \(ref)"
        case "type":
            if let text = string(args, "text") { return "Typing “\(abbreviate(text, limit: 40))” into \(ref)" }
            return "Typing into \(ref)"
        case "select": return "Choosing an option in \(ref)"
        case "scroll":
            if args["ref"]?.intValue != nil { return "Scrolling to \(ref)" }
            return "Scrolling \(string(args, "direction") ?? "down")"
        case "evaluate": return "Running script in the page"
        case "screenshot": return "Taking a screenshot"
        default: return "Browsing"
        }
    }

    // MARK: - Helpers

    /// "a\nb\n" → ["a", "b"]; "" → [].
    private static func lines(of text: String) -> [String] {
        if text.isEmpty { return [] }
        var parts = text.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        if parts.count > 1, parts.last?.isEmpty == true { parts.removeLast() }
        return parts
    }

    private static func string(_ arguments: JSONValue, _ key: String) -> String? {
        guard let raw = arguments[key]?.stringValue else { return nil }
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    /// One line, whitespace collapsed, capped at `limit` characters with an ellipsis.
    static func abbreviate(_ text: String, limit: Int = 80) -> String {
        let collapsed = text
            .split(whereSeparator: { $0.isWhitespace || $0.isNewline })
            .joined(separator: " ")
        if collapsed.count <= limit { return collapsed }
        return String(collapsed.prefix(limit - 1)).trimmingCharacters(in: .whitespaces) + "…"
    }

    /// "my_custom_tool" → "My custom tool".
    private static func humanize(_ name: String) -> String {
        let words = name.split(whereSeparator: { $0 == "_" || $0 == "-" || $0 == "." }).map(String.init)
        guard let first = words.first else { return name }
        let rest = words.dropFirst().map { $0.lowercased() }
        return ([first.prefix(1).uppercased() + first.dropFirst().lowercased()] + rest).joined(separator: " ")
    }
}
