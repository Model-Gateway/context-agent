import Foundation
import AppKit

/// Chat responsiveness benchmark, run by the self-test when `CTX_PERFBENCH=1`
/// is set next to `--autotest <dir>` (ARCHITECTURE.md "Self-test mode").
///
/// Grows the open chat in stages by replaying synthetic runs through
/// `ChatState.apply` (the reducer real events go through), and at each size
/// measures a streaming run with four sub-agents and a burst of composer
/// typing. A probe on a background queue posts a block to the main queue
/// every 8 ms and records how long it waited: that wait is what a keystroke
/// or a scroll tick waits too. Results go to `perf.json`.
@MainActor
enum PerfBench {
    struct Phase: Encodable {
        var name: String
        var items: Int
        var seconds: Double
        var samples: Int
        var p50Ms: Double
        var p95Ms: Double
        var p99Ms: Double
        var maxMs: Double
        /// Total main-thread wait above one 60 Hz frame (16.7 ms).
        var stalledMs: Double
        /// CPU time of the app process (all threads) during the phase.
        var cpuMs: Double
    }

    /// Transcript sizes, in synthetic runs, the stages grow the chat to
    /// (`CTX_PERFBENCH_STAGES=0,30` overrides).
    static let stages: [Int] = {
        let custom = ProcessInfo.processInfo.environment["CTX_PERFBENCH_STAGES"]?.split(separator: ",").compactMap { Int($0) } ?? []
        return custom.isEmpty ? [0, 10, 30, 60] : custom
    }()

    static func run(appState: AppState, chat: ChatState, log: (String) -> Void) async -> [Phase] {
        var phases: [Phase] = []
        var seeded = 0
        for target in stages {
            while seeded < target {
                for event in seededRun(seeded, agents: seeded % 2 == 0) { chat.apply(event) }
                seeded += 1
                if seeded % 5 == 0 { await Task.yield() }
            }
            try? await Task.sleep(for: .seconds(1.5))
            let items = chat.transcript.count
            log("perf: \(items) items (\(seeded) runs) — measuring")
            phases.append(await measure("stream-agents@\(seeded)", items: items) { await streamRun(chat, index: 10_000 + seeded) })
            await scrollUpWhileStreaming(chat, index: 15_000 + seeded, log: log)
            try? await Task.sleep(for: .seconds(0.5))
            phases.append(await measure("typing@\(seeded)", items: chat.transcript.count) { await typing(chat) })
            chat.draft = ""
            try? await Task.sleep(for: .seconds(0.5))
            var corrections = ScrollCorrections()
            phases.append(await measure("scroll@\(seeded)", items: chat.transcript.count) { corrections = await scrollThrough() })
            // Back at the bottom after the drag: the window settles.
            try? await Task.sleep(for: .seconds(0.5))
            if let window = NSApp.windows.first(where: { $0.isVisible && $0.contentView != nil }), let scrollView = transcriptScrollView(in: window.contentView!) {
                log("perf:   transcript at rest: content height changed \(await idleHeightChanges(scrollView, seconds: 2)) times in 2 s")
            }
            phases.append(await measure("stream-after-scroll@\(seeded)", items: chat.transcript.count) { await streamRun(chat, index: 20_000 + seeded) })
            try? await Task.sleep(for: .seconds(0.5))
            phases.append(await measure("tab-switch@\(seeded)", items: chat.transcript.count) { await switchTabs(appState) })
            for phase in phases.suffix(5) {
                log("perf:   \(phase.name): p95 \(fmt(phase.p95Ms)) ms max \(fmt(phase.maxMs)) ms stalled \(fmt(phase.stalledMs)) ms cpu \(fmt(phase.cpuMs)) ms")
            }
            log("perf:   scroll: \(corrections.steps) steps, \(corrections.count) offset corrections (largest \(fmt(corrections.largest)) pt), reached top \(corrections.reachedTop), content height changed after \(corrections.heightChanges) steps (largest \(fmt(corrections.largestHeightChange)) pt)\(corrections.visualChecked > 0 ? ", \(corrections.visualJumps) visible jumps in \(corrections.visualChecked) checked steps (largest \(corrections.visualLargest) pt)" : "")")
        }
        if let phase = await agentPane(appState: appState, chat: chat, log: log) { phases.append(phase) }
        return phases
    }

    // MARK: Scrolling up during a run

    /// Starts a live run, scrolls the transcript up 6000 pt with a trackpad
    /// gesture (began/changed/ended events handed to the scroll view, which
    /// starts and ends a live scroll as for a real one) and logs the
    /// distance from the bottom while the run streams on — the view must
    /// stay where the user left it — then scrolls back past the bottom,
    /// where following must resume.
    private static func scrollUpWhileStreaming(_ chat: ChatState, index: Int, log: (String) -> Void) async {
        guard let window = NSApp.windows.first(where: { $0.isVisible && $0.contentView != nil }),
              let scrollView = transcriptScrollView(in: window.contentView!) else { return }
        let run = Task { @MainActor in await streamRun(chat, index: index) }
        try? await Task.sleep(for: .seconds(1.5))
        let clip = scrollView.contentView
        func distance() -> Int { Int((scrollView.documentView?.frame.height ?? 0) - clip.bounds.maxY) }
        func gesture(_ delta: Int32, steps: Int) async -> [Int] {
            var trace: [Int] = []
            for (phase, delta) in [(Int64(1), Int32(0))] + Array(repeating: (Int64(2), delta), count: steps) + [(Int64(4), Int32(0))] {
                guard let cg = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: delta, wheel2: 0, wheel3: 0) else { break }
                cg.setIntegerValueField(.scrollWheelEventIsContinuous, value: 1)
                cg.setIntegerValueField(.scrollWheelEventScrollPhase, value: phase)
                cg.setIntegerValueField(.scrollWheelEventPointDeltaAxis1, value: Int64(delta))
                guard let event = NSEvent(cgEvent: cg) else { break }
                scrollView.scrollWheel(with: event)
                try? await Task.sleep(for: .milliseconds(16))
                trace.append(distance())
            }
            return trace
        }
        func sample(_ count: Int) async -> [Int] {
            var samples: [Int] = []
            for _ in 0..<count {
                try? await Task.sleep(for: .milliseconds(100))
                samples.append(distance())
            }
            return samples
        }
        func every5(_ values: [Int]) -> String { values.enumerated().filter { $0.offset % 5 == 0 }.map { String($0.element) }.joined(separator: ",") }

        let before = distance()
        let up = await gesture(40, steps: 150)
        let after = await sample(20)
        log("perf:   scroll up while streaming: \(before) pt from the bottom, during the gesture \(every5(up)), then \(every5(after))")
        _ = await gesture(-40, steps: distance() / 40 + 20)
        log("perf:   scrolled back down: \(every5(await sample(15))) pt from the bottom while streaming")
        await run.value
    }

    // MARK: Agent pane

    /// One sub-agent with 150 tool calls, shown in the Inspector's Agents
    /// tab: checks the view opens at the bottom, then drags through it.
    private static func agentPane(appState: AppState, chat: ChatState, log: (String) -> Void) async -> Phase? {
        let task = "bench-long-agent"
        let id = "agent-long"
        var events: [AgentEvent] = [
            .taskStarted(taskId: task, objective: "Long sub-agent", continued: false),
            .toolCallStarted(taskId: task, callId: "\(task)-spawn", name: "spawn_agents", arguments: .object([:])),
            .agentStarted(taskId: task, agentId: id, agentTaskId: "\(id)-task", name: "long-agent", mode: .explore, objective: "Read every module."),
        ]
        for c in 0..<150 {
            let call = "\(id)-c\(c)"
            events.append(.agentProgress(taskId: task, agentId: id, inner: .toolCallStarted(taskId: "\(id)-task", callId: call, name: "read_file", arguments: .object(["path": .string("crates/bench/src/module_\(c).rs")]))))
            events.append(.agentProgress(taskId: task, agentId: id, inner: .toolCallOutput(taskId: "\(id)-task", callId: call, name: "read_file", content: sampleOutput, fullLength: sampleOutput.utf8.count, isError: false)))
            events.append(.agentProgress(taskId: task, agentId: id, inner: .toolCallFinished(taskId: "\(id)-task", callId: call, name: "read_file", success: true, summary: "read 40 lines", durationMs: 3)))
        }
        events.append(.agentFinished(taskId: task, agentId: id, report: report(id: id, name: "long-agent")))
        events.append(.toolCallFinished(taskId: task, callId: "\(task)-spawn", name: "spawn_agents", success: true, summary: "1 agent finished", durationMs: 9_000))
        events.append(.taskCompleted(taskId: task, outcome: outcome(task)))
        for event in events { chat.apply(event) }

        let (visible, tab) = (appState.inspectorVisible, appState.inspectorTab)
        defer {
            appState.inspectorVisible = visible
            appState.inspectorTab = tab
            chat.inspectedAgentId = nil
        }
        appState.inspectorVisible = true
        appState.inspectorTab = .agents
        chat.inspectedAgentId = id
        try? await Task.sleep(for: .seconds(1))
        guard let window = NSApp.windows.first(where: { $0.isVisible && $0.contentView != nil }),
              let scrollView = inspectorScrollView(in: window.contentView!) else {
            log("perf:   agent pane: no scroll view found")
            return nil
        }
        let clip = scrollView.contentView
        let bottom = (scrollView.documentView?.frame.height ?? 0) - clip.bounds.height
        let idle = await idleHeightChanges(scrollView, seconds: 2)
        log("perf:   agent pane: opened \(Int(bottom - clip.bounds.origin.y)) pt above the bottom (content \(Int(scrollView.documentView?.frame.height ?? 0)) pt); content height changed \(idle) times in 2 s at rest")
        var corrections = ScrollCorrections()
        let phase = await measure("agent-pane-scroll", items: 150) { corrections = await scrollThrough(scrollView) }
        log("perf:   agent pane scroll: p95 \(fmt(phase.p95Ms)) ms max \(fmt(phase.maxMs)) ms · \(corrections.steps) steps, reached top \(corrections.reachedTop), content height changed after \(corrections.heightChanges) steps\(corrections.visualChecked > 0 ? ", \(corrections.visualJumps) visible jumps in \(corrections.visualChecked) checked steps (largest \(corrections.visualLargest) pt)" : "")")
        return phase
    }

    /// The rightmost vertical scroll view with something to scroll: the
    /// Inspector's agent view.
    private static func inspectorScrollView(in view: NSView) -> NSScrollView? {
        var best: NSScrollView?
        func walk(_ view: NSView) {
            if let scroll = view as? NSScrollView, (scroll.documentView?.frame.height ?? 0) > scroll.contentView.bounds.height + 1 {
                let x = scroll.convert(scroll.bounds, to: nil).minX
                if x > (best.map { $0.convert($0.bounds, to: nil).minX } ?? -1) { best = scroll }
            }
            view.subviews.forEach(walk)
        }
        walk(view)
        return best
    }

    // MARK: Scrolling

    /// Steps after which the offset was not where the step put it. With
    /// lazy rows and a `TailWindow` these are SwiftUI re-anchoring the
    /// visible rows while estimated heights resolve or items load above
    /// (the content on screen stays put; the scroller's knob resizes), not
    /// jumps the user sees. `visualJumps` (with `CTX_PERFBENCH_VISUAL=1`)
    /// compares screenshots of consecutive steps instead: steps after which
    /// the content on screen moved by something other than the step.
    struct ScrollCorrections {
        var steps = 0
        var count = 0
        var largest: Double = 0
        var reachedTop = false
        var visualJumps = 0
        var visualLargest = 0
        var visualChecked = 0
        /// Steps after which the content's height had changed (the
        /// scroller's knob resized), and the largest change.
        var heightChanges = 0
        var largestHeightChange: Double = 0
    }

    /// How many times the content's height changes while the view sits
    /// still for `seconds` (a window that loads and unloads in a loop
    /// shows up here).
    private static func idleHeightChanges(_ scrollView: NSScrollView, seconds: Double) async -> Int {
        var changes = 0
        var last = scrollView.documentView?.frame.height ?? 0
        let end = Date().addingTimeInterval(seconds)
        while Date() < end {
            try? await Task.sleep(for: .milliseconds(16))
            let height = scrollView.documentView?.frame.height ?? 0
            if abs(height - last) > 0.5 { changes += 1 }
            last = height
        }
        return changes
    }

    /// Drags the transcript to the top and back down in 60 pt steps every
    /// 16 ms, as a scroller drag does (a live scroll, so the pane stops
    /// following the bottom). An edge counts as reached once 30 steps in a
    /// row stay there: a windowed list loads more as it is reached.
    private static func scrollThrough(_ given: NSScrollView? = nil) async -> ScrollCorrections {
        var result = ScrollCorrections()
        guard let window = NSApp.windows.first(where: { $0.isVisible && $0.contentView != nil }),
              let scrollView = given ?? transcriptScrollView(in: window.contentView!) else { return result }
        let clip = scrollView.contentView
        let visual = ProcessInfo.processInfo.environment["CTX_PERFBENCH_VISUAL"] == "1"
        let scale = window.backingScaleFactor
        NotificationCenter.default.post(name: NSScrollView.willStartLiveScrollNotification, object: scrollView)
        defer { NotificationCenter.default.post(name: NSScrollView.didEndLiveScrollNotification, object: scrollView) }
        for direction in [1, -1] {
            var settled = 0
            var previous = visual ? RowHashes(scrollView) : nil
            for _ in 0..<20_000 {
                let top = -scrollView.contentInsets.top
                let bottom = (scrollView.documentView?.frame.height ?? 0) - clip.bounds.height
                let before = clip.bounds.origin.y
                let heightBefore = scrollView.documentView?.frame.height ?? 0
                let target = min(max(before - 60 * Double(direction), top), bottom)
                clip.scroll(to: NSPoint(x: clip.bounds.origin.x, y: target))
                scrollView.reflectScrolledClipView(clip)
                try? await Task.sleep(for: .milliseconds(16))
                result.steps += 1
                let heightChange = abs((scrollView.documentView?.frame.height ?? 0) - heightBefore)
                if heightChange > 0.5 {
                    result.heightChanges += 1
                    result.largestHeightChange = max(result.largestHeightChange, heightChange)
                }
                let error = abs(clip.bounds.origin.y - target)
                if error > 4 {
                    result.count += 1
                    result.largest = max(result.largest, error)
                }
                if visual, let old = previous {
                    let new = RowHashes(scrollView)
                    // Content moves down the screen when scrolling up.
                    let expected = Int(((before - target) * scale).rounded())
                    if let shift = new.shift(from: old) {
                        result.visualChecked += 1
                        let off = abs(shift - expected)
                        if off > Int(2 * scale) {
                            result.visualJumps += 1
                            result.visualLargest = max(result.visualLargest, Int(Double(off) / scale))
                        }
                    }
                    previous = new
                }
                let atEdge = direction == 1 ? target <= top + 0.5 : target >= bottom - 0.5
                settled = atEdge ? settled + 1 : 0
                if settled >= 30 {
                    if direction == 1 { result.reachedTop = true }
                    break
                }
            }
        }
        return result
    }

    /// One hash per pixel row of a view's rendering, to find how far the
    /// content moved between two renderings.
    private struct RowHashes {
        var rows: [Int] = []

        init(_ view: NSView) {
            guard let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return }
            view.cacheDisplay(in: view.bounds, to: rep)
            guard let data = rep.bitmapData else { return }
            let stride = rep.bytesPerRow
            rows = (0..<rep.pixelsHigh).map { y in
                var hasher = Hasher()
                hasher.combine(bytes: UnsafeRawBufferPointer(start: data + y * stride, count: stride))
                return hasher.finalize()
            }
        }

        /// Pixel rows the content moved down since `old` (negative: up),
        /// voted by rows that appear exactly once in both renderings.
        func shift(from old: RowHashes) -> Int? {
            var oldIndex: [Int: Int] = [:]
            var oldCount: [Int: Int] = [:]
            for (y, hash) in old.rows.enumerated() {
                oldIndex[hash] = y
                oldCount[hash, default: 0] += 1
            }
            var newCount: [Int: Int] = [:]
            for hash in rows { newCount[hash, default: 0] += 1 }
            var votes: [Int: Int] = [:]
            for (y, hash) in rows.enumerated() where newCount[hash] == 1 && oldCount[hash] == 1 {
                votes[y - oldIndex[hash]!, default: 0] += 1
            }
            guard let best = votes.max(by: { $0.value < $1.value }), best.value >= 20 else { return nil }
            return best.key
        }
    }

    /// The tallest vertical scroll view in the window: the transcript.
    private static func transcriptScrollView(in view: NSView) -> NSScrollView? {
        var best: NSScrollView?
        func walk(_ view: NSView) {
            if let scroll = view as? NSScrollView, (scroll.documentView?.frame.height ?? 0) > (best?.documentView?.frame.height ?? 0) {
                best = scroll
            }
            view.subviews.forEach(walk)
        }
        walk(view)
        return best
    }

    private static func fmt(_ value: Double) -> String { String(format: "%.1f", value) }

    // MARK: Measuring

    private static func measure(_ name: String, items: Int, _ body: () async -> Void) async -> Phase {
        let probe = LatencyProbe()
        let cpu0 = processCPU()
        let started = Date()
        probe.start()
        await body()
        let samples = probe.stop()
        let seconds = Date().timeIntervalSince(started)
        let sorted = samples.sorted()
        func pct(_ p: Double) -> Double { sorted.isEmpty ? 0 : sorted[min(sorted.count - 1, Int(Double(sorted.count) * p))] }
        return Phase(
            name: name, items: items, seconds: seconds, samples: sorted.count,
            p50Ms: pct(0.5), p95Ms: pct(0.95), p99Ms: pct(0.99), maxMs: sorted.last ?? 0,
            stalledMs: sorted.reduce(0) { $0 + max(0, $1 - 16.7) },
            cpuMs: (processCPU() - cpu0) * 1000
        )
    }

    private static func processCPU() -> Double {
        var usage = rusage()
        getrusage(RUSAGE_SELF, &usage)
        func seconds(_ t: timeval) -> Double { Double(t.tv_sec) + Double(t.tv_usec) / 1_000_000 }
        return seconds(usage.ru_utime) + seconds(usage.ru_stime)
    }

    /// Main-queue wait sampler.
    private final class LatencyProbe: @unchecked Sendable {
        private let queue = DispatchQueue(label: "perfbench.probe", qos: .userInteractive)
        private var timer: DispatchSourceTimer?
        private let lock = NSLock()
        private var samples: [Double] = []
        private var inFlight = false

        func start() {
            let timer = DispatchSource.makeTimerSource(queue: queue)
            timer.schedule(deadline: .now(), repeating: .milliseconds(8), leeway: .milliseconds(1))
            timer.setEventHandler { [self] in
                lock.lock()
                // One probe at a time: a stalled main thread counts once, for its full length.
                if inFlight { lock.unlock(); return }
                inFlight = true
                lock.unlock()
                let posted = DispatchTime.now().uptimeNanoseconds
                DispatchQueue.main.async { [self] in
                    let waited = Double(DispatchTime.now().uptimeNanoseconds - posted) / 1_000_000
                    lock.lock()
                    samples.append(waited)
                    inFlight = false
                    lock.unlock()
                }
            }
            self.timer = timer
            timer.resume()
        }

        func stop() -> [Double] {
            timer?.cancel()
            timer = nil
            lock.lock()
            defer { lock.unlock() }
            return samples
        }
    }

    // MARK: Workloads

    /// About 9 s of a live run: the lead thinks, deploys four agents that
    /// stream text and read files for 6 s, then streams its answer.
    private static func streamRun(_ chat: ChatState, index: Int) async {
        let task = "bench-live-\(index)"
        let tick: Duration = .milliseconds(15)
        chat.apply(.taskStarted(taskId: task, objective: "Live benchmark run \(index)", continued: false))
        chat.apply(.stateChanged(taskId: task, from: .preparing, to: .investigating))
        chat.apply(.modelRequestStarted(taskId: task, step: 1, estimatedPromptTokens: 24_000))
        for i in 0..<60 {
            chat.apply(.modelReasoningDelta(taskId: task, text: words[i % words.count] + " "))
            try? await Task.sleep(for: tick)
        }
        chat.apply(.modelResponse(taskId: task, text: nil, toolCallCount: 1, usage: nil))
        chat.apply(.toolCallStarted(taskId: task, callId: "\(task)-spawn", name: "spawn_agents", arguments: .object([:])))
        let agents = (0..<4).map { "agent-\(index)-\($0)" }
        for (n, id) in agents.enumerated() {
            chat.apply(.agentStarted(taskId: task, agentId: id, agentTaskId: "\(id)-task", name: "bench-agent-\(n)", mode: n % 2 == 0 ? .explore : .implement, objective: "Investigate area \(n) of the codebase and report."))
        }
        var calls = 0
        for t in 0..<400 {
            for (n, id) in agents.enumerated() {
                let phase = (t + n * 9) % 40
                let inner: AgentEvent
                switch phase {
                case 0: inner = .modelRequestStarted(taskId: "\(id)-task", step: t / 40 + 1, estimatedPromptTokens: 8_000)
                case 1...30: inner = .modelTextDelta(taskId: "\(id)-task", text: words[(t + n) % words.count] + " ")
                case 31: inner = .modelResponse(taskId: "\(id)-task", text: nil, toolCallCount: 1, usage: nil)
                case 32:
                    calls += 1
                    inner = .toolCallStarted(taskId: "\(id)-task", callId: "\(id)-c\(calls)", name: "read_file", arguments: .object(["path": .string("crates/bench/src/module_\(calls % 30).rs")]))
                case 33:
                    inner = .toolCallOutput(taskId: "\(id)-task", callId: "\(id)-c\(calls)", name: "read_file", content: sampleOutput, fullLength: sampleOutput.utf8.count, isError: false)
                case 34: inner = .toolCallFinished(taskId: "\(id)-task", callId: "\(id)-c\(calls)", name: "read_file", success: true, summary: "read 40 lines", durationMs: 3)
                default: continue
                }
                chat.apply(.agentProgress(taskId: task, agentId: id, inner: inner))
            }
            try? await Task.sleep(for: tick)
        }
        for (n, id) in agents.enumerated() {
            chat.apply(.agentFinished(taskId: task, agentId: id, report: report(id: id, name: "bench-agent-\(n)")))
        }
        chat.apply(.toolCallFinished(taskId: task, callId: "\(task)-spawn", name: "spawn_agents", success: true, summary: "4 agents finished", durationMs: 6_000))
        chat.apply(.modelRequestStarted(taskId: task, step: 2, estimatedPromptTokens: 30_000))
        for chunk in chunks(answer, size: 12) {
            chat.apply(.modelTextDelta(taskId: task, text: chunk))
            try? await Task.sleep(for: tick)
        }
        chat.apply(.modelResponse(taskId: task, text: answer, toolCallCount: 0, usage: usage))
        chat.apply(.taskCompleted(taskId: task, outcome: outcome(task)))
    }

    /// Editor tab and back to the chat, four times: the chat is rebuilt
    /// each time it comes back.
    private static func switchTabs(_ appState: AppState) async {
        for _ in 0..<4 {
            appState.sidebarItem = .editor
            try? await Task.sleep(for: .milliseconds(600))
            appState.sidebarItem = .chat
            try? await Task.sleep(for: .milliseconds(600))
        }
    }

    /// 4 s of typing into the composer at 25 characters per second.
    private static func typing(_ chat: ChatState) async {
        let text = "Can you also check whether the retry path handles the upstream 402 case and add a test for it please"
        chat.draft = ""
        for character in text.prefix(100) {
            chat.draft.append(character)
            try? await Task.sleep(for: .milliseconds(40))
        }
    }

    /// One finished run: a prompt, three file reads, sometimes three
    /// sub-agents with five reads each, and a markdown answer.
    static func seededRun(_ index: Int, agents: Bool) -> [AgentEvent] {
        let task = "bench-\(index)"
        var events: [AgentEvent] = [
            .taskStarted(taskId: task, objective: "Benchmark prompt \(index): explain how `ChatState` applies events and where the transcript grows.", continued: false),
            .modelRequestStarted(taskId: task, step: 1, estimatedPromptTokens: 20_000),
            .modelReasoningDelta(taskId: task, text: "Need to look at the reducer and the views that read the transcript."),
            .modelTextDelta(taskId: task, text: "I'll read the reducer first."),
            .modelResponse(taskId: task, text: "I'll read the reducer first.", toolCallCount: 3, usage: usage),
        ]
        for k in 0..<3 {
            let id = "\(task)-r\(k)"
            events.append(.toolCallStarted(taskId: task, callId: id, name: "read_file", arguments: .object(["path": .string("apps/macos/ContextAgent/App/File\(k).swift")])))
            events.append(.toolCallOutput(taskId: task, callId: id, name: "read_file", content: output(index), fullLength: output(index).utf8.count, isError: false))
            events.append(.toolCallFinished(taskId: task, callId: id, name: "read_file", success: true, summary: "read 40 lines", durationMs: 2))
        }
        if agents {
            events.append(.modelRequestStarted(taskId: task, step: 2, estimatedPromptTokens: 22_000))
            events.append(.modelResponse(taskId: task, text: nil, toolCallCount: 1, usage: usage))
            events.append(.toolCallStarted(taskId: task, callId: "\(task)-spawn", name: "spawn_agents", arguments: .object([:])))
            let ids = (0..<3).map { "agent-\(index)-\($0)" }
            for (n, id) in ids.enumerated() {
                events.append(.agentStarted(taskId: task, agentId: id, agentTaskId: "\(id)-task", name: "agent-\(n)", mode: .explore, objective: "Explore part \(n)."))
            }
            for (n, id) in ids.enumerated() {
                for c in 0..<5 {
                    let call = "\(id)-c\(c)"
                    events.append(.agentProgress(taskId: task, agentId: id, inner: .modelTextDelta(taskId: "\(id)-task", text: "Checking module \(c). ")))
                    events.append(.agentProgress(taskId: task, agentId: id, inner: .toolCallStarted(taskId: "\(id)-task", callId: call, name: "search_code", arguments: .object(["query": .string("fn apply_\(c)")]))))
                    events.append(.agentProgress(taskId: task, agentId: id, inner: .toolCallOutput(taskId: "\(id)-task", callId: call, name: "search_code", content: output(index), fullLength: output(index).utf8.count, isError: false)))
                    events.append(.agentProgress(taskId: task, agentId: id, inner: .toolCallFinished(taskId: "\(id)-task", callId: call, name: "search_code", success: true, summary: "12 matches", durationMs: 4)))
                }
                events.append(.agentFinished(taskId: task, agentId: id, report: report(id: id, name: "agent-\(n)")))
            }
            events.append(.toolCallFinished(taskId: task, callId: "\(task)-spawn", name: "spawn_agents", success: true, summary: "3 agents finished", durationMs: 9_000))
        }
        events.append(.modelRequestStarted(taskId: task, step: 3, estimatedPromptTokens: 26_000))
        events.append(.modelTextDelta(taskId: task, text: answer(index)))
        events.append(.modelResponse(taskId: task, text: answer(index), toolCallCount: 0, usage: usage))
        events.append(.taskCompleted(taskId: task, outcome: outcome(task)))
        return events
    }

    // MARK: Fixtures

    private static let usage = TokenUsage(promptTokens: 20_000, completionTokens: 600, cachedPromptTokens: 12_000, reasoningTokens: 120, source: .reported)

    private static func report(id: String, name: String) -> AgentReport {
        AgentReport(agentId: id, name: name, mode: .explore, status: .completed, summary: "Found the reducer in `App/ChatState.swift`; the transcript grows in `Transcript.append`.", modifiedFiles: [], unresolved: [], steps: 6, toolCalls: 5, usage: usage, durationMs: 9_000)
    }

    private static func outcome(_ task: String) -> TaskOutcome {
        TaskOutcome(taskId: task, status: .completed, summary: "Explained the reducer.", modifiedFiles: [], validation: [], unresolved: [], steps: 3, modelCalls: 3, toolCalls: 4, usage: usage, costUsd: 0.04, durationMs: 14_000)
    }

    private static let words = "the reducer applies each event to the chat state and the transcript keeps committed items while live turns stream in separately so views".split(separator: " ").map(String.init)

    private static let sampleOutput = (1...40).map { "\($0)\tlet value\($0) = transcript.items[\($0)].id // \(String(repeating: "x", count: $0 % 30))" }.joined(separator: "\n")

    private static let answer = """
    ## How events reach the transcript

    `ChatState.apply(_:)` is the reducer. Streaming text is coalesced every 33 ms, then committed when the model responds:

    1. **Deltas** go to `pendingText` and flush into `live`.
    2. **Tool calls** open a `ToolGroup` in `liveTools`.
    3. **Sub-agents** collect in `liveAgents` until each one reports.

    ```swift
    func apply(_ event: AgentEvent, date: Date = Date()) {
        switch event {
        case .modelTextDelta(_, let text):
            pendingText += text
            scheduleFlush()
        default:
            break
        }
    }
    ```

    The views read `chat.transcript.items` in `ChatView.swift` and `TranscriptItemViews.swift`; see also `Models/Transcript.swift`.
    """

    /// Fixtures tagged with the run's number on every line, so that no two
    /// runs render alike (the visual scroll check matches rendered rows).
    private static func output(_ index: Int) -> String {
        sampleOutput.split(separator: "\n").map { "\($0) // run \(index)" }.joined(separator: "\n")
    }

    private static func answer(_ index: Int) -> String {
        answer.split(separator: "\n", omittingEmptySubsequences: false).map { $0.isEmpty ? "" : "\($0) (\(index))" }.joined(separator: "\n")
    }

    private static func chunks(_ text: String, size: Int) -> [String] {
        var out: [String] = []
        var index = text.startIndex
        while index < text.endIndex {
            let end = text.index(index, offsetBy: size, limitedBy: text.endIndex) ?? text.endIndex
            out.append(String(text[index..<end]))
            index = end
        }
        return out
    }
}
