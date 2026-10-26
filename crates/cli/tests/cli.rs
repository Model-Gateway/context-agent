//! CLI smoke tests, including a headless run against a fake
//! OpenAI-compatible server (exercises the real HTTP + SSE transport).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn workspace_root() -> PathBuf {
    ctx_core::platform::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
}

fn snapshot(name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&workspace_root().join("tests/repositories").join(name), dir.path());
    dir
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let p = e.path();
        let name = e.file_name();
        if matches!(name.to_string_lossy().as_ref(), "target" | "__pycache__" | "node_modules") {
            continue;
        }
        if p.is_dir() {
            copy_dir(&p, &dst.join(name));
        } else {
            std::fs::copy(&p, dst.join(name)).unwrap();
        }
    }
}

fn ctx(data: &std::path::Path, repo: &std::path::Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_ctx"));
    c.arg("--data-dir").arg(data).arg("--repo").arg(repo).env_remove("OPENAI_API_KEY");
    c
}

#[test]
fn index_context_task_memory_and_doctor_commands() {
    let data = tempfile::tempdir().unwrap();
    let repo = snapshot("ts-web-app");
    let out = ctx(data.path(), repo.path()).args(["index", "rebuild"]).output().unwrap();
    assert!(out.status.success(), "stdout:\n{}\nstderr:\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("rebuilt index generation 1"), "{stdout}");

    let out = ctx(data.path(), repo.path()).args(["index", "status"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("generation      1") && stdout.contains("tsx="), "{stdout}");

    let out = ctx(data.path(), repo.path()).args(["context", "explain", "--query", "add a spinner to the login button"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("Selected sources") && stdout.contains("LoginButton.tsx"), "{stdout}");

    let out = ctx(data.path(), repo.path()).args(["context", "sources", "--query", "footer copyright"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("Footer.tsx"));

    let out = ctx(data.path(), repo.path()).args(["task", "list"]).output().unwrap();
    assert!(out.status.success());
    let out = ctx(data.path(), repo.path()).args(["memory", "list"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("memory record(s)"));

    let out = ctx(data.path(), repo.path()).args(["doctor"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("== validation commands") && stdout.contains("npm run test"), "{stdout}");
    assert!(stdout.contains("no capability profile cached"));
    // No API key configured → doctor reports a problem (exit 1) but runs.
    assert_eq!(out.status.code(), Some(1));

    let out = ctx(data.path(), repo.path()).args(["init"]).output().unwrap();
    assert!(out.status.success());
    assert!(data.path().join("config.toml").exists());
}

/// Minimal OpenAI-compatible chat-completions server with scripted replies.
fn fake_server(scripts: Vec<serde_json::Value>) -> (String, std::thread::JoinHandle<Vec<serde_json::Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let counter = Arc::new(AtomicUsize::new(0));
    let handle = std::thread::spawn(move || {
        let mut seen_bodies = Vec::new();
        let total = scripts.len();
        while counter.load(Ordering::SeqCst) < total {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            let (mut header_end, mut content_length) = (None, 0usize);
            loop {
                let n = stream.read(&mut tmp).unwrap();
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                if header_end.is_none() {
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = Some(pos + 4);
                        let headers = String::from_utf8_lossy(&buf[..pos]).to_lowercase();
                        content_length = headers.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                    }
                }
                if let Some(h) = header_end {
                    if buf.len() >= h + content_length {
                        break;
                    }
                }
            }
            let request_line = String::from_utf8_lossy(&buf[..buf.iter().position(|b| *b == b'\n').unwrap_or(0)]).to_string();
            let body: serde_json::Value = header_end.map(|h| serde_json::from_slice(&buf[h..]).unwrap_or(serde_json::Value::Null)).unwrap_or(serde_json::Value::Null);
            if request_line.starts_with("GET /v1/models") {
                let resp = r#"{"data":[{"id":"fake-model"}]}"#;
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", resp.len(), resp);
                continue;
            }
            let i = counter.fetch_add(1, Ordering::SeqCst);
            let script = scripts.get(i).cloned().unwrap_or_else(|| serde_json::json!({"text": "done"}));
            seen_bodies.push(body.clone());
            let stream_requested = body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
            let text = script.get("text").and_then(|t| t.as_str()).map(|s| s.to_string());
            let tool = script.get("tool").cloned();
            if stream_requested {
                let mut chunks = Vec::new();
                if let Some(t) = &text {
                    for word in t.split_inclusive(' ') {
                        chunks.push(serde_json::json!({"id": "c1", "choices": [{"index": 0, "delta": {"content": word}, "finish_reason": null}]}));
                    }
                }
                if let Some(tool) = &tool {
                    let args = tool["arguments"].to_string();
                    let (a, b) = args.split_at(args.len() / 2);
                    chunks.push(serde_json::json!({"id": "c1", "choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "id": format!("call_{i}"), "type": "function", "function": {"name": tool["name"], "arguments": a}}]}, "finish_reason": null}]}));
                    chunks.push(serde_json::json!({"id": "c1", "choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "function": {"arguments": b}}]}, "finish_reason": null}]}));
                }
                chunks.push(serde_json::json!({"id": "c1", "choices": [{"index": 0, "delta": {}, "finish_reason": if tool.is_some() { "tool_calls" } else { "stop" }}]}));
                chunks.push(serde_json::json!({"id": "c1", "choices": [], "usage": {"prompt_tokens": 120, "completion_tokens": 8, "prompt_tokens_details": {"cached_tokens": 64}}}));
                let mut payload = String::new();
                for c in chunks {
                    payload.push_str(&format!("data: {}\n\n", c));
                }
                payload.push_str("data: [DONE]\n\n");
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", payload.len(), payload);
            } else {
                let message = match &tool {
                    Some(tool) => serde_json::json!({"role": "assistant", "content": null, "tool_calls": [{"id": format!("call_{i}"), "type": "function", "function": {"name": tool["name"], "arguments": tool["arguments"].to_string()}}]}),
                    None => serde_json::json!({"role": "assistant", "content": text}),
                };
                let resp = serde_json::json!({"id": "resp", "choices": [{"index": 0, "message": message, "finish_reason": if tool.is_some() { "tool_calls" } else { "stop" }}], "usage": {"prompt_tokens": 120, "completion_tokens": 8}}).to_string();
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", resp.len(), resp);
            }
        }
        seen_bodies
    });
    (format!("http://{addr}/v1"), handle)
}

#[test]
fn headless_run_against_fake_openai_server() {
    let data = tempfile::tempdir().unwrap();
    let repo = snapshot("py-service");
    let (base_url, server) = fake_server(vec![
        serde_json::json!({"tool": {"name": "read_file", "arguments": {"path": "svc/pricing.py"}}}),
        serde_json::json!({"tool": {"name": "apply_patch", "arguments": {"path": "svc/pricing.py", "edits": [{"old_text": "return round(price - pct, 2)", "new_text": "return round(price * (100 - pct) / 100, 2)"}]}}}),
        serde_json::json!({"text": "Fixed the percentage discount. Validation: see the automatic test run."}),
    ]);
    std::fs::write(
        repo.path().join(".context-agent.toml"),
        format!("[validation]\ncommands = [\"{} -m unittest discover -s tests -v\"]\n[agent]\nauto_validate = true\n[permissions]\nprofile = \"workspace_autonomous\"\n", ctx_core::platform::python_interpreter()),
    )
    .unwrap();
    let out = ctx(data.path(), repo.path())
        .env("CTX_MODEL_BASE_URL", &base_url)
        .env("CTX_MODEL_NAME", "fake-model")
        .env("CTX_MODEL_API_KEY_ENV", "FAKE_KEY")
        .env("FAKE_KEY", "sk-fake-key-for-tests-0000000000")
        .args(["run", "--json", "--approval", "auto", "--no-probe", "--new-session", "fix the discount bug in svc/pricing.py"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "stdout:\n{stdout}\nstderr:\n{stderr}");
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    assert_eq!(json["outcome"]["status"], "completed", "{json:#}");
    assert_eq!(json["outcome"]["modified_files"][0], "svc/pricing.py", "{json:#}\nstderr:\n{stderr}");
    assert_eq!(json["outcome"]["validation"][0]["passed"], true, "automatic validation ran: {json:#}");
    assert_eq!(json["outcome"]["usage"]["source"], "reported");
    assert_eq!(json["outcome"]["usage"]["cached_prompt_tokens"], 64 * 3);
    assert!(stderr.contains("\"event\":\"tool_call_finished\""), "{stderr}");
    let bodies = server.join().unwrap();
    assert_eq!(bodies.len(), 3);
    // Trust boundary and tool wiring on the wire.
    assert_eq!(bodies[0]["messages"][0]["role"], "system");
    assert!(bodies[0]["tools"].as_array().map(|t| t.len() >= 10).unwrap_or(false));
    assert!(bodies[0]["stream"].as_bool().unwrap_or(false), "streaming was used");
    assert_eq!(bodies[1]["messages"].as_array().unwrap().last().unwrap()["role"], "tool");
    assert!(!bodies[0].to_string().contains("sk-fake-key"), "the API key never appears in a request body");

    // Follow-up listing shows the task.
    let out = ctx(data.path(), repo.path()).args(["task", "list"]).output().unwrap();
    let listing = String::from_utf8_lossy(&out.stdout);
    assert!(listing.contains("completed") && listing.contains("discount"), "{listing}");
    let out = ctx(data.path(), repo.path()).args(["context", "explain"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("Selected sources"));
    let out = ctx(data.path(), repo.path()).args(["memory", "list"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("episodic"));
}
