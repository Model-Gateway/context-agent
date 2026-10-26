import AppKit
import WebKit
import Foundation

/// The Inspector Browser tab's page for one project: a long-lived `WKWebView`
/// the user browses in and the agent drives through `browser/request`
/// notifications (`docs/desktop-protocol.md`). The view outlives tab
/// switches, loads and runs script while the tab is not shown, and the
/// agent's actions never change which Inspector tab is selected.
@MainActor
@Observable
final class BrowserWorkspace: NSObject {
    let projectId: String

    private(set) var url: URL?
    private(set) var title: String = ""
    private(set) var isLoading = false
    private(set) var progress: Double = 0
    private(set) var canGoBack = false
    private(set) var canGoForward = false
    /// The last navigation failure, shown under the toolbar until the next load.
    private(set) var loadError: String?
    /// What the agent is doing right now ("Opening example.com…"); nil when idle.
    private(set) var agentActivity: String?
    /// The agent's last finished action, kept for a few seconds.
    private(set) var recentAgentAction: String?
    /// The address field's text; follows the page, edited by the user.
    var addressText: String = ""

    @ObservationIgnored let webView: WKWebView
    @ObservationIgnored private var observers: [NSKeyValueObservation] = []
    /// Serializes the agent's actions (two sessions may drive one browser).
    @ObservationIgnored private var tail: Task<Void, Never>?
    @ObservationIgnored private var recentActionReset: Task<Void, Never>?
    /// JavaScript dialogs the page raised during the current action, auto-dismissed.
    @ObservationIgnored private var dialogs: [String] = []

    /// Size the detached view renders at (snapshots, layout-dependent script)
    /// before it is first shown; the host resizes it afterwards.
    static let detachedSize = NSSize(width: 1024, height: 768)
    static let startPage = "about:blank"
    private static let maxTextChars = 15_000
    private static let navigateTextChars = 6_000
    private static let maxElements = 150
    private static let loadTimeout: TimeInterval = 30

    var isAgentDriving: Bool { agentActivity != nil }
    var hasPage: Bool { url != nil && url?.absoluteString != Self.startPage }

    init(projectId: String) {
        self.projectId = projectId
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .default()
        configuration.preferences.isFraudulentWebsiteWarningEnabled = true
        configuration.defaultWebpagePreferences.allowsContentJavaScript = true
        webView = WKWebView(frame: NSRect(origin: .zero, size: Self.detachedSize), configuration: configuration)
        webView.allowsBackForwardNavigationGestures = true
        webView.allowsMagnification = true
        super.init()
        webView.navigationDelegate = self
        webView.uiDelegate = self
        observers = [
            webView.observe(\.url, options: [.new]) { [weak self] view, _ in
                Task { @MainActor in self?.pageChanged(url: view.url) }
            },
            webView.observe(\.title, options: [.new]) { [weak self] view, _ in
                Task { @MainActor in self?.title = view.title ?? "" }
            },
            webView.observe(\.isLoading, options: [.new]) { [weak self] view, _ in
                Task { @MainActor in self?.isLoading = view.isLoading }
            },
            webView.observe(\.estimatedProgress, options: [.new]) { [weak self] view, _ in
                Task { @MainActor in self?.progress = view.estimatedProgress }
            },
            webView.observe(\.canGoBack, options: [.new]) { [weak self] view, _ in
                Task { @MainActor in self?.canGoBack = view.canGoBack }
            },
            webView.observe(\.canGoForward, options: [.new]) { [weak self] view, _ in
                Task { @MainActor in self?.canGoForward = view.canGoForward }
            },
        ]
    }

    private func pageChanged(url: URL?) {
        self.url = url
        if let url, url.absoluteString != Self.startPage {
            addressText = url.absoluteString
        }
    }

    // MARK: - User navigation

    /// Loads what the user typed: a URL (scheme added when missing), else a
    /// web search for the words.
    func open(_ text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        if let url = Self.url(from: trimmed) {
            load(url)
        } else if var components = URLComponents(string: "https://duckduckgo.com/") {
            components.queryItems = [URLQueryItem(name: "q", value: trimmed)]
            if let url = components.url { load(url) }
        }
    }

    func load(_ url: URL) {
        loadError = nil
        webView.load(URLRequest(url: url))
    }

    func goBack() { webView.goBack() }
    func goForward() { webView.goForward() }
    func reload() {
        loadError = nil
        if webView.url == nil { return }
        webView.reload()
    }
    func stop() { webView.stopLoading() }

    /// Lets the page go when its project closes.
    func tearDown() {
        tail?.cancel()
        recentActionReset?.cancel()
        webView.stopLoading()
        webView.navigationDelegate = nil
        webView.uiDelegate = nil
        observers.removeAll()
    }

    /// `example.com/docs` → https://example.com/docs; words with spaces or
    /// without a dot are not URLs.
    static func url(from text: String) -> URL? {
        if text.contains("://") {
            guard let url = URL(string: text), let scheme = url.scheme?.lowercased(), ["http", "https"].contains(scheme), url.host != nil else { return nil }
            return url
        }
        guard !text.contains(" ") else { return nil }
        let host = text.split(separator: "/", maxSplits: 1).first.map(String.init) ?? text
        let bare = host.split(separator: ":").first.map(String.init) ?? host
        guard bare.contains(".") || bare == "localhost" || bare.hasSuffix(".localhost") else { return nil }
        return URL(string: "https://" + text)
    }

    // MARK: - Agent actions

    /// Performs one `browser/request` action and returns the reply for
    /// `browser/respond`. Actions run one after another.
    func perform(_ action: JSONValue) async -> BrowserReply {
        let previous = tail
        let task = Task<BrowserReply, Never> { [weak self] in
            await previous?.value
            guard let self else { return BrowserReply(ok: false, error: "the browser is gone") }
            return await self.run(action)
        }
        tail = Task { _ = await task.value }
        return await task.value
    }

    private func run(_ action: JSONValue) async -> BrowserReply {
        let kind = action["action"]?.stringValue ?? ""
        dialogs = []
        agentActivity = Self.activityLabel(for: action)
        defer {
            recentAgentAction = agentActivity
            agentActivity = nil
            recentActionReset?.cancel()
            recentActionReset = Task { [weak self] in
                try? await Task.sleep(nanoseconds: 4_000_000_000)
                guard !Task.isCancelled else { return }
                self?.recentAgentAction = nil
            }
        }
        var reply: BrowserReply
        do {
            switch kind {
            case "navigate":
                guard let text = action["url"]?.stringValue, let url = URL(string: text) else { return failure("navigate needs a valid `url`") }
                load(url)
                await awaitLoad()
                if let loadError { return failure("could not load \(text): \(loadError)") }
                reply = try await pageSummary(textLimit: Self.navigateTextChars, includeElements: true, query: nil)
            case "back", "forward", "reload":
                switch kind {
                case "back":
                    guard canGoBack else { return failure("there is no page to go back to") }
                    goBack()
                case "forward":
                    guard canGoForward else { return failure("there is no page to go forward to") }
                    goForward()
                default:
                    guard hasPage else { return failure("no page is open; use navigate first") }
                    reload()
                }
                await awaitLoad()
                if let loadError { return failure(loadError) }
                reply = try await pageSummary(textLimit: Self.navigateTextChars, includeElements: true, query: nil)
            case "read":
                guard hasPage else { return failure("no page is open; use navigate first") }
                reply = try await pageSummary(textLimit: Self.maxTextChars, includeElements: false, query: nil)
            case "snapshot":
                guard hasPage else { return failure("no page is open; use navigate first") }
                reply = try await pageSummary(textLimit: 0, includeElements: true, query: action["query"]?.stringValue)
            case "click":
                guard let ref = action["ref"]?.intValue else { return failure("click needs `ref`") }
                let result = try await script(Self.clickScript, ["ref": ref])
                if let error = result["error"] as? String { return failure(error) }
                await awaitLoad(graceMs: 600)
                reply = try await pageSummary(textLimit: Self.navigateTextChars, includeElements: true, query: nil)
            case "type":
                guard let ref = action["ref"]?.intValue, let text = action["text"]?.stringValue else { return failure("type needs `ref` and `text`") }
                let submit = action["submit"]?.boolValue ?? false
                let result = try await script(Self.typeScript, ["ref": ref, "text": text, "submit": submit])
                if let error = result["error"] as? String { return failure(error) }
                if submit {
                    await awaitLoad(graceMs: 600)
                    reply = try await pageSummary(textLimit: Self.navigateTextChars, includeElements: true, query: nil)
                } else {
                    reply = BrowserReply(ok: true, content: "Typed \(text.count) character\(text.count == 1 ? "" : "s") into [\(ref)].", url: url?.absoluteString, title: title)
                }
            case "select":
                guard let ref = action["ref"]?.intValue, let value = action["value"]?.stringValue else { return failure("select needs `ref` and `value`") }
                let result = try await script(Self.selectScript, ["ref": ref, "value": value])
                if let error = result["error"] as? String { return failure(error) }
                reply = BrowserReply(ok: true, content: "Selected \"\(result["label"] as? String ?? value)\" in [\(ref)].", url: url?.absoluteString, title: title)
            case "scroll":
                guard hasPage else { return failure("no page is open; use navigate first") }
                let direction = action["direction"]?.stringValue ?? "down"
                let amount = action["amount"]?.intValue ?? 0
                let result = try await script(Self.scrollScript, ["ref": action["ref"]?.intValue ?? -1, "direction": direction, "amount": amount])
                if let error = result["error"] as? String { return failure(error) }
                try? await Task.sleep(nanoseconds: 150_000_000)
                reply = BrowserReply(ok: true, content: "Scrolled to y=\(result["y"] as? Int ?? 0) of \(result["height"] as? Int ?? 0).", url: url?.absoluteString, title: title)
            case "evaluate":
                guard hasPage else { return failure("no page is open; use navigate first") }
                guard let source = action["script"]?.stringValue else { return failure("evaluate needs `script`") }
                let value = try await webView.callAsyncJavaScript(source, arguments: [:], in: nil, contentWorld: .page)
                reply = BrowserReply(ok: true, content: Self.render(value), url: url?.absoluteString, title: title)
            case "screenshot":
                guard hasPage else { return failure("no page is open; use navigate first") }
                let path = try await screenshot()
                reply = BrowserReply(ok: true, content: "Saved a screenshot of the page to \(path) (\(Int(webView.bounds.width))×\(Int(webView.bounds.height)) pt).", url: url?.absoluteString, title: title)
            default:
                return failure("unknown action `\(kind)`")
            }
        } catch {
            return failure(Self.describe(error))
        }
        if !dialogs.isEmpty {
            reply.content += "\n\nPage dialogs (dismissed automatically): " + dialogs.joined(separator: " · ")
        }
        return reply
    }

    private func failure(_ message: String) -> BrowserReply {
        BrowserReply(ok: false, url: url?.absoluteString, title: title, error: message)
    }

    /// Waits for a navigation to finish: one that is already under way, or
    /// one that starts within `graceMs` (a click that navigates).
    private func awaitLoad(graceMs: Int = 300) async {
        let started = Date()
        while !webView.isLoading, Date().timeIntervalSince(started) < Double(graceMs) / 1000 {
            try? await Task.sleep(nanoseconds: 40_000_000)
        }
        while webView.isLoading, Date().timeIntervalSince(started) < Self.loadTimeout {
            try? await Task.sleep(nanoseconds: 60_000_000)
        }
        // Let scripts that run on load settle before reading the DOM.
        try? await Task.sleep(nanoseconds: 200_000_000)
    }

    private func pageSummary(textLimit: Int, includeElements: Bool, query: String?) async throws -> BrowserReply {
        let result = try await script(Self.snapshotScript, ["query": query ?? "", "maxElements": Self.maxElements, "maxText": textLimit])
        let pageURL = result["url"] as? String ?? url?.absoluteString
        let pageTitle = result["title"] as? String ?? title
        var parts: [String] = []
        if textLimit > 0 {
            let text = (result["text"] as? String ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            let total = result["textLength"] as? Int ?? text.count
            if text.isEmpty {
                parts.append("(the page has no visible text)")
            } else {
                parts.append(text)
                if total > text.count { parts.append("… [\(total - text.count) more characters; use read for the full text or scroll]") }
            }
        }
        if includeElements {
            let elements = result["elements"] as? [String] ?? []
            let total = result["total"] as? Int ?? elements.count
            if elements.isEmpty {
                parts.append(query == nil ? "Interactive elements: none found." : "Interactive elements matching \"\(query!)\": none.")
            } else {
                var header = "Interactive elements (\(elements.count)\(total > elements.count ? " of \(total)" : "")): click, type and select take these numbers."
                if let query { header = "Interactive elements matching \"\(query)\" (\(elements.count) of \(total)):" }
                parts.append(header + "\n" + elements.joined(separator: "\n"))
            }
        }
        return BrowserReply(ok: true, content: parts.joined(separator: "\n\n"), url: pageURL, title: pageTitle)
    }

    private func script(_ body: String, _ arguments: [String: Any]) async throws -> [String: Any] {
        let value = try await webView.callAsyncJavaScript(body, arguments: arguments, in: nil, contentWorld: .page)
        return value as? [String: Any] ?? [:]
    }

    private func screenshot() async throws -> String {
        let configuration = WKSnapshotConfiguration()
        configuration.afterScreenUpdates = true
        let image = try await webView.takeSnapshot(configuration: configuration)
        guard let tiff = image.tiffRepresentation, let bitmap = NSBitmapImageRep(data: tiff), let png = bitmap.representation(using: .png, properties: [:]) else {
            throw BrowserActionError.message("could not encode the screenshot")
        }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("ContextBrowser", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let stamp = ISO8601DateFormatter().string(from: Date()).replacingOccurrences(of: ":", with: "-")
        let file = directory.appendingPathComponent("page-\(stamp).png")
        try png.write(to: file)
        return file.path
    }

    private static func render(_ value: Any?) -> String {
        guard let value else { return "undefined" }
        if let string = value as? String { return string }
        if JSONSerialization.isValidJSONObject(value), let data = try? JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys]), let text = String(data: data, encoding: .utf8) {
            return text
        }
        if let number = value as? NSNumber { return number.stringValue }
        return String(describing: value)
    }

    private static func describe(_ error: Error) -> String {
        let nsError = error as NSError
        if let message = nsError.userInfo["WKJavaScriptExceptionMessage"] as? String {
            return "script error: \(message)"
        }
        if case BrowserActionError.message(let message) = error { return message }
        return nsError.localizedDescription
    }

    /// "Opening example.com…", "Clicking [3]…".
    static func activityLabel(for action: JSONValue) -> String {
        let kind = action["action"]?.stringValue ?? "browser"
        let ref = action["ref"]?.intValue.map { "[\($0)]" } ?? "an element"
        switch kind {
        case "navigate":
            let host = action["url"]?.stringValue.flatMap { URL(string: $0)?.host } ?? action["url"]?.stringValue ?? "a page"
            return "Opening \(host)…"
        case "back": return "Going back…"
        case "forward": return "Going forward…"
        case "reload": return "Reloading…"
        case "read": return "Reading the page…"
        case "snapshot": return "Listing the page's elements…"
        case "click": return "Clicking \(ref)…"
        case "type": return "Typing into \(ref)…"
        case "select": return "Choosing an option in \(ref)…"
        case "scroll": return "Scrolling…"
        case "evaluate": return "Running script in the page…"
        case "screenshot": return "Taking a screenshot…"
        default: return "Driving the browser…"
        }
    }

    // MARK: - Scripts (function bodies for callAsyncJavaScript)

    /// Numbers the visible interactive elements with `data-ctx-ref` and
    /// returns the page text and one line per element.
    private static let snapshotScript = #"""
    const selector = 'a[href], button, input, select, textarea, summary, [role="button"], [role="link"], [role="tab"], [role="menuitem"], [role="menuitemcheckbox"], [role="menuitemradio"], [role="checkbox"], [role="radio"], [role="switch"], [role="textbox"], [role="searchbox"], [role="combobox"], [role="option"], [role="slider"], [contenteditable=""], [contenteditable="true"], [onclick], [tabindex]:not([tabindex="-1"])';
    const vh = window.innerHeight;
    function visible(el) {
        const r = el.getBoundingClientRect();
        if (r.width === 0 && r.height === 0) return false;
        const cs = getComputedStyle(el);
        return cs.visibility !== 'hidden' && cs.display !== 'none' && cs.opacity !== '0';
    }
    function role(el) {
        const r = el.getAttribute('role');
        if (r) return r;
        const tag = el.tagName.toLowerCase();
        if (tag === 'a') return 'link';
        if (tag === 'input') {
            const t = (el.type || 'text').toLowerCase();
            if (t === 'submit' || t === 'button' || t === 'reset' || t === 'image') return 'button';
            if (t === 'checkbox' || t === 'radio' || t === 'range' || t === 'file' || t === 'color') return t;
            return 'textbox';
        }
        if (tag === 'textarea') return 'textbox';
        if (el.isContentEditable) return 'textbox';
        return tag;
    }
    function name(el) {
        const aria = el.getAttribute('aria-label');
        if (aria) return aria;
        const labelled = el.getAttribute('aria-labelledby');
        if (labelled) {
            const text = labelled.split(/\s+/).map(id => { const n = document.getElementById(id); return n ? n.innerText : ''; }).join(' ').trim();
            if (text) return text;
        }
        if (el.labels && el.labels.length && el.labels[0].innerText.trim()) return el.labels[0].innerText.trim();
        const own = (el.innerText || '').trim();
        if (own) return own;
        return (el.getAttribute('placeholder') || el.getAttribute('title') || el.getAttribute('alt') || el.getAttribute('name') || el.value || '').toString().trim();
    }
    document.querySelectorAll('[data-ctx-ref]').forEach(e => e.removeAttribute('data-ctx-ref'));
    const q = (query || '').toLowerCase();
    const lines = [];
    let n = 0;
    for (const el of document.querySelectorAll(selector)) {
        if (!visible(el)) continue;
        const ref = n++;
        el.setAttribute('data-ctx-ref', String(ref));
        if (lines.length >= maxElements) continue;
        const tag = el.tagName.toLowerCase();
        const r = el.getBoundingClientRect();
        let extra = '';
        if (tag === 'a' && el.href) extra = ' → ' + String(el.href).slice(0, 120);
        if ((tag === 'input' || tag === 'textarea') && el.value && el.type !== 'password') extra = ' value="' + String(el.value).slice(0, 60) + '"';
        if (tag === 'input' && el.type === 'password') extra = ' (password field)';
        if (tag === 'input' && (el.type === 'checkbox' || el.type === 'radio')) extra = el.checked ? ' (checked)' : ' (unchecked)';
        if (tag === 'select') extra = ' options: ' + Array.from(el.options).map(o => o.text.trim()).slice(0, 12).join(' | ') + (el.options.length > 12 ? ' | …' : '');
        if (el.disabled) extra += ' (disabled)';
        const line = role(el) + ' "' + name(el).replace(/\s+/g, ' ').slice(0, 80) + '"' + extra;
        if (q && !line.toLowerCase().includes(q)) continue;
        lines.push('[' + ref + '] ' + line + (r.bottom < 0 || r.top > vh ? ' (offscreen)' : ''));
    }
    const text = (document.body ? document.body.innerText : '').replace(/[ \t]+\n/g, '\n').replace(/\n{3,}/g, '\n\n').trim();
    return { url: location.href, title: document.title, text: maxText > 0 ? text.slice(0, maxText) : '', textLength: text.length, elements: lines, total: n };
    """#

    private static let clickScript = #"""
    const el = document.querySelector('[data-ctx-ref="' + ref + '"]');
    if (!el) return { error: 'no element [' + ref + '] on this page; take a new snapshot' };
    if (el.disabled) return { error: 'element [' + ref + '] is disabled' };
    el.scrollIntoView({ block: 'center', inline: 'nearest' });
    const r = el.getBoundingClientRect();
    const init = { bubbles: true, cancelable: true, view: window, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 };
    el.dispatchEvent(new MouseEvent('mousedown', init));
    if (typeof el.focus === 'function') el.focus();
    el.dispatchEvent(new MouseEvent('mouseup', init));
    el.click();
    return { ok: true };
    """#

    private static let typeScript = #"""
    const el = document.querySelector('[data-ctx-ref="' + ref + '"]');
    if (!el) return { error: 'no element [' + ref + '] on this page; take a new snapshot' };
    if (el.disabled || el.readOnly) return { error: 'element [' + ref + '] cannot be edited' };
    el.scrollIntoView({ block: 'center', inline: 'nearest' });
    if (typeof el.focus === 'function') el.focus();
    const tag = el.tagName.toLowerCase();
    if (el.isContentEditable) {
        el.textContent = text;
        el.dispatchEvent(new InputEvent('input', { bubbles: true, data: text, inputType: 'insertText' }));
    } else if (tag === 'input' || tag === 'textarea') {
        const proto = tag === 'textarea' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
        const setter = Object.getOwnPropertyDescriptor(proto, 'value');
        if (setter && setter.set) setter.set.call(el, text); else el.value = text;
        el.dispatchEvent(new Event('input', { bubbles: true }));
        el.dispatchEvent(new Event('change', { bubbles: true }));
    } else {
        return { error: 'element [' + ref + '] is not a text field' };
    }
    if (submit) {
        const key = { key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true, cancelable: true };
        const proceed = el.dispatchEvent(new KeyboardEvent('keydown', key));
        el.dispatchEvent(new KeyboardEvent('keypress', key));
        el.dispatchEvent(new KeyboardEvent('keyup', key));
        if (proceed && el.form) {
            if (typeof el.form.requestSubmit === 'function') el.form.requestSubmit(); else el.form.submit();
        }
    }
    return { ok: true };
    """#

    private static let selectScript = #"""
    const el = document.querySelector('[data-ctx-ref="' + ref + '"]');
    if (!el) return { error: 'no element [' + ref + '] on this page; take a new snapshot' };
    if (el.tagName.toLowerCase() !== 'select') return { error: 'element [' + ref + '] is not a select' };
    const wanted = String(value).trim().toLowerCase();
    const option = Array.from(el.options).find(o => o.value.toLowerCase() === wanted) || Array.from(el.options).find(o => o.text.trim().toLowerCase() === wanted);
    if (!option) return { error: 'no option "' + value + '" in [' + ref + ']; options: ' + Array.from(el.options).map(o => o.text.trim()).join(' | ') };
    el.value = option.value;
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return { ok: true, label: option.text.trim() };
    """#

    private static let scrollScript = #"""
    if (ref >= 0) {
        const el = document.querySelector('[data-ctx-ref="' + ref + '"]');
        if (!el) return { error: 'no element [' + ref + '] on this page; take a new snapshot' };
        el.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'instant' });
    } else {
        const step = amount > 0 ? amount : Math.max(200, window.innerHeight - 80);
        if (direction === 'top') window.scrollTo(0, 0);
        else if (direction === 'bottom') window.scrollTo(0, document.documentElement.scrollHeight);
        else window.scrollBy(0, direction === 'up' ? -step : step);
    }
    return { ok: true, y: Math.round(window.scrollY), height: Math.round(document.documentElement.scrollHeight) };
    """#
}

enum BrowserActionError: Error {
    case message(String)
}

// MARK: - WebKit delegates

extension BrowserWorkspace: WKNavigationDelegate, WKUIDelegate {
    nonisolated func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
        Task { @MainActor in self.loadError = nil }
    }

    nonisolated func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        let message = Self.navigationError(error)
        Task { @MainActor in if let message { self.loadError = message } }
    }

    nonisolated func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        let message = Self.navigationError(error)
        Task { @MainActor in if let message { self.loadError = message } }
    }

    /// A cancelled load (the user typed a new address) is not an error.
    nonisolated private static func navigationError(_ error: Error) -> String? {
        let nsError = error as NSError
        if nsError.domain == NSURLErrorDomain && nsError.code == NSURLErrorCancelled { return nil }
        if nsError.domain == "WebKitErrorDomain" && nsError.code == 102 { return nil } // frame load interrupted (downloads, redirects)
        return nsError.localizedDescription
    }

    /// Pop-ups (`target="_blank"`, `window.open`) open in the same view.
    nonisolated func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration, for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
        let request = navigationAction.request
        Task { @MainActor in
            if request.url != nil { self.webView.load(request) }
        }
        return nil
    }

    /// JavaScript dialogs are dismissed on their own (nobody is at the
    /// keyboard while the agent drives) and reported in the reply.
    nonisolated func webView(_ webView: WKWebView, runJavaScriptAlertPanelWithMessage message: String, initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping () -> Void) {
        Task { @MainActor in self.dialogs.append("alert: \(message)") }
        completionHandler()
    }

    nonisolated func webView(_ webView: WKWebView, runJavaScriptConfirmPanelWithMessage message: String, initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping (Bool) -> Void) {
        Task { @MainActor in self.dialogs.append("confirm (answered Cancel): \(message)") }
        completionHandler(false)
    }

    nonisolated func webView(_ webView: WKWebView, runJavaScriptTextInputPanelWithPrompt prompt: String, defaultText: String?, initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping (String?) -> Void) {
        Task { @MainActor in self.dialogs.append("prompt (dismissed): \(prompt)") }
        completionHandler(nil)
    }
}
