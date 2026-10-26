//! `ctx serve --mock` driven over stdin/stdout: the essential desktop flow
//! (initialize, project/open, task/run, session/history, shutdown) with the
//! scripted mock provider, asserting the protocol frames the macOS app
//! relies on.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(120);

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git").args(args).current_dir(repo).stdout(Stdio::null()).stderr(Stdio::null()).status().expect("git is installed");
    assert!(status.success(), "git {args:?} failed");
}

/// A small git repository with a README, two source files, one commit and
/// one uncommitted change.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    let mut readme = String::from("# Fixture project\n\n");
    for i in 1..=28 {
        readme.push_str(&format!("Line {i} of the README describes feature {i}.\n"));
    }
    std::fs::write(root.join("README.md"), readme).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n").unwrap();
    std::fs::write(root.join("src/util.py"), "def clamp(x, lo, hi):\n    return max(lo, min(hi, x))\n").unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "commit", "-q", "-m", "initial"]);
    // Check the files out again as a clone has them (CRLF under Git for
    // Windows' core.autocrlf), so a worktree checkout matches them byte for byte.
    for file in ["README.md", "src/lib.rs", "src/util.py"] {
        std::fs::remove_file(root.join(file)).unwrap();
    }
    git(root, &["checkout", "--", "."]);
    std::fs::write(root.join("src/lib.rs"), "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn sub(a: i32, b: i32) -> i32 {\n    a - b\n}\n").unwrap();
    dir
}

/// A JSON-RPC client over the child's pipes. Every stdout line must be a
/// JSON object; anything else is recorded and fails the test at the end.
struct Client {
    child: Child,
    stdin: ChildStdin,
    rx: mpsc::Receiver<Result<Value, String>>,
    notifications: Vec<Value>,
    /// Every frame in arrival order (responses and notifications), for
    /// ordering assertions.
    trace: Vec<Value>,
    next_id: i64,
}

impl Client {
    fn spawn(data_dir: &Path) -> Self {
        Self::spawn_with(data_dir, |_| {})
    }

    fn spawn_with(data_dir: &Path, configure: impl FnOnce(&mut Command)) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ctx"));
        command.args(["serve", "--mock", "--data-dir"]).arg(data_dir).env_remove("OPENAI_API_KEY").env_remove("CTX_MODEL_NAME").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        configure(&mut command);
        let mut child = command.spawn().expect("spawn ctx serve");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let parsed = serde_json::from_str::<Value>(&line).ok().filter(|v| v.is_object()).ok_or_else(|| line.clone());
                if tx.send(parsed).is_err() {
                    break;
                }
            }
        });
        Self { child, stdin, rx, notifications: Vec::new(), trace: Vec::new(), next_id: 1 }
    }

    fn recv(&mut self, deadline: Instant) -> Value {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "timed out waiting for a frame; buffered notifications: {:#?}", self.notifications);
            match self.rx.recv_timeout(remaining) {
                Ok(Ok(v)) => {
                    self.trace.push(v.clone());
                    return v;
                }
                Ok(Err(line)) => panic!("non-JSON output on stdout: {line:?}"),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => panic!("server closed stdout; notifications: {:#?}", self.notifications),
            }
        }
    }

    fn send_raw(&mut self, line: &str) {
        writeln!(self.stdin, "{line}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Write one request frame without waiting; `wait_response` collects it.
    fn send(&mut self, method: &str, params: Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send_raw(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string());
        id
    }

    /// The response to `id`; responses to other ids are kept for their own
    /// `wait_response`, notifications are buffered.
    fn wait_response(&mut self, id: i64) -> Value {
        if let Some(i) = self.notifications.iter().position(|n| n.get("id").map(|i| *i == json!(id)).unwrap_or(false)) {
            return self.notifications.remove(i);
        }
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let msg = self.recv(deadline);
            if msg.get("id").map(|i| *i == json!(id)).unwrap_or(false) {
                return msg;
            }
            self.notifications.push(msg);
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.send(method, params);
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let msg = self.recv(deadline);
            if msg.get("id").map(|i| *i == json!(id)).unwrap_or(false) {
                return msg;
            }
            assert!(msg.get("id").map(|i| i.is_null()).unwrap_or(true), "unexpected response {msg}");
            self.notifications.push(msg);
        }
    }

    fn result(&mut self, method: &str, params: Value) -> Value {
        let resp = self.request(method, params);
        assert!(resp.get("error").is_none(), "{method} failed: {resp}");
        resp["result"].clone()
    }

    fn wait_notification(&mut self, pred: impl Fn(&Value) -> bool) -> Value {
        if let Some(i) = self.notifications.iter().position(|n| pred(n)) {
            return self.notifications.remove(i);
        }
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let msg = self.recv(deadline);
            assert!(msg.get("id").map(|i| i.is_null()).unwrap_or(true), "unexpected response {msg}");
            if pred(&msg) {
                return msg;
            }
            self.notifications.push(msg);
        }
    }

    /// Collect whatever arrives for `period` (for "nothing more comes" checks).
    fn drain_for(&mut self, period: Duration) {
        let deadline = Instant::now() + period;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) {
            match self.rx.recv_timeout(remaining) {
                Ok(Ok(v)) => {
                    assert!(v.get("id").map(|i| i.is_null()).unwrap_or(true), "unexpected response {v}");
                    self.trace.push(v.clone());
                    self.notifications.push(v);
                }
                Ok(Err(line)) => panic!("non-JSON output on stdout: {line:?}"),
                Err(_) => break,
            }
        }
    }

    /// Position in the trace of the first frame matching `pred` at or after `from`.
    fn trace_position(&self, from: usize, pred: impl Fn(&Value) -> bool) -> Option<usize> {
        self.trace.iter().enumerate().skip(from).find(|(_, f)| pred(f)).map(|(i, _)| i)
    }

    fn take_events(&mut self, session_id: &str) -> Vec<Value> {
        let (events, rest): (Vec<Value>, Vec<Value>) = std::mem::take(&mut self.notifications).into_iter().partition(|n| n["method"] == "event" && n["params"]["session_id"] == session_id);
        self.notifications = rest;
        events.into_iter().map(|n| n["params"]["event"].clone()).collect()
    }
}

#[test]
fn mock_serve_round_trip() {
    let data = tempfile::tempdir().unwrap();
    let repo = fixture();
    let mut c = Client::spawn(data.path());

    let init = c.result("initialize", json!({}));
    assert_eq!(init["protocol_version"], 1, "{init}");
    assert_eq!(init["mock"], true);
    assert!(init["model"]["name"].is_string() && init["model"].get("api_key_set").is_some(), "{init}");
    assert_eq!(init["permissions"]["profile"], "approval_required");
    let model = init["model"]["name"].as_str().unwrap().to_string();

    let info = c.result("project/open", json!({"path": repo.path()}));
    let pid = info["project_id"].as_str().unwrap().to_string();
    assert_eq!(pid.len(), 24, "{info}");
    assert_eq!(info["root"], json!(ctx_core::platform::canonicalize(repo.path()).unwrap()));
    assert_eq!(info["git"]["dirty_files"], 1, "{info}");
    assert_eq!(info["permission_profile"], "approval_required");
    assert_eq!(info["sandbox"], "none");
    assert!(info["running_task"].is_null());

    let ready = c.wait_notification(|n| n["method"] == "index/ready" && n["params"]["project_id"] == pid.as_str());
    assert!(ready["params"]["error"].is_null(), "{ready}");
    assert_eq!(ready["params"]["rebuilt"], false);
    assert!(ready["params"]["generation"].as_u64().unwrap() >= 1);

    let status = c.result("project/status", json!({"project_id": pid}));
    assert!(status["index"]["files"].as_u64().unwrap() >= 3, "{status}");
    assert!(status["index"]["languages"].as_array().unwrap().iter().any(|l| l["language"] == "rust"), "{status}");

    let session = c.result("session/create", json!({"project_id": pid}));
    let sid = session["id"].as_str().unwrap().to_string();
    assert_eq!(session["mode"], "desktop");
    assert_eq!(session["title"], "New chat");
    assert_eq!(session["model"], json!(model));

    let prompt = "Summarize this repository";
    let accepted = c.result("task/run", json!({"project_id": pid, "session_id": sid, "prompt": prompt}));
    assert_eq!(accepted["accepted"], true);
    let busy = c.request("task/run", json!({"project_id": pid, "session_id": sid, "prompt": "again"}));
    assert_eq!(busy["error"]["code"], -32002, "{busy}");

    let finished = c.wait_notification(|n| n["method"] == "task/finished" && n["params"]["project_id"] == pid.as_str());
    let fp = &finished["params"];
    assert_eq!(fp["status"], "completed", "{finished}");
    assert!(fp["error"].is_null() && fp["error_kind"].is_null(), "{finished}");
    assert_eq!(fp["session_id"], json!(sid));
    let task_id = fp["task_id"].as_str().unwrap().to_string();

    let events = c.take_events(&sid);
    let kinds: Vec<&str> = events.iter().filter_map(|e| e["event"].as_str()).collect();
    assert_eq!(kinds.first(), Some(&"task_started"), "{kinds:?}");
    for required in ["model_text_delta", "tool_call_started", "tool_call_finished", "tool_call_output", "model_response", "usage", "task_completed"] {
        assert!(kinds.contains(&required), "missing {required} in {kinds:?}");
    }
    let started: Vec<&str> = events.iter().filter(|e| e["event"] == "tool_call_started").filter_map(|e| e["name"].as_str()).collect();
    assert!(started.contains(&"list_files") && started.contains(&"read_file"), "{started:?}");
    let outputs: Vec<&Value> = events.iter().filter(|e| e["event"] == "tool_call_output").collect();
    assert!(outputs.len() >= 2, "{kinds:?}");
    for out in &outputs {
        assert!(!out["content"].as_str().unwrap_or("").is_empty(), "{out}");
        assert_eq!(out["is_error"], false);
        assert!(out["full_len"].is_u64() && out["call_id"].is_string(), "{out}");
        let call_id = &out["call_id"];
        let finished_at = events.iter().position(|e| e["event"] == "tool_call_finished" && &e["call_id"] == call_id).expect("tool_call_finished precedes its output");
        let output_at = events.iter().position(|e| std::ptr::eq(e, *out)).unwrap();
        assert!(finished_at < output_at, "tool_call_output arrived before tool_call_finished");
    }
    let completed = events.iter().find(|e| e["event"] == "task_completed").unwrap();
    assert_eq!(completed["outcome"]["status"], "completed");
    assert_eq!(completed["outcome"]["task_id"], json!(task_id));
    assert_eq!(completed["outcome"]["modified_files"], json!([]));

    let history = c.result("session/history", json!({"project_id": pid, "session_id": sid}));
    assert_eq!(history["session"]["title"], prompt, "{history}");
    let turns = history["turns"].as_array().unwrap();
    assert_eq!(turns.len(), 1);
    let turn = &turns[0];
    assert_eq!(turn["task"]["id"], json!(task_id));
    assert_eq!(turn["task"]["status"], "completed");
    let message_kinds: Vec<&str> = turn["messages"].as_array().unwrap().iter().filter_map(|m| m["kind"].as_str()).collect();
    assert_eq!(message_kinds.first(), Some(&"user"), "{message_kinds:?}");
    assert_eq!(turn["messages"][0]["text"], prompt);
    assert!(message_kinds.contains(&"assistant") && message_kinds.contains(&"tool_result"), "{message_kinds:?}");
    let tool_result = turn["messages"].as_array().unwrap().iter().find(|m| m["kind"] == "tool_result").unwrap();
    assert!(tool_result["call_id"].is_string() && tool_result["tool_name"].is_string() && tool_result["content"].is_string() && tool_result["is_error"].is_boolean(), "{tool_result}");
    assert!(!turn["events"].as_array().unwrap().is_empty());
    assert!(turn["events"].as_array().unwrap().iter().all(|e| e["event"] != "model_text_delta" && e["event"] != "tool_output"), "deltas and raw rows are not part of the history");
    assert_eq!(turn["outcome"]["status"], "completed", "{turn}");
    assert!(turn["usage"]["requests"].as_u64().unwrap() >= 3, "{turn}");

    let sessions = c.result("sessions/list", json!({"project_id": pid}));
    let list = sessions["sessions"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["title"], prompt);
    assert_eq!(list[0]["task_count"], 1);
    assert_eq!(list[0]["last_status"], "completed");

    let git_status = c.result("git/status", json!({"project_id": pid}));
    let lib = git_status["files"].as_array().unwrap().iter().find(|f| f["path"] == "src/lib.rs").expect("the uncommitted change is listed");
    assert_eq!(lib["status"], "M");
    assert_eq!(lib["staged"], false);
    assert_eq!(lib["insertions"], 4, "the uncommitted `sub` function: {lib}");
    assert_eq!(lib["deletions"], 0, "{lib}");
    assert!(git_status["branch"].is_string() && git_status["head"].is_string(), "{git_status}");

    let readme = c.result("file/read", json!({"project_id": pid, "path": "README.md"}));
    assert!(readme["content"].as_str().unwrap().starts_with("# Fixture project"));
    assert_eq!(readme["language"], "markdown");
    assert_eq!(readme["truncated"], false);
    let escape = c.request("file/read", json!({"project_id": pid, "path": "../../etc/passwd"}));
    assert_eq!(escape["error"]["code"], -32000, "{escape}");
    assert_eq!(escape["error"]["data"]["kind"], "permission_denied", "{escape}");

    let idle = c.request("task/cancel", json!({"project_id": pid}));
    assert_eq!(idle["error"]["code"], -32003, "{idle}");

    assert_eq!(c.result("shutdown", json!({})), json!({}));
    let status = c.child.wait().unwrap();
    assert_eq!(status.code(), Some(0), "ctx serve exited with {status}");
    // Whatever arrived after the last response must still be well-formed.
    while let Ok(frame) = c.rx.try_recv() {
        assert!(frame.is_ok(), "non-JSON output on stdout: {frame:?}");
    }
}

/// `task/run` `images` and `files`: bad files are refused before a run
/// starts; good ones are stored with the user message and listed by
/// `session/history`.
/// Sessions of one project run at the same time: each has its own run slot,
/// `project/status` lists every running task, and `task/cancel` with a
/// session id stops only that session's run.
#[test]
fn sessions_of_one_project_run_in_parallel() {
    let data = tempfile::tempdir().unwrap();
    let repo = fixture();
    // Paced so the runs overlap long enough to observe.
    let mut c = Client::spawn_with(data.path(), |cmd| {
        cmd.env("CTX_MOCK_PACE_MS", "40");
    });
    c.result("initialize", json!({}));
    let pid = c.result("project/open", json!({"path": repo.path()}))["project_id"].as_str().unwrap().to_string();
    c.wait_notification(|n| n["method"] == "index/ready" && n["params"]["project_id"] == pid.as_str());
    let first = c.result("session/create", json!({"project_id": pid}))["id"].as_str().unwrap().to_string();
    let second = c.result("session/create", json!({"project_id": pid}))["id"].as_str().unwrap().to_string();

    assert_eq!(c.result("task/run", json!({"project_id": pid, "session_id": first, "prompt": "Summarize"}))["accepted"], true);
    assert_eq!(c.result("task/run", json!({"project_id": pid, "session_id": second, "prompt": "Summarize too"}))["accepted"], true, "another session of the project runs alongside");
    let busy = c.request("task/run", json!({"project_id": pid, "session_id": first, "prompt": "again"}));
    assert_eq!(busy["error"]["code"], -32002, "one run per session: {busy}");
    assert!(busy["error"]["message"].as_str().unwrap().contains("session"), "{busy}");

    let status = c.result("project/status", json!({"project_id": pid}));
    let running: Vec<&str> = status["running_tasks"].as_array().unwrap().iter().map(|t| t["session_id"].as_str().unwrap()).collect();
    assert_eq!(running, vec![first.as_str(), second.as_str()], "{status}");
    assert_eq!(status["running_task"]["session_id"], json!(first), "the oldest, for older clients: {status}");

    assert_eq!(c.result("task/cancel", json!({"project_id": pid, "session_id": first})), json!({}));
    let stopped = c.wait_notification(|n| n["method"] == "task/finished" && n["params"]["session_id"] == first.as_str());
    assert!(stopped["params"]["status"] == "cancelled" || stopped["params"]["error_kind"] == "cancelled", "{stopped}");
    let finished = c.wait_notification(|n| n["method"] == "task/finished" && n["params"]["session_id"] == second.as_str());
    assert_eq!(finished["params"]["status"], "completed", "the other session's run was not cancelled: {finished}");
    let status = c.result("project/status", json!({"project_id": pid}));
    assert_eq!(status["running_tasks"], json!([]), "{status}");
    let idle = c.request("task/cancel", json!({"project_id": pid, "session_id": second}));
    assert_eq!(idle["error"]["code"], -32003, "{idle}");
    assert_eq!(c.result("shutdown", json!({})), json!({}));
}

#[test]
fn task_run_attachments_are_validated_and_listed_in_history() {
    let data = tempfile::tempdir().unwrap();
    let repo = fixture();
    let mut c = Client::spawn(data.path());
    c.result("initialize", json!({}));
    let info = c.result("project/open", json!({"path": repo.path()}));
    let pid = info["project_id"].as_str().unwrap().to_string();
    c.wait_notification(|n| n["method"] == "index/ready" && n["params"]["project_id"] == pid.as_str());
    let sid = c.result("session/create", json!({"project_id": pid}))["id"].as_str().unwrap().to_string();

    let png = data.path().join("shot.png");
    std::fs::write(&png, b"\x89PNG\r\n\x1a\nnot really pixels").unwrap();
    let text = data.path().join("notes.txt");
    std::fs::write(&text, "hello").unwrap();
    for (images, needle) in [
        (json!([text]), "not a PNG"),
        (json!([data.path().join("missing.png")]), "cannot read image"),
        (json!(["relative.png"]), "must be absolute"),
    ] {
        let refused = c.request("task/run", json!({"project_id": pid, "session_id": sid, "prompt": "look", "images": images}));
        assert_eq!(refused["error"]["code"], -32602, "{refused}");
        assert!(refused["error"]["message"].as_str().unwrap().contains(needle), "{refused}");
    }
    let binary = c.request("task/run", json!({"project_id": pid, "session_id": sid, "prompt": "look", "files": [{"path": png}]}));
    assert_eq!(binary["error"]["code"], -32602, "{binary}");
    assert!(binary["error"]["message"].as_str().unwrap().contains("not a UTF-8 text file"), "{binary}");

    let accepted = c.result("task/run", json!({"project_id": pid, "session_id": sid, "prompt": "What is in this screenshot?", "images": [png], "files": [{"path": text, "name": "notes.pdf"}]}));
    assert_eq!(accepted["accepted"], true);
    let finished = c.wait_notification(|n| n["method"] == "task/finished" && n["params"]["project_id"] == pid.as_str());
    assert_eq!(finished["params"]["status"], "completed", "{finished}");

    let history = c.result("session/history", json!({"project_id": pid, "session_id": sid}));
    let messages = history["turns"][0]["messages"].as_array().unwrap();
    assert_eq!(messages[0]["kind"], "user");
    assert_eq!(messages[0]["images"], json!([png]), "{}", messages[0]);
    assert_eq!(messages[0]["files"], json!([{"path": text, "name": "notes.pdf"}]), "{}", messages[0]);
    assert_eq!(messages[0]["text"], "What is in this screenshot?", "file contents are not part of the shown text");
    assert!(messages.iter().skip(1).all(|m| m.get("images").is_none() && m.get("files").is_none()), "only the user's message carries attachments");

    assert_eq!(c.result("shutdown", json!({})), json!({}));
    c.child.wait().unwrap();
}

fn is_event(frame: &Value, project_id: &str, kind: &str) -> bool {
    frame["method"] == "event" && frame["params"]["project_id"] == project_id && frame["params"]["event"]["event"] == kind
}

fn is_notification(frame: &Value, method: &str, project_id: &str) -> bool {
    frame["method"] == method && frame["params"]["project_id"] == project_id
}

/// Many small files so indexing takes long enough for a run to have to
/// wait for it.
fn big_fixture(files: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for i in 0..files {
        let pkg = root.join(format!("pkg{}", i % 20));
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join(format!("m{i}.py")), format!("# module {i}\ndef f{i}(x):\n    return x + {i}\n\n\nclass C{i}:\n    def m(self):\n        return f{i}(1)\n")).unwrap();
    }
    std::fs::write(root.join("README.md"), format!("# Big\n{}", "line\n".repeat(40))).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "commit", "-q", "-m", "initial"]);
    dir
}

/// The project's configuration includes the repository's `.context-agent.toml`
/// (validation commands, agent.max_steps) exactly like `ctx run`, while the
/// user-level configuration the app edits does not; `options: null` is
/// accepted; `git/diff` output does not depend on the user's git config;
/// `file/read` is bounded.
#[test]
fn project_open_honours_repository_configuration() {
    let data = tempfile::tempdir().unwrap();
    let repo = fixture();
    std::fs::write(repo.path().join(".context-agent.toml"), "[validation]\ncommands = [\"echo repo-level-validation\"]\n\n[agent]\nmax_steps = 1\n\n[model]\nname = \"evil-model\"\n").unwrap();
    std::fs::write(repo.path().join("utf8.txt"), "é".repeat(4)).unwrap();
    std::fs::write(repo.path().join("big.txt"), "x".repeat(400_000)).unwrap();
    let gitconfig = data.path().join("gitconfig");
    std::fs::write(&gitconfig, "[diff]\n\tnoprefix = true\n\tmnemonicPrefix = true\n[color]\n\tui = always\n").unwrap();
    let mut c = Client::spawn_with(data.path(), |cmd| {
        cmd.env("GIT_CONFIG_GLOBAL", &gitconfig);
    });

    let init = c.result("initialize", json!({}));
    let model = init["model"]["name"].as_str().unwrap().to_string();
    assert_ne!(model, "evil-model");

    let info = c.result("project/open", json!({"path": repo.path()}));
    let pid = info["project_id"].as_str().unwrap().to_string();
    let commands: Vec<&str> = info["validation_commands"].as_array().unwrap().iter().filter_map(|v| v["command"].as_str()).collect();
    assert!(commands.contains(&"echo repo-level-validation"), "repository validation commands are honoured: {info}");
    assert_eq!(info["model"], json!(model), "a repository file cannot set the model: {info}");
    let user_level = c.result("config/get", json!({}));
    assert_eq!(user_level["model"]["name"], json!(model), "the user-level configuration never includes the repository layer");
    c.wait_notification(|n| is_notification(n, "index/ready", &pid));

    let session = c.result("session/create", json!({"project_id": pid}));
    let sid = session["id"].as_str().unwrap().to_string();
    let accepted = c.result("task/run", json!({"project_id": pid, "session_id": sid, "prompt": "Summarize", "options": null}));
    assert_eq!(accepted["accepted"], true, "options: null means defaults");
    let finished = c.wait_notification(|n| is_notification(n, "task/finished", &pid));
    assert_eq!(finished["params"]["status"], "blocked", "repository agent.max_steps = 1 stops the run after one step: {finished}");
    c.drain_for(Duration::from_millis(300));
    let events = c.take_events(&sid);
    let steps = events.iter().filter(|e| e["event"] == "model_request_started").count();
    assert_eq!(steps, 1, "{events:#?}");

    let doctor = c.result("doctor/run", json!({"project_id": pid}));
    let rows: Vec<&Value> = doctor["sections"].as_array().unwrap().iter().flat_map(|s| s["rows"].as_array().unwrap()).collect();
    assert!(rows.iter().any(|r| r["label"] == "note" && r["value"].as_str().unwrap().contains("[model]")), "doctor reports the ignored repository table: {doctor}");
    assert!(rows.iter().any(|r| r["value"].as_str().unwrap().contains("echo repo-level-validation")), "{doctor}");

    let diff = c.result("git/diff", json!({"project_id": pid}));
    let text = diff["diff"].as_str().unwrap();
    assert!(text.contains("--- a/src/lib.rs") && text.contains("+++ b/src/lib.rs"), "prefixes are pinned regardless of diff.noprefix/mnemonicPrefix: {text}");
    assert!(!text.contains('\u{1b}'), "no colour escapes despite color.ui=always");

    let small = c.result("file/read", json!({"project_id": pid, "path": "utf8.txt", "max_bytes": 3}));
    assert_eq!(small["content"], "é", "{small}");
    assert_eq!((small["truncated"].as_bool(), small["size"].as_u64()), (Some(true), Some(8)));
    let big = c.result("file/read", json!({"project_id": pid, "path": "big.txt"}));
    assert_eq!((big["content"].as_str().unwrap().len(), big["truncated"].as_bool(), big["size"].as_u64()), (262_144, Some(true), Some(400_000)));
    let capped = c.result("file/read", json!({"project_id": pid, "path": "big.txt", "max_bytes": 1u64 << 40}));
    assert_eq!((capped["content"].as_str().unwrap().len(), capped["truncated"].as_bool()), (400_000, Some(false)));

    assert_eq!(c.result("shutdown", json!({})), json!({}));
    assert_eq!(c.child.wait().unwrap().code(), Some(0));
}

/// `config/patch` refuses unknown keys below an allowed prefix, writes the
/// file privately, and `model.*` patches keep a project's switched model
/// unless `model.name` itself is patched.
/// `file/write` honours the path policy and `expected_hash`, updates the
/// index as it goes, and `symbols/find` answers from that index.
#[test]
fn file_write_and_symbol_lookup() {
    let data = tempfile::tempdir().unwrap();
    let repo = fixture();
    let mut c = Client::spawn(data.path());
    c.result("initialize", json!({}));
    let info = c.result("project/open", json!({"path": repo.path()}));
    let pid = info["project_id"].as_str().unwrap().to_string();
    c.wait_notification(|n| is_notification(n, "index/ready", &pid));

    let found = c.result("symbols/find", json!({"project_id": pid, "query": "ad"}));
    let add = found["symbols"].as_array().unwrap().iter().find(|s| s["name"] == "add").unwrap_or_else(|| panic!("prefix match lists `add`: {found}"));
    assert_eq!(add["path"], "src/lib.rs", "{add}");
    assert_eq!(add["line"], 1, "lines are 1-based: {add}");
    assert!(add["kind"].as_str().is_some_and(|k| !k.is_empty()), "{add}");
    assert_eq!(c.result("symbols/find", json!({"project_id": pid, "query": "  "}))["symbols"], json!([]));

    let read = c.result("file/read", json!({"project_id": pid, "path": "src/lib.rs"}));
    let hash = read["hash"].as_str().unwrap().to_string();
    assert!(hash.starts_with("sha256:"), "{read}");
    let content = format!("{}\npub fn multiply(a: i32, b: i32) -> i32 {{\n    a * b\n}}\n", read["content"].as_str().unwrap());
    let written = c.result("file/write", json!({"project_id": pid, "path": "src/lib.rs", "content": content, "expected_hash": hash}));
    assert_eq!(written["created"], false, "{written}");
    assert_eq!(written["language"], "rust", "{written}");
    assert_eq!(std::fs::read_to_string(repo.path().join("src/lib.rs")).unwrap(), content);
    let again = c.result("file/read", json!({"project_id": pid, "path": "src/lib.rs"}));
    assert_eq!(again["hash"], written["hash"], "the write reports the hash a read then sees: {again}");

    let stale = c.request("file/write", json!({"project_id": pid, "path": "src/lib.rs", "content": "clobbered", "expected_hash": hash}));
    assert_eq!(stale["error"]["code"], -32000, "{stale}");
    assert_eq!(stale["error"]["data"]["kind"], "conflict", "{stale}");
    assert_eq!(std::fs::read_to_string(repo.path().join("src/lib.rs")).unwrap(), content, "a stale hash writes nothing");

    let multiply = c.result("symbols/find", json!({"project_id": pid, "query": "multiply", "mode": "exact"}));
    assert_eq!(multiply["symbols"][0]["path"], "src/lib.rs", "the index was updated by the write: {multiply}");
    assert_eq!(multiply["symbols"][0]["line"], 9, "{multiply}");

    let created = c.result("file/write", json!({"project_id": pid, "path": "src/fresh/mod.rs", "content": "pub struct Fresh;\n"}));
    assert_eq!(created["created"], true, "{created}");
    assert_eq!(std::fs::read_to_string(repo.path().join("src/fresh/mod.rs")).unwrap(), "pub struct Fresh;\n");

    let escape = c.request("file/write", json!({"project_id": pid, "path": "../../outside.txt", "content": "x"}));
    assert_eq!(escape["error"]["data"]["kind"], "permission_denied", "{escape}");
    let env = c.result("file/write", json!({"project_id": pid, "path": ".env", "content": "TOKEN=1\n"}));
    assert_eq!(env["created"], true, "the editor may write credential files: {env}");
    let env = c.result("file/read", json!({"project_id": pid, "path": ".env"}));
    assert_eq!(env["content"], "TOKEN=1\n", "and read them back: {env}");
    let directory = c.request("file/write", json!({"project_id": pid, "path": "src", "content": "x"}));
    assert_eq!(directory["error"]["data"]["kind"], "invalid_argument", "{directory}");

    assert_eq!(c.result("shutdown", json!({})), json!({}));
    assert_eq!(c.child.wait().unwrap().code(), Some(0));
}

/// The `lsp/*` methods against a fake language server (Python, chosen with
/// `CTX_LSP_COMMAND_PYTHON`): buffer sync, completion shaping, hover,
/// definition, forwarded diagnostics, and the empty answers for a language
/// without a server.
#[test]
fn lsp_editor_methods_talk_to_the_language_server() {
    let python = ctx_core::platform::python_interpreter();
    if ctx_core::platform::find_executable(python).is_none() {
        eprintln!("{python} not available; skipping");
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let repo = fixture();
    let script = data.path().join("fake_lsp.py");
    std::fs::write(
        &script,
        r#"
import sys, json
def read():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.decode().strip()
        if not line:
            break
        if line.lower().startswith("content-length:"):
            length = int(line.split(":")[1])
    return json.loads(sys.stdin.buffer.read(length))
def send(obj):
    body = json.dumps(obj).encode()
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    sys.stdout.buffer.flush()
def diagnostics(uri, text):
    send({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"uri": uri, "diagnostics": [
        {"range": {"start": {"line": 0, "character": 4}, "end": {"line": 0, "character": 9}}, "severity": 2, "message": "len=%d" % len(text), "source": "fake"}]}})
while True:
    msg = read()
    if msg is None:
        break
    m = msg.get("method")
    p = msg.get("params", {})
    if m == "initialize":
        send({"jsonrpc": "2.0", "id": msg["id"], "result": {"capabilities": {"completionProvider": {"triggerCharacters": ["."]}, "hoverProvider": True}}})
    elif m == "textDocument/didOpen":
        diagnostics(p["textDocument"]["uri"], p["textDocument"]["text"])
    elif m == "textDocument/didChange":
        diagnostics(p["textDocument"]["uri"], p["contentChanges"][0]["text"])
    elif m == "textDocument/completion":
        send({"jsonrpc": "2.0", "id": msg["id"], "result": {"isIncomplete": False, "items": [
            {"label": "max", "kind": 14, "sortText": "b"},
            {"label": "clamp", "kind": 3, "detail": "def clamp(x, lo, hi)", "sortText": "a", "insertTextFormat": 2,
             "textEdit": {"range": {"start": {"line": p["position"]["line"], "character": 0}, "end": p["position"]}, "newText": "clamp(${1:x}, ${2:lo}, ${3:hi})$0"},
             "documentation": {"kind": "markdown", "value": "Clamps x."}}]}})
    elif m == "textDocument/hover":
        send({"jsonrpc": "2.0", "id": msg["id"], "result": {"contents": {"kind": "markdown", "value": "```python\ndef clamp(x, lo, hi)\n```"}}})
    elif m == "textDocument/definition":
        send({"jsonrpc": "2.0", "id": msg["id"], "result": [{"uri": p["textDocument"]["uri"], "range": {"start": {"line": 0, "character": 4}, "end": {"line": 0, "character": 9}}}]})
    elif m == "shutdown":
        send({"jsonrpc": "2.0", "id": msg["id"], "result": None})
    elif m == "exit":
        break
"#,
    )
    .unwrap();
    let command = format!("{python} -I {}", script.display());
    let mut c = Client::spawn_with(data.path(), |cmd| {
        cmd.env("CTX_LSP_COMMAND_PYTHON", &command);
    });
    c.result("initialize", json!({}));
    let info = c.result("project/open", json!({"path": repo.path()}));
    let pid = info["project_id"].as_str().unwrap().to_string();

    let status = c.result("lsp/status", json!({"project_id": pid, "path": "src/util.py"}));
    assert_eq!((status["language"].as_str(), status["installed"].as_bool(), status["running"].as_bool()), (Some("python"), Some(true), Some(false)), "{status}");
    let notes = c.result("lsp/status", json!({"project_id": pid, "path": "notes.txt"}));
    assert_eq!(notes["installed"], false, "no server is known for plain text: {notes}");
    assert_eq!(c.result("lsp/open", json!({"project_id": pid, "path": "notes.txt", "text": "hi"}))["server"], Value::Null);
    assert_eq!(c.result("lsp/completion", json!({"project_id": pid, "path": "notes.txt", "line": 0, "character": 0}))["items"], json!([]));

    let text = "def clamp(x, lo, hi):\n    return max(lo, min(hi, x))\nc".to_string();
    let opened = c.result("lsp/open", json!({"project_id": pid, "path": "src/util.py", "text": text}));
    assert_eq!((opened["server"].as_str(), opened["version"].as_i64()), (Some(python), Some(1)), "{opened}");
    assert_eq!(opened["trigger_characters"], json!(["."]));
    let diagnostics = c.wait_notification(|n| n["method"] == "lsp/diagnostics" && n["params"]["path"] == "src/util.py");
    let first = &diagnostics["params"]["diagnostics"][0];
    assert_eq!((first["severity"].as_str(), first["message"].as_str(), first["line"].as_u64(), first["end_character"].as_u64()), (Some("warning"), Some("len=54"), Some(0), Some(9)), "{diagnostics}");
    assert_eq!(c.result("lsp/status", json!({"project_id": pid, "path": "src/util.py"}))["running"], true);

    let changed = c.result("lsp/change", json!({"project_id": pid, "path": "src/util.py", "text": format!("{text}la")}));
    assert_eq!(changed["version"], 2, "{changed}");
    let diagnostics = c.wait_notification(|n| n["method"] == "lsp/diagnostics" && n["params"]["diagnostics"][0]["message"] == "len=56");
    assert_eq!(diagnostics["params"]["project_id"], pid.as_str());

    let completion = c.result("lsp/completion", json!({"project_id": pid, "path": "src/util.py", "line": 2, "character": 3}));
    let items = completion["items"].as_array().unwrap();
    assert_eq!(items.iter().map(|i| i["label"].as_str().unwrap()).collect::<Vec<_>>(), vec!["clamp", "max"], "sorted by sort_text: {completion}");
    assert_eq!((items[0]["kind"].as_str(), items[0]["insert_text"].as_str(), items[0]["documentation"].as_str()), (Some("function"), Some("clamp(x, lo, hi)"), Some("Clamps x.")), "{completion}");
    assert_eq!(items[0]["range"], json!({"start_line": 2, "start_character": 0, "end_line": 2, "end_character": 3}));
    assert_eq!((items[1]["kind"].as_str(), items[1]["range"].is_null()), (Some("keyword"), true));
    assert_eq!(completion["server"], python);

    let hover = c.result("lsp/hover", json!({"project_id": pid, "path": "src/util.py", "line": 1, "character": 12}));
    assert_eq!(hover["contents"], "```python\ndef clamp(x, lo, hi)\n```");
    let definition = c.result("lsp/definition", json!({"project_id": pid, "path": "src/util.py", "line": 1, "character": 12}));
    assert_eq!(definition["locations"], json!([{"path": "src/util.py", "line": 1, "column": 5}]));

    assert_eq!(c.result("lsp/close", json!({"project_id": pid, "path": "src/util.py"})), json!({}));
    assert_eq!(c.result("project/close", json!({"project_id": pid})), json!({}));
    assert_eq!(c.result("shutdown", json!({})), json!({}));
    assert_eq!(c.child.wait().unwrap().code(), Some(0));
}

#[test]
fn config_patch_rejects_unknown_keys_and_keeps_switched_models() {
    let data = tempfile::tempdir().unwrap();
    let repo = fixture();
    let mut c = Client::spawn(data.path());
    let init = c.result("initialize", json!({}));
    let model = init["model"]["name"].as_str().unwrap().to_string();
    let config_file = Path::new(init["config_file"].as_str().unwrap()).to_path_buf();

    let typo = c.request("config/patch", json!({"values": {"model.nmae": "typo"}}));
    assert_eq!(typo["error"]["code"], -32602, "{typo}");
    assert!(typo["error"]["message"].as_str().unwrap().contains("model.nmae"), "{typo}");
    assert!(!config_file.exists(), "a rejected patch writes nothing");
    let nested = c.request("config/patch", json!({"values": {"index.foo.bar": 1}}));
    assert_eq!(nested["error"]["code"], -32602, "{nested}");
    let not_table = c.request("config/patch", json!({"values": {"model.name.x": 1}}));
    assert_eq!(not_table["error"]["code"], -32602, "{not_table}");

    let header = c.result("config/patch", json!({"values": {"model.extra_headers.X-Test": "1", "pricing.input_per_million": 2.5}}));
    assert_eq!(header["pricing"]["input_per_million"], 2.5, "{header}");
    let written = std::fs::read_to_string(&config_file).unwrap();
    assert!(written.contains("X-Test"), "{written}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&config_file).unwrap().permissions().mode() & 0o777, 0o600, "the configuration file is private");
    }
    let leftovers: Vec<String> = std::fs::read_dir(config_file.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).filter(|n| n.contains(".tmp")).collect();
    assert!(leftovers.is_empty(), "atomic writes leave no temp files: {leftovers:?}");
    c.result("config/patch", json!({"values": {"model.extra_headers.X-Test": null}}));

    let info = c.result("project/open", json!({"path": repo.path()}));
    let pid = info["project_id"].as_str().unwrap().to_string();
    c.wait_notification(|n| is_notification(n, "index/ready", &pid));
    let switched = c.result("models/switch", json!({"project_id": pid, "model": "fancy-model"}));
    assert_eq!(switched["model"], "fancy-model");
    let patched = c.result("config/patch", json!({"values": {"model.reasoning_effort": "high"}}));
    assert_eq!(patched["model"]["reasoning_effort"], "high", "{patched}");
    let xhigh = c.result("config/patch", json!({"values": {"model.reasoning_effort": "xhigh"}}));
    assert_eq!(xhigh["model"]["reasoning_effort"], "xhigh", "a model's own effort value: {xhigh}");
    let ultra = c.result("config/patch", json!({"values": {"model.reasoning_effort": "ultra"}}));
    assert_eq!(ultra["model"]["reasoning_effort"], "ultra", "the Ultra tier is a configuration value: {ultra}");
    let bogus = c.request("config/patch", json!({"values": {"model.reasoning_effort": "very hard"}}));
    assert_eq!(bogus["error"]["code"], -32602, "{bogus}");
    for gone in ["read_only", "sandbox_autonomous"] {
        let refused = c.request("permissions/set", json!({"project_id": pid, "profile": gone}));
        assert_eq!(refused["error"]["code"], -32602, "{gone} is no longer a profile: {refused}");
        let refused = c.request("config/patch", json!({"values": {"permissions.profile": gone}}));
        assert_eq!(refused["error"]["code"], -32602, "{gone} is no longer a profile: {refused}");
    }
    let bypass = c.result("permissions/set", json!({"project_id": pid, "profile": "bypass"}));
    assert_eq!(bypass["permission_profile"], "bypass_permissions", "{bypass}");
    c.result("permissions/set", json!({"project_id": pid, "profile": "approval_required"}));
    let status = c.result("project/status", json!({"project_id": pid}));
    assert_eq!(status["model"], "fancy-model", "an unrelated model.* patch keeps the switched model: {status}");
    c.result("config/patch", json!({"values": {"model.name": "patched-model"}}));
    let status = c.result("project/status", json!({"project_id": pid}));
    assert_eq!(status["model"], "patched-model", "{status}");
    let reset = c.result("config/patch", json!({"values": {"model.name": null, "model.reasoning_effort": null, "pricing.input_per_million": null}}));
    assert_eq!(reset["model"]["name"], json!(model));
    assert!(reset["pricing"].is_null(), "{reset}");

    assert_eq!(c.result("shutdown", json!({})), json!({}));
    assert_eq!(c.child.wait().unwrap().code(), Some(0));
}

/// While `task/run` waits for the background index, `project/status` keeps
/// reporting `indexing`; `index/ready` always precedes the run's
/// `task_started`; a cancel during the wait ends in `task/finished`; closing
/// a project mid-rebuild and reopening it yields exactly one `index/ready`;
/// `task/finished` is ordered before the next accepted `task/run`.
#[test]
fn indexing_runs_and_close_reopen_are_ordered() {
    let data = tempfile::tempdir().unwrap();
    let repo = big_fixture(400);
    let mut c = Client::spawn(data.path());
    c.result("initialize", json!({}));
    let info = c.result("project/open", json!({"path": repo.path()}));
    let pid = info["project_id"].as_str().unwrap().to_string();
    let session = c.result("session/create", json!({"project_id": pid}));
    let sid = session["id"].as_str().unwrap().to_string();

    // Cancel while the run is still waiting for the index: the run must end
    // now (task/finished before index/ready, no task created), not when the
    // index is ready — the slot would stay taken for the whole first index.
    c.result("task/run", json!({"project_id": pid, "session_id": sid, "prompt": "Cancel me"}));
    assert_eq!(c.result("task/cancel", json!({"project_id": pid})), json!({}));
    let cancelled_at = c.trace.len();
    let finished = c.wait_notification(|n| is_notification(n, "task/finished", &pid));
    let fp = &finished["params"];
    assert!(fp["error_kind"] == "cancelled" || fp["status"] == "cancelled", "{finished}");
    let finished_at = c.trace_position(cancelled_at, |f| is_notification(f, "task/finished", &pid)).unwrap();
    match c.trace_position(0, |f| is_notification(f, "index/ready", &pid)) {
        // The index was still running when the cancel arrived (the normal
        // case with 400 files): the run ended during the wait.
        Some(ready_at) if ready_at > cancelled_at => {
            assert!(finished_at < ready_at, "a cancel during the index wait must not wait for index/ready@{ready_at}: task/finished@{finished_at}");
            assert!(fp["task_id"].is_null() && fp["status"].is_null() && fp["error_kind"] == "cancelled", "a run cancelled before it reached the runtime creates no task: {finished}");
        }
        None => {
            assert!(fp["task_id"].is_null() && fp["error_kind"] == "cancelled", "{finished}");
        }
        // A very fast machine finished indexing before the cancel; the
        // runtime cancelled the task instead, which is also correct.
        Some(_) => {}
    }

    c.result("task/run", json!({"project_id": pid, "session_id": sid, "prompt": "Summarize"}));
    let status = c.result("project/status", json!({"project_id": pid}));
    assert!(status["indexing"] == true || status["index"]["generation"].as_u64().unwrap() >= 1, "indexing is reported while the run waits for it: {status}");
    assert_eq!(status["running_task"]["session_id"], json!(sid));
    let finished = c.wait_notification(|n| is_notification(n, "task/finished", &pid));
    assert_eq!(finished["params"]["status"], "completed", "{finished}");
    let ready_at = c.trace_position(0, |f| is_notification(f, "index/ready", &pid)).expect("index/ready");
    let started_at = c.trace_position(0, |f| is_event(f, &pid, "task_started")).expect("task_started");
    assert!(ready_at < started_at, "index/ready@{ready_at} must precede task_started@{started_at}");

    // Rebuild, then run: the run waits for the rebuild.
    let mark = c.trace.len();
    assert_eq!(c.result("index/rebuild", json!({"project_id": pid})), json!({}));
    c.result("task/run", json!({"project_id": pid, "session_id": sid, "prompt": "Summarize again"}));
    let status = c.result("project/status", json!({"project_id": pid}));
    assert!(status["indexing"] == true || c.trace_position(mark, |f| is_notification(f, "index/ready", &pid)).is_some(), "{status}");
    let finished = c.wait_notification(|n| is_notification(n, "task/finished", &pid));
    assert_eq!(finished["params"]["status"], "completed", "{finished}");
    let ready_at = c.trace_position(mark, |f| is_notification(f, "index/ready", &pid)).expect("index/ready");
    assert_eq!(c.trace[ready_at]["params"]["rebuilt"], true);
    let started_at = c.trace_position(mark, |f| is_event(f, &pid, "task_started")).expect("task_started");
    assert!(ready_at < started_at, "index/ready@{ready_at} must precede task_started@{started_at}");

    // task/finished is queued before the slot is released: the first
    // accepted follow-up run always comes after it on the wire.
    c.drain_for(Duration::from_millis(200));
    let mark = c.trace.len();
    c.result("task/run", json!({"project_id": pid, "session_id": sid, "prompt": "Ordering probe"}));
    let mut accepted_at = None;
    for _ in 0..2000 {
        let r = c.request("task/run", json!({"project_id": pid, "session_id": sid, "prompt": "Ordering probe 2"}));
        if r.get("result").is_some() {
            accepted_at = Some(c.trace.len() - 1);
            break;
        }
        assert_eq!(r["error"]["code"], -32002, "{r}");
    }
    let accepted_at = accepted_at.expect("the second run is accepted once the first ends");
    let first_finished = c.trace_position(mark, |f| is_notification(f, "task/finished", &pid)).expect("task/finished of the first run");
    assert!(first_finished < accepted_at, "task/finished@{first_finished} must precede the next accepted task/run@{accepted_at}");
    // The first run's task/finished is already buffered; wait until the
    // second run's has arrived too (both are in the trace once seen), so the
    // rebuild below never races a run that is still active.
    while c.trace.iter().skip(mark).filter(|f| is_notification(f, "task/finished", &pid)).count() < 2 {
        c.wait_notification(|n| is_notification(n, "task/finished", &pid) && n["params"]["session_id"] == sid.as_str());
    }
    c.drain_for(Duration::from_millis(200));

    // Close during a rebuild, reopen immediately: the orphaned pass stays
    // silent and the reopen waits for it, so exactly one index/ready follows.
    assert_eq!(c.result("index/rebuild", json!({"project_id": pid})), json!({}));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(c.result("project/close", json!({"project_id": pid})), json!({}));
    let closed_at = c.trace.len();
    let reopened = c.result("project/open", json!({"path": repo.path()}));
    assert_eq!(reopened["project_id"], json!(pid));
    c.wait_notification(|n| is_notification(n, "index/ready", &pid));
    c.drain_for(Duration::from_secs(2));
    let readies = c.trace.iter().skip(closed_at).filter(|f| is_notification(f, "index/ready", &pid)).count();
    assert_eq!(readies, 1, "one index/ready after close+reopen, none from the orphaned rebuild: {:#?}", c.trace.iter().skip(closed_at).filter(|f| f["method"] == "index/ready").collect::<Vec<_>>());
    let status = c.result("project/status", json!({"project_id": pid}));
    assert_eq!(status["indexing"], false, "{status}");
    assert!(status["index"]["files"].as_u64().unwrap() >= 400, "{status}");

    // Shutdown while a run is active still reports task/finished.
    c.result("task/run", json!({"project_id": pid, "session_id": sid, "prompt": "Shutdown mid-run"}));
    assert_eq!(c.result("shutdown", json!({})), json!({}));
    assert_eq!(c.child.wait().unwrap().code(), Some(0));
    let mut saw_finished = false;
    while let Ok(frame) = c.rx.try_recv() {
        let frame = frame.expect("well-formed frame after shutdown");
        saw_finished |= is_notification(&frame, "task/finished", &pid) && frame["params"]["session_id"] == sid.as_str();
    }
    assert!(saw_finished, "the interrupted run reports task/finished before the process exits");
}

/// Requests that race on one project are serialized where the spec needs
/// them to be: concurrent `project/open` of one root all succeed and index
/// once; two `index/rebuild` dispatched together run two full passes one
/// after the other (never `clear()` under a running pass); a project closed
/// while running and reopened does not start a second runtime on the same
/// session before the cancelled one has reported `task/finished`.
#[test]
fn concurrent_opens_rebuilds_and_close_reopen_are_serialized() {
    let data = tempfile::tempdir().unwrap();
    let repo = big_fixture(400);
    let mut c = Client::spawn(data.path());
    c.result("initialize", json!({}));

    // Three opens of the same root written back to back.
    let ids: Vec<i64> = (0..3).map(|_| c.send("project/open", json!({"path": repo.path()}))).collect();
    let mut pids = Vec::new();
    for id in ids {
        let resp = c.wait_response(id);
        assert!(resp.get("error").is_none(), "a concurrent open of the same root must not fail: {resp}");
        pids.push(resp["result"]["project_id"].as_str().unwrap().to_string());
    }
    assert!(pids.iter().all(|p| p == &pids[0]), "{pids:?}");
    let pid = pids[0].clone();
    c.wait_notification(|n| is_notification(n, "index/ready", &pid));
    c.drain_for(Duration::from_millis(500));
    let readies = c.trace.iter().filter(|f| is_notification(f, "index/ready", &pid)).count();
    assert_eq!(readies, 1, "three concurrent opens index once");
    let files = c.result("project/status", json!({"project_id": pid}))["index"]["files"].as_u64().unwrap();
    assert!(files >= 400, "{files}");

    // Two rebuilds back to back: both accepted, two sequential full passes.
    let mark = c.trace.len();
    let r1 = c.send("index/rebuild", json!({"project_id": pid}));
    let r2 = c.send("index/rebuild", json!({"project_id": pid}));
    assert_eq!(c.wait_response(r1)["result"], json!({}));
    assert_eq!(c.wait_response(r2)["result"], json!({}));
    let first = c.wait_notification(|n| is_notification(n, "index/ready", &pid));
    let second = c.wait_notification(|n| is_notification(n, "index/ready", &pid));
    for ready in [&first, &second] {
        assert_eq!(ready["params"]["rebuilt"], true, "{ready}");
        assert!(ready["params"]["error"].is_null(), "{ready}");
        assert_eq!(ready["params"]["files_indexed"].as_u64(), Some(files), "each rebuild is a full pass over the cleared store, never a no-op under the other: {ready}");
    }
    // A rebuild clears the store, so each full pass reports generation 1
    // again (a `clear()` under a running pass showed up as a 0-file pass).
    for ready in [&first, &second] {
        assert!(ready["params"]["generation"].as_u64().unwrap() >= 1 && ready["params"]["files_unchanged"] == 0, "{ready}");
    }
    c.drain_for(Duration::from_millis(300));
    assert_eq!(c.trace.iter().skip(mark).filter(|f| is_notification(f, "index/ready", &pid)).count(), 2);
    let status = c.result("project/status", json!({"project_id": pid}));
    assert_eq!(status["indexing"], false, "{status}");
    assert_eq!(status["index"]["files"].as_u64(), Some(files), "{status}");

    // Close while a run is in flight, reopen, run again on the same session.
    let session = c.result("session/create", json!({"project_id": pid}));
    let sid = session["id"].as_str().unwrap().to_string();
    let mark = c.trace.len();
    c.result("task/run", json!({"project_id": pid, "session_id": sid, "prompt": "Close me mid-run"}));
    assert_eq!(c.result("project/close", json!({"project_id": pid})), json!({}));
    let reopened = c.result("project/open", json!({"path": repo.path()}));
    let reopened_at = c.trace.len() - 1;
    assert_eq!(reopened["project_id"], json!(pid));
    assert!(reopened["running_task"].is_null(), "{reopened}");
    // Consume the first run's task/finished (already buffered when the
    // reopen waited for it) before the second run produces its own.
    let first = c.wait_notification(|n| is_notification(n, "task/finished", &pid));
    let first_finished = c.trace_position(mark, |f| is_notification(f, "task/finished", &pid)).expect("the closed project's run reports task/finished");
    assert!(first_finished < reopened_at, "the reopen waits for the cancelled run: task/finished@{first_finished} must precede the project/open response@{reopened_at}");
    // A run the close cancels before it started (still waiting for the
    // background index) reports `status: null` with `error_kind: "cancelled"`
    // (docs/desktop-protocol.md, `task/cancel`).
    let cancelled_before_start = first["params"]["status"].is_null() && first["params"]["error_kind"] == "cancelled";
    assert!(first["params"]["status"] == "cancelled" || first["params"]["status"] == "completed" || cancelled_before_start, "{first}");
    c.result("task/run", json!({"project_id": pid, "session_id": sid, "prompt": "Second run on the reopened project"}));
    let finished = c.wait_notification(|n| is_notification(n, "task/finished", &pid));
    assert_eq!(finished["params"]["status"], "completed", "{finished}");
    c.drain_for(Duration::from_millis(300));
    let finished_frames = c.trace.iter().skip(mark).filter(|f| is_notification(f, "task/finished", &pid)).count();
    assert_eq!(finished_frames, 2, "exactly one task/finished per task/run");
    // A task_started of the second run can only come after the first run's
    // task/finished: the two runtimes never overlapped on the session.
    let second_started = c.trace_position(reopened_at, |f| is_event(f, &pid, "task_started")).expect("the second run started");
    assert!(second_started > first_finished);
    let history = c.result("session/history", json!({"project_id": pid, "session_id": sid}));
    assert!(!history["turns"].as_array().unwrap().is_empty(), "{history}");

    assert_eq!(c.result("shutdown", json!({})), json!({}));
    assert_eq!(c.child.wait().unwrap().code(), Some(0));
}

/// Error responses never leak secrets (paths, URLs and provider bodies are
/// redacted like events and logs), and the edge cases of the framing and
/// parameter validation behave as specified: `"id": null` is a request,
/// relative project paths are refused, object values of `config/patch` are
/// schema-checked, `git/diff` on a plain directory is empty, and the mock
/// `models/list` puts the current model first.
#[test]
fn error_frames_are_redacted_and_requests_are_validated() {
    let data = tempfile::tempdir().unwrap();
    let repo = fixture();
    let plain = tempfile::tempdir().unwrap();
    std::fs::write(plain.path().join("a.py"), "x = 1\n").unwrap();
    let mut c = Client::spawn(data.path());
    let secret = "sk-abcdefghijklmnopqrstuvwxyz0123456789";

    // An explicit null id is a request (JSON-RPC 2.0 discourages but allows it).
    c.send_raw(r#"{"jsonrpc":"2.0","id":null,"method":"initialize"}"#);
    let null_id = c.wait_notification(|n| n.get("method").is_none() && n.get("result").is_some());
    assert!(null_id["id"].is_null() && null_id["result"]["protocol_version"] == 1, "{null_id}");
    // A frame without an id stays a notification: no response at all.
    c.send_raw(r#"{"jsonrpc":"2.0","method":"initialize"}"#);
    c.drain_for(Duration::from_millis(300));
    assert!(c.notifications.iter().all(|n| n.get("method").is_some()), "{:#?}", c.notifications);
    // A line that is not valid UTF-8 is a parse error for that frame; the
    // server keeps running.
    c.stdin.write_all(b"\xff\xfe{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"initialize\"}\n").unwrap();
    c.stdin.flush().unwrap();
    let invalid_utf8 = c.wait_notification(|n| n.get("error").is_some());
    assert_eq!(invalid_utf8["error"]["code"], -32700, "{invalid_utf8}");
    assert!(invalid_utf8["id"].is_null(), "{invalid_utf8}");
    let init = c.result("initialize", json!({}));
    let model = init["model"]["name"].as_str().unwrap().to_string();

    let relative = c.request("project/open", json!({"path": "."}));
    assert_eq!(relative["error"]["code"], -32602, "relative paths are refused: {relative}");
    let missing = c.request("project/open", json!({"path": data.path().join("no").join("such").join(secret)}));
    assert_eq!(missing["error"]["code"], -32000, "{missing}");
    assert_eq!(missing["error"]["data"]["kind"], "config", "{missing}");
    let message = missing["error"]["message"].as_str().unwrap();
    assert!(!message.contains(secret) && message.contains("[REDACTED]"), "error messages are redacted: {message}");

    let info = c.result("project/open", json!({"path": repo.path()}));
    let pid = info["project_id"].as_str().unwrap().to_string();
    c.wait_notification(|n| is_notification(n, "index/ready", &pid));
    let read = c.request("file/read", json!({"project_id": pid, "path": format!("{secret}/x.txt")}));
    assert_eq!(read["error"]["code"], -32000, "{read}");
    let message = read["error"]["message"].as_str().unwrap();
    assert!(!message.contains(secret) && message.contains("[REDACTED]"), "{message}");
    let rename = c.request("session/rename", json!({"project_id": pid, "session_id": secret, "title": "x"}));
    assert_eq!(rename["error"]["code"], -32602, "{rename}");
    assert!(!rename["error"]["message"].as_str().unwrap().contains(secret), "{rename}");

    // Object values are checked leaf by leaf, like dotted keys.
    let config_file = Path::new(init["config_file"].as_str().unwrap()).to_path_buf();
    let typo = c.request("config/patch", json!({"values": {"model.capabilities": {"streming": false}}}));
    assert_eq!(typo["error"]["code"], -32602, "{typo}");
    assert!(typo["error"]["message"].as_str().unwrap().contains("model.capabilities.streming"), "{typo}");
    assert!(!config_file.exists(), "a rejected patch writes nothing");
    c.result("config/patch", json!({"values": {"model.capabilities": {"streaming": false}}}));
    assert!(std::fs::read_to_string(&config_file).unwrap().contains("streaming = false"));
    c.result("config/patch", json!({"values": {"model.capabilities": null}}));

    let models = c.result("models/list", json!({"project_id": pid}));
    assert_eq!(models["models"][0], json!(model), "the current model comes first: {models}");
    assert_eq!(models["current"], json!(model));
    let switched = c.result("models/switch", json!({"project_id": pid, "model": "zeta"}));
    assert_eq!(switched["model"], "zeta");
    let models = c.result("models/list", json!({"project_id": pid}));
    assert_eq!(models["models"], json!(["zeta", "mock-model"]), "{models}");

    // A plain directory: no git, so no diff rather than git's usage text.
    let plain_info = c.result("project/open", json!({"path": plain.path()}));
    let plain_id = plain_info["project_id"].as_str().unwrap().to_string();
    assert!(plain_info["git"].is_null(), "{plain_info}");
    assert_eq!(c.result("git/diff", json!({"project_id": plain_id})), json!({"diff": "", "truncated": false}));
    assert_eq!(c.result("git/status", json!({"project_id": plain_id})), json!({"branch": null, "head": null, "head_subject": null, "upstream": null, "ahead": 0, "behind": 0, "remotes": [], "files": []}));
    c.wait_notification(|n| is_notification(n, "index/ready", &plain_id));

    assert_eq!(c.result("shutdown", json!({})), json!({}));
    assert_eq!(c.child.wait().unwrap().code(), Some(0));
    while let Ok(frame) = c.rx.try_recv() {
        assert!(frame.is_ok(), "non-JSON output on stdout: {frame:?}");
    }
}

/// `session/rewind` removes a turn (or everything from a message on) and,
/// on request, puts the working tree back to the snapshot taken before it;
/// `project/open` initializes git on request so those snapshots exist.
#[test]
fn rewind_removes_turns_and_restores_files() {
    let data = tempfile::tempdir().unwrap();
    let repo = fixture();
    let mut c = Client::spawn(data.path());
    c.result("initialize", json!({}));
    let info = c.result("project/open", json!({"path": repo.path(), "init_git": true}));
    let pid = info["project_id"].as_str().unwrap().to_string();
    assert_eq!(info["git_initialized"], false, "an existing repository is left alone: {info}");
    c.wait_notification(|n| n["method"] == "index/ready" && n["params"]["project_id"] == pid.as_str());
    let sid = c.result("session/create", json!({"project_id": pid}))["id"].as_str().unwrap().to_string();

    let run = |c: &mut Client, prompt: &str| -> i64 {
        c.result("task/run", json!({"project_id": pid, "session_id": sid, "prompt": prompt}));
        let finished = c.wait_notification(|n| n["method"] == "task/finished" && n["params"]["session_id"] == sid.as_str());
        assert_eq!(finished["params"]["status"], "completed", "{finished}");
        assert_eq!(finished["params"]["snapshot"], true, "a snapshot is taken before every run: {finished}");
        finished["params"]["message_id"].as_i64().expect("the prompt's conversation id")
    };
    let first = run(&mut c, "First question");
    let second = run(&mut c, "Second question");
    assert!(second > first);
    c.take_events(&sid);

    // Changes made outside the session's file tools during the second turn
    // (the mock never edits): another session, the user. A rewind puts back
    // only what the session itself wrote, so these survive it.
    let lib = repo.path().join("src/lib.rs");
    let lib_before = std::fs::read_to_string(&lib).unwrap();
    std::fs::write(&lib, "// edited during the second turn\n").unwrap();
    std::fs::remove_file(repo.path().join("README.md")).unwrap();
    std::fs::write(repo.path().join("new.txt"), "created during the second turn\n").unwrap();

    // The mock's second prompt continues the first task (one turn, two user
    // messages) or starts a second one: a rewind handles both.
    let user_messages = |history: &Value| -> Vec<Value> {
        history["turns"].as_array().unwrap().iter().flat_map(|t| t["messages"].as_array().unwrap().iter().filter(|m| m["kind"] == "user").cloned()).collect()
    };
    let history = c.result("session/history", json!({"project_id": pid, "session_id": sid}));
    let users = user_messages(&history);
    assert_eq!(users.iter().map(|m| m["id"].as_i64().unwrap()).collect::<Vec<_>>(), vec![first, second], "{history}");
    assert!(users.iter().all(|m| m["snapshot"] == true), "{users:?}");
    let turns_before = history["turns"].as_array().unwrap().len();

    let bad = c.request("session/rewind", json!({"project_id": pid, "session_id": sid, "message_id": 999_999, "restore_files": true}));
    assert_eq!(bad["error"]["code"], -32602, "{bad}");

    let rewound = c.result("session/rewind", json!({"project_id": pid, "session_id": sid, "message_id": second, "restore_files": true}));
    assert_eq!(rewound["removed_tasks"], json!(turns_before - 1), "{rewound}");
    assert!(rewound["removed_messages"].as_u64().unwrap() >= 3, "the message, the assistant turns and tool results: {rewound}");
    assert_eq!(rewound["files"], json!({"restored": 0, "deleted": 0, "skipped": []}), "{rewound}");
    assert_eq!(rewound["session"]["task_count"], 1);
    assert_eq!(rewound["session"]["title"], "First question");
    assert_eq!(std::fs::read_to_string(&lib).unwrap(), "// edited during the second turn\n");
    assert!(!repo.path().join("README.md").exists());
    assert!(repo.path().join("new.txt").exists());
    std::fs::write(&lib, &lib_before).unwrap();
    let history = c.result("session/history", json!({"project_id": pid, "session_id": sid}));
    assert_eq!(history["turns"].as_array().unwrap().len(), 1, "{history}");
    let users = user_messages(&history);
    assert_eq!(users.len(), 1, "{history}");
    assert_eq!(users[0]["id"], json!(first));
    assert_eq!(users[0]["text"], "First question");
    let turn = &history["turns"][0];
    assert_eq!(turn["task"]["status"], "completed", "a task cut in the middle keeps its earlier completion: {turn}");
    assert_eq!(turn["outcome"]["status"], "completed", "{turn}");
    assert!(turn["events"].as_array().unwrap().iter().filter(|e| e["event"] == "task_completed").count() == 1, "{turn}");

    // Keeping the files: the chat is emptied, the working tree stays.
    std::fs::write(repo.path().join("kept.txt"), "kept\n").unwrap();
    let rewound = c.result("session/rewind", json!({"project_id": pid, "session_id": sid, "message_id": first, "restore_files": false}));
    assert_eq!(rewound["removed_tasks"], 1, "{rewound}");
    assert!(rewound["files"].is_null(), "{rewound}");
    assert_eq!(rewound["session"]["title"], "New chat");
    assert!(repo.path().join("kept.txt").exists());
    let history = c.result("session/history", json!({"project_id": pid, "session_id": sid}));
    assert!(history["turns"].as_array().unwrap().is_empty(), "{history}");
    let gone = c.request("session/rewind", json!({"project_id": pid, "session_id": sid, "message_id": first}));
    assert_eq!(gone["error"]["code"], -32602, "a removed message is unknown: {gone}");

    // The session is still usable, and a rewind is refused while it runs.
    c.result("task/run", json!({"project_id": pid, "session_id": sid, "prompt": "Third question"}));
    let busy = c.request("session/rewind", json!({"project_id": pid, "session_id": sid, "message_id": 1}));
    assert_eq!(busy["error"]["code"], -32002, "{busy}");
    let finished = c.wait_notification(|n| n["method"] == "task/finished" && n["params"]["session_id"] == sid.as_str());
    assert_eq!(finished["params"]["status"], "completed", "{finished}");
    let sessions = c.result("sessions/list", json!({"project_id": pid}));
    assert_eq!(sessions["sessions"][0]["task_count"], 1, "{sessions}");

    assert_eq!(c.result("shutdown", json!({})), json!({}));
    assert_eq!(c.child.wait().unwrap().code(), Some(0));
}

/// A plain folder becomes a repository when the client asks for it.
#[test]
fn project_open_initializes_git_on_request() {
    let data = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.md"), "# Notes\n").unwrap();
    let mut c = Client::spawn(data.path());
    c.result("initialize", json!({}));

    let plain = c.result("project/open", json!({"path": dir.path()}));
    assert!(plain["git"].is_null(), "{plain}");
    assert_eq!(plain["git_initialized"], false);
    let pid = plain["project_id"].as_str().unwrap().to_string();
    assert_eq!(c.result("project/close", json!({"project_id": pid})), json!({}));

    let opened = c.result("project/open", json!({"path": dir.path(), "init_git": true}));
    assert_eq!(opened["git_initialized"], true, "{opened}");
    assert!(opened["git"].is_object(), "{opened}");
    assert!(dir.path().join(".git").is_dir());
    assert_eq!(opened["project_id"], json!(pid), "the repository id is the root's, unchanged by git init");

    let again = c.result("project/open", json!({"path": dir.path(), "init_git": true}));
    assert_eq!(again["git_initialized"], false, "already a repository: {again}");

    assert_eq!(c.result("shutdown", json!({})), json!({}));
    assert_eq!(c.child.wait().unwrap().code(), Some(0));
}

/// The Changes tab's git actions: stage / unstage, per-file and all-in
/// commits, amend, undo, push with upstream, branches, discard, log and the
/// (mock) commit message.
#[test]
fn git_actions_stage_commit_push_and_branches() {
    let data = tempfile::tempdir().unwrap();
    let repo = fixture();
    let bare = tempfile::tempdir().unwrap();
    git(bare.path(), &["init", "-q", "--bare"]);
    git(repo.path(), &["remote", "add", "origin", bare.path().to_str().unwrap()]);
    git(repo.path(), &["config", "user.name", "fixture"]);
    git(repo.path(), &["config", "user.email", "fixture@example.invalid"]);
    let mut c = Client::spawn(data.path());
    c.result("initialize", json!({}));
    let info = c.result("project/open", json!({"path": repo.path()}));
    let pid = info["project_id"].as_str().unwrap().to_string();
    c.wait_notification(|n| n["method"] == "index/ready" && n["params"]["project_id"] == pid.as_str());
    let p = json!({"project_id": pid});

    let status = c.result("git/status", p.clone());
    let branch = status["branch"].as_str().expect("branch").to_string();
    assert!(status["upstream"].is_null(), "{status}");
    assert_eq!(status["remotes"][0]["name"], "origin", "{status}");
    assert_eq!(status["head_subject"], "initial", "{status}");
    let lib = |status: &Value| status["files"].as_array().unwrap().iter().find(|f| f["path"] == "src/lib.rs").cloned();
    assert_eq!(lib(&status).unwrap()["staged"], false);

    c.result("git/stage", json!({"project_id": pid, "paths": ["src/lib.rs"]}));
    let status = c.result("git/status", p.clone());
    assert_eq!(lib(&status).unwrap()["staged"], true, "{status}");
    let message = c.result("git/commit_message", p.clone());
    assert_eq!(message["source"], "heuristic");
    assert_eq!(message["message"], "Update lib.rs", "{message}");
    c.result("git/unstage", json!({"project_id": pid, "paths": ["src/lib.rs"]}));
    let status = c.result("git/status", p.clone());
    assert_eq!(lib(&status).unwrap()["staged"], false, "{status}");

    // A per-file commit leaves the other change alone.
    std::fs::write(repo.path().join("note.txt"), "a note\n").unwrap();
    let committed = c.result("git/commit", json!({"project_id": pid, "message": "Add note", "paths": ["note.txt"]}));
    assert_eq!(committed["subject"], "Add note", "{committed}");
    let status = c.result("git/status", p.clone());
    assert!(lib(&status).is_some(), "{status}");
    assert!(status["files"].as_array().unwrap().iter().all(|f| f["path"] != "note.txt"), "{status}");

    let empty = c.request("git/commit", json!({"project_id": pid, "message": "  ", "all": true}));
    assert_eq!(empty["error"]["code"], -32602, "{empty}");
    let committed = c.result("git/commit", json!({"project_id": pid, "message": "Update lib\n\nAdds sub.", "all": true}));
    assert_eq!(committed["subject"], "Update lib");
    let status = c.result("git/status", p.clone());
    assert!(status["files"].as_array().unwrap().is_empty(), "{status}");
    assert_eq!(status["head_subject"], "Update lib");
    let nothing = c.request("git/commit", json!({"project_id": pid, "message": "again"}));
    assert_eq!(nothing["error"]["code"], -32000, "{nothing}");
    assert!(nothing["error"]["message"].as_str().unwrap().contains("nothing to commit"), "{nothing}");
    let log = c.result("git/log", json!({"project_id": pid, "limit": 10}));
    let subjects: Vec<&str> = log["commits"].as_array().unwrap().iter().map(|e| e["subject"].as_str().unwrap()).collect();
    assert_eq!(subjects, vec!["Update lib", "Add note", "initial"], "{log}");

    let pushed = c.result("git/push", json!({"project_id": pid, "remote": "origin", "branch": branch, "set_upstream": true}));
    assert!(pushed["output"].as_str().unwrap().contains(&branch), "{pushed}");
    let status = c.result("git/status", p.clone());
    assert_eq!(status["upstream"], json!(format!("origin/{branch}")), "{status}");
    assert_eq!(status["ahead"], 0);
    assert_eq!(status["behind"], 0);
    let fetched = c.result("git/fetch", json!({"project_id": pid, "prune": true}));
    assert!(fetched["output"].is_string());
    let pulled = c.result("git/pull", p.clone());
    assert!(pulled["output"].is_string(), "{pulled}");

    // Amend diverges from the pushed commit; undo puts its changes back in the index.
    std::fs::write(repo.path().join("src/util.py"), "def clamp(x, lo, hi):\n    return max(lo, min(hi, x))\n\n# amended\n").unwrap();
    let amended = c.result("git/commit", json!({"project_id": pid, "message": "Update lib (amended)", "all": true, "amend": true}));
    assert_eq!(amended["subject"], "Update lib (amended)");
    let status = c.result("git/status", p.clone());
    assert_eq!(status["ahead"], 1, "{status}");
    assert_eq!(status["behind"], 1, "{status}");
    let log = c.result("git/log", json!({"project_id": pid}));
    assert_eq!(log["commits"].as_array().unwrap().len(), 3);
    let undone = c.result("git/undo_commit", p.clone());
    assert_eq!(undone["subject"], "Update lib (amended)", "{undone}");
    let status = c.result("git/status", p.clone());
    assert_eq!(status["head_subject"], "Add note", "{status}");
    assert!(status["files"].as_array().unwrap().iter().all(|f| f["staged"] == true), "{status}");
    assert_eq!(status["files"].as_array().unwrap().len(), 2, "{status}");

    // Unstage everything, then discard: tracked files go back, untracked ones away.
    c.result("git/unstage", json!({"project_id": pid, "paths": []}));
    std::fs::write(repo.path().join("scratch.txt"), "temp\n").unwrap();
    let status = c.result("git/status", p.clone());
    assert_eq!(status["files"].as_array().unwrap().len(), 3, "{status}");
    let discarded = c.result("git/discard", json!({"project_id": pid, "paths": ["scratch.txt"]}));
    assert_eq!(discarded["deleted"], 1, "{discarded}");
    assert!(!repo.path().join("scratch.txt").exists());
    let discarded = c.result("git/discard", json!({"project_id": pid}));
    assert_eq!(discarded["restored"], 2, "{discarded}");
    let status = c.result("git/status", p.clone());
    assert!(status["files"].as_array().unwrap().is_empty(), "{status}");
    let escape = c.request("git/discard", json!({"project_id": pid, "paths": ["../outside"]}));
    assert_eq!(escape["error"]["code"], -32602, "{escape}");

    // Branches.
    let switched = c.result("git/checkout", json!({"project_id": pid, "branch": "feature", "create": true}));
    assert_eq!(switched["branch"], "feature", "{switched}");
    let branches = c.result("git/branches", p.clone());
    assert_eq!(branches["current"], "feature", "{branches}");
    let local: Vec<&str> = branches["local"].as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap()).collect();
    assert!(local.contains(&"feature") && local.contains(&branch.as_str()), "{branches}");
    assert!(branches["remote"].as_array().unwrap().iter().any(|b| b["name"] == json!(format!("origin/{branch}")) && b["short"] == json!(branch)), "{branches}");
    let bad = c.request("git/checkout", json!({"project_id": pid, "branch": "-oops"}));
    assert_eq!(bad["error"]["code"], -32602, "{bad}");
    let back = c.result("git/checkout", json!({"project_id": pid, "branch": branch}));
    assert_eq!(back["branch"], json!(branch));

    assert_eq!(c.result("shutdown", json!({})), json!({}));
    assert_eq!(c.child.wait().unwrap().code(), Some(0));
}

/// A worktree is a second checkout opened as a project of its own: tasks in
/// the parent and the worktree run at the same time, its index starts from
/// the parent's, and `worktree/remove` takes the checkout and branch away.
#[test]
fn worktrees_run_in_parallel_and_are_removed() {
    let data = tempfile::tempdir().unwrap();
    let repo = fixture();
    let mut c = Client::spawn(data.path());
    c.result("initialize", json!({}));
    let info = c.result("project/open", json!({"path": repo.path()}));
    let pid = info["project_id"].as_str().unwrap().to_string();
    assert!(info["git"]["worktree"].is_null(), "{info}");
    c.wait_notification(|n| n["method"] == "index/ready" && n["params"]["project_id"] == pid.as_str());

    let wt = c.result("worktree/create", json!({"project_id": pid, "name": "Fix the Login bug"}));
    let wid = wt["project_id"].as_str().unwrap().to_string();
    assert_ne!(wid, pid);
    let root = wt["root"].as_str().unwrap().to_string();
    assert!(root.ends_with("fix-the-login-bug"), "{wt}");
    assert!(Path::new(&root).starts_with(ctx_core::platform::canonicalize(data.path().join("worktrees")).unwrap_or(data.path().join("worktrees"))) || root.contains("/worktrees/"), "{wt}");
    assert_eq!(wt["git"]["branch"], "ctx/fix-the-login-bug", "{wt}");
    assert_eq!(wt["git"]["worktree"]["parent_root"], json!(ctx_core::platform::canonicalize(repo.path()).unwrap()), "{wt}");
    assert_eq!(wt["git"]["worktree"]["managed"], true, "{wt}");
    assert_eq!(wt["git"]["dirty_files"], 0, "a fresh checkout: {wt}");
    let ready = c.wait_notification(|n| n["method"] == "index/ready" && n["params"]["project_id"] == wid.as_str());
    assert!(ready["params"]["error"].is_null(), "{ready}");
    // Seeded from the parent: README and util.py are unchanged; lib.rs
    // differs from the parent's uncommitted edit.
    assert!(ready["params"]["files_unchanged"].as_u64().unwrap() >= 2, "{ready}");

    // A second worktree with the same name gets a free slug.
    let again = c.result("worktree/create", json!({"project_id": pid, "name": "fix the login bug"}));
    assert_eq!(again["git"]["branch"], "ctx/fix-the-login-bug-2", "{again}");
    let again_id = again["project_id"].as_str().unwrap().to_string();
    c.wait_notification(|n| n["method"] == "index/ready" && n["params"]["project_id"] == again_id.as_str());

    let list = c.result("worktree/list", json!({"project_id": wid}));
    let entries = list["worktrees"].as_array().unwrap();
    assert_eq!(entries.len(), 3, "{list}");
    assert!(entries[0]["is_main"] == true && entries[1]["managed"] == true, "{list}");

    // Both projects run at once.
    let s1 = c.result("session/create", json!({"project_id": pid}))["id"].as_str().unwrap().to_string();
    let s2 = c.result("session/create", json!({"project_id": wid}))["id"].as_str().unwrap().to_string();
    assert_eq!(c.result("task/run", json!({"project_id": pid, "session_id": s1, "prompt": "Summarize this repository"}))["accepted"], true);
    assert_eq!(c.result("task/run", json!({"project_id": wid, "session_id": s2, "prompt": "Summarize this repository"}))["accepted"], true);
    let busy = c.request("worktree/remove", json!({"project_id": wid}));
    assert_eq!(busy["error"]["code"], -32002, "{busy}");
    let f1 = c.wait_notification(|n| n["method"] == "task/finished" && n["params"]["project_id"] == pid.as_str());
    let f2 = c.wait_notification(|n| n["method"] == "task/finished" && n["params"]["project_id"] == wid.as_str());
    assert_eq!(f1["params"]["status"], "completed", "{f1}");
    assert_eq!(f2["params"]["status"], "completed", "{f2}");

    let not_a_worktree = c.request("worktree/remove", json!({"project_id": pid}));
    assert_eq!(not_a_worktree["error"]["code"], -32602, "{not_a_worktree}");
    let removed = c.result("worktree/remove", json!({"project_id": wid, "delete_branch": true, "force": true}));
    assert_eq!(removed["branch_deleted"], true, "{removed}");
    assert!(!Path::new(&root).exists());
    let gone = c.request("project/status", json!({"project_id": wid}));
    assert_eq!(gone["error"]["code"], -32001, "{gone}");
    let list = c.result("worktree/list", json!({"project_id": pid}));
    assert_eq!(list["worktrees"].as_array().unwrap().len(), 2, "{list}");
    let branches = c.result("git/branches", json!({"project_id": pid}));
    assert!(branches["local"].as_array().unwrap().iter().all(|b| b["name"] != "ctx/fix-the-login-bug"), "{branches}");

    assert_eq!(c.result("shutdown", json!({})), json!({}));
    assert_eq!(c.child.wait().unwrap().code(), Some(0));
}

/// `worktree/merge` lands a worktree's commits in the parent checkout with a
/// merge commit; it refuses an uncommitted worktree, a busy project and a
/// conflict (aborting the merge), and can take the worktree away afterwards.
#[test]
fn worktree_merge_lands_the_branch_in_the_parent() {
    let data = tempfile::tempdir().unwrap();
    let repo = fixture();
    git(repo.path(), &["config", "user.name", "fixture"]);
    git(repo.path(), &["config", "user.email", "fixture@example.invalid"]);
    let mut c = Client::spawn(data.path());
    c.result("initialize", json!({}));
    let info = c.result("project/open", json!({"path": repo.path()}));
    let pid = info["project_id"].as_str().unwrap().to_string();
    c.wait_notification(|n| n["method"] == "index/ready" && n["params"]["project_id"] == pid.as_str());
    let parent_branch = c.result("git/status", json!({"project_id": pid}))["branch"].as_str().unwrap().to_string();

    let wt = c.result("worktree/create", json!({"project_id": pid, "name": "add docs"}));
    let wid = wt["project_id"].as_str().unwrap().to_string();
    let root = std::path::PathBuf::from(wt["root"].as_str().unwrap());
    c.wait_notification(|n| n["method"] == "index/ready" && n["params"]["project_id"] == wid.as_str());

    let not_a_worktree = c.request("worktree/merge", json!({"project_id": pid}));
    assert_eq!(not_a_worktree["error"]["code"], -32602, "{not_a_worktree}");
    let nothing = c.request("worktree/merge", json!({"project_id": wid}));
    assert_eq!(nothing["error"]["code"], -32000, "{nothing}");
    assert!(nothing["error"]["message"].as_str().unwrap().contains("nothing to merge"), "{nothing}");

    // Uncommitted work in the worktree is refused until it is committed there.
    std::fs::write(root.join("docs.md"), "# Docs\n").unwrap();
    let dirty = c.request("worktree/merge", json!({"project_id": wid}));
    assert_eq!(dirty["error"]["code"], -32000, "{dirty}");
    assert!(dirty["error"]["message"].as_str().unwrap().contains("uncommitted"), "{dirty}");
    let commit = c.result("git/commit", json!({"project_id": wid, "message": "Add docs", "all": true}));
    let wt_sha = commit["commit"].as_str().unwrap().to_string();

    // Busy parent: refused while its task runs.
    let s1 = c.result("session/create", json!({"project_id": pid}))["id"].as_str().unwrap().to_string();
    assert_eq!(c.result("task/run", json!({"project_id": pid, "session_id": s1, "prompt": "Summarize this repository"}))["accepted"], true);
    let busy = c.request("worktree/merge", json!({"project_id": wid}));
    assert_eq!(busy["error"]["code"], -32002, "{busy}");
    c.wait_notification(|n| n["method"] == "task/finished" && n["params"]["project_id"] == pid.as_str());

    // A conflicting edit in the parent: the merge is aborted and reported.
    std::fs::write(repo.path().join("docs.md"), "# Other docs\n").unwrap();
    git(repo.path(), &["add", "docs.md"]);
    git(repo.path(), &["commit", "-q", "-m", "Parent docs"]);
    let conflict = c.request("worktree/merge", json!({"project_id": wid}));
    assert_eq!(conflict["error"]["code"], -32000, "{conflict}");
    assert!(conflict["error"]["message"].as_str().unwrap().contains("docs.md"), "{conflict}");
    assert!(!repo.path().join(".git/MERGE_HEAD").exists(), "the merge was aborted");
    let status = c.result("git/status", json!({"project_id": pid}));
    assert!(status["files"].as_array().unwrap().iter().all(|f| f["path"] != "docs.md"), "{status}");
    git(repo.path(), &["reset", "-q", "--hard", "HEAD~1"]);

    let merged = c.result("worktree/merge", json!({"project_id": wid, "delete_after": true}));
    let sha = merged["merged"].as_str().unwrap().to_string();
    assert_ne!(sha, wt_sha, "a merge commit, not a fast-forward: {merged}");
    assert!(merged["subject"].as_str().unwrap().contains("ctx/add-docs"), "{merged}");
    assert_eq!(merged["removed"], true, "{merged}");
    let log = c.result("git/log", json!({"project_id": pid, "limit": 5}));
    let commits = log["commits"].as_array().unwrap();
    assert_eq!(commits[0]["hash"], sha.as_str(), "{log}");
    assert!(commits.iter().any(|e| e["hash"] == wt_sha.as_str()), "{log}");
    // The merge checks the file out with the user's line endings (CRLF under Git for Windows).
    assert_eq!(std::fs::read_to_string(repo.path().join("docs.md")).unwrap().replace("\r\n", "\n"), "# Docs\n");
    assert_eq!(c.result("git/status", json!({"project_id": pid}))["branch"], parent_branch.as_str());
    assert!(!root.exists(), "the worktree checkout is gone");
    assert_eq!(c.request("project/status", json!({"project_id": wid}))["error"]["code"], -32001);
    let branches = c.result("git/branches", json!({"project_id": pid}));
    assert!(branches["local"].as_array().unwrap().iter().all(|b| b["name"] != "ctx/add-docs"), "{branches}");

    assert_eq!(c.result("shutdown", json!({})), json!({}));
    assert_eq!(c.child.wait().unwrap().code(), Some(0));
}
