//! Storage, git, model-list, configuration-patch and doctor helpers shared by the request handlers.

use super::*;

// ---------------------------------------------------------------------------
// Sessions and tasks: storage helpers
// ---------------------------------------------------------------------------

pub(super) fn truncate_chars(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    line.chars().take(max).collect()
}

pub(super) fn session_summary(store: &SessionStore, s: &SessionRecord) -> Result<SessionSummary> {
    let tasks = store.tasks_for_session(&s.id)?;
    let title = s
        .metadata
        .get("title")
        .and_then(|t| t.as_str())
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .or_else(|| tasks.first().map(|t| truncate_chars(&t.objective, 80)).filter(|t| !t.is_empty()))
        .unwrap_or_else(|| "New chat".to_string());
    Ok(SessionSummary {
        id: s.id.clone(),
        model: s.model.clone(),
        mode: s.mode.clone(),
        title,
        created_at: s.created_at.to_rfc3339(),
        updated_at: s.updated_at.to_rfc3339(),
        active_task_id: s.active_task_id.clone(),
        task_count: tasks.len(),
        // The runtime continues older tasks, so "last" is the task touched
        // most recently, not the one created last.
        last_status: tasks.iter().max_by_key(|t| t.updated_at).and_then(|t| t.status.clone()),
    })
}

/// The next unconsumed `tool_output.full_len` for `call_id`: rows are
/// paired with tool results positionally because call ids repeat inside a
/// continued task.
pub(super) fn take_full_len(lens: &mut HashMap<String, VecDeque<u64>>, call_id: &str) -> Option<u64> {
    lens.get_mut(call_id).and_then(|q| q.pop_front())
}

pub(super) fn task_summary(t: &TaskRecord, with_session: bool) -> TaskSummary {
    let state = ctx_memory::TaskState::from_json(&t.state_json);
    TaskSummary {
        id: t.id.clone(),
        session_id: with_session.then(|| t.session_id.clone()),
        objective: t.objective.clone(),
        category: t.category.clone(),
        state: t.state.clone(),
        status: t.status.clone(),
        summary: t.summary.clone(),
        created_at: t.created_at.to_rfc3339(),
        updated_at: t.updated_at.to_rfc3339(),
        completed_at: t.completed_at.map(|c| c.to_rfc3339()),
        modified_files: state.as_ref().map(|s| s.modified_files.clone()).unwrap_or_default(),
        unresolved: state.as_ref().map(|s| s.unresolved.clone()).unwrap_or_default(),
    }
}

/// Store (or clear) `metadata.title` for a session and bump `updated_at`.
pub(super) fn set_session_title(store: &SessionStore, session_id: &str, title: Option<&str>) -> Result<()> {
    let session = store.get_session(session_id)?.ok_or_else(|| CtxError::NotFound(format!("session {session_id}")))?;
    let mut metadata = match session.metadata {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    match title {
        Some(t) => {
            metadata.insert("title".into(), Value::String(t.to_string()));
        }
        None => {
            metadata.remove("title");
        }
    }
    let metadata = Value::Object(metadata).to_string();
    let now = ctx_core::now().to_rfc3339();
    store.database().with(|c| {
        c.execute("UPDATE sessions SET metadata = ?1, updated_at = ?2 WHERE id = ?3", (&metadata, &now, session_id))?;
        Ok(())
    })
}

/// Delete a session and everything that belongs to it in one transaction.
/// No table declares foreign keys, so nothing cascades on its own;
/// episodic memories are kept (they are repository knowledge) but lose
/// their task link, as do tasks that pointed at a deleted parent.
pub(super) fn delete_session(store: &SessionStore, session_id: &str) -> Result<()> {
    store.database().with_tx(|tx| snapshot::delete_session_rows(tx, session_id))
}

/// The `tool_output` rows of a task after `after_id`.
pub(super) fn tool_output_rows(store: &SessionStore, task_id: &str, after_id: i64, limit: usize) -> Result<Vec<(i64, Value)>> {
    store.database().with(|c| {
        let mut stmt = c.prepare("SELECT id, payload FROM events WHERE task_id = ?1 AND kind = 'tool_output' AND id > ?2 ORDER BY id ASC LIMIT ?3")?;
        let rows = stmt.query_map((task_id, after_id, limit as i64), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        rows.map(|row| row.map(|(id, payload)| (id, serde_json::from_str(&payload).unwrap_or(Value::Null)))).collect()
    })
}

/// Bytes of each sub-agent tool output `session/history` replays. Agents
/// make thousands of calls in a long session; their full outputs (up to
/// 24 KB each) made one history tens of megabytes.
const AGENT_OUTPUT_PREVIEW_BYTES: usize = 1_000;

/// The longest prefix of `text` within `max` bytes, cut on a char boundary.
fn clip_bytes(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut cut = max;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    &text[..cut]
}

/// History replay of sub-agents: the lead task's own tool outputs come from
/// its conversation, but an agent's conversation lives in its own session,
/// so its persisted `tool_output` rows are turned into `tool_call_output`
/// events (wrapped in `agent_progress`) right after each wrapped
/// `tool_call_finished`, paired positionally per call id, with the content
/// clipped to `AGENT_OUTPUT_PREVIEW_BYTES` (`full_len` keeps the full size).
pub(super) fn with_agent_tool_outputs(store: &SessionStore, events: Vec<AgentEvent>) -> Result<Vec<AgentEvent>> {
    let agent_tasks: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::AgentStarted { agent_task_id, .. } => Some(agent_task_id.as_str().to_string()),
            _ => None,
        })
        .collect();
    if agent_tasks.is_empty() {
        return Ok(events);
    }
    let mut outputs: BTreeMap<(String, String), VecDeque<Value>> = BTreeMap::new();
    for task in &agent_tasks {
        for (_, payload) in tool_output_rows(store, task, 0, usize::MAX as usize / 2)? {
            if let Some(call_id) = payload.get("call_id").and_then(|c| c.as_str()) {
                outputs.entry((task.clone(), call_id.to_string())).or_default().push_back(payload.clone());
            }
        }
    }
    let mut out = Vec::with_capacity(events.len());
    for event in events {
        let synthetic = match &event {
            AgentEvent::AgentProgress { task_id, agent_id, inner } => match inner.as_ref() {
                AgentEvent::ToolCallFinished { task_id: agent_task, call_id, name, success, .. } => {
                    outputs.get_mut(&(agent_task.as_str().to_string(), call_id.clone())).and_then(|q| q.pop_front()).map(|payload| {
                        let content = payload.get("content").and_then(|c| c.as_str()).unwrap_or("");
                        AgentEvent::AgentProgress {
                            task_id: task_id.clone(),
                            agent_id: agent_id.clone(),
                            inner: Box::new(AgentEvent::ToolCallOutput {
                                task_id: agent_task.clone(),
                                call_id: call_id.clone(),
                                name: payload.get("tool").and_then(|t| t.as_str()).unwrap_or(name).to_string(),
                                content: clip_bytes(content, AGENT_OUTPUT_PREVIEW_BYTES).to_string(),
                                // Never below the stored content's length, so a clipped preview reports what it left out.
                                full_len: (payload.get("full_len").and_then(|n| n.as_u64()).unwrap_or(0) as usize).max(content.len()),
                                is_error: !success,
                            }),
                        }
                    })
                }
                _ => None,
            },
            _ => None,
        };
        out.push(event);
        if let Some(s) = synthetic {
            out.push(s);
        }
    }
    Ok(out)
}

/// Most images, and most files, one `task/run` may carry.
pub(super) const MAX_IMAGES_PER_RUN: usize = 20;
pub(super) const MAX_FILES_PER_RUN: usize = 20;

/// The images of a `task/run`, read and encoded: paths on this machine, or
/// bytes sent inline by a client on another one. A missing file, an
/// unsupported format or an oversized file is `-32602`, so the client hears
/// about it before a run starts.
pub(super) fn read_images(images: &[ImageParam]) -> RpcResult<Vec<ImageInput>> {
    if images.len() > MAX_IMAGES_PER_RUN {
        return Err(RpcError::invalid_params(format!("at most {MAX_IMAGES_PER_RUN} images per message ({} sent)", images.len())));
    }
    images
        .iter()
        .map(|image| match image {
            ImageParam::Path(path) => {
                if !Path::new(path).is_absolute() {
                    return Err(RpcError::invalid_params(format!("image path must be absolute: {path}")));
                }
                let len = std::fs::metadata(path).map_err(|e| RpcError::invalid_params(format!("cannot read image {path}: {e}")))?.len();
                if len > ImageInput::MAX_BYTES as u64 {
                    return Err(RpcError::invalid_params(format!("image {path} is larger than {} MB", ImageInput::MAX_BYTES >> 20)));
                }
                let bytes = std::fs::read(path).map_err(|e| RpcError::invalid_params(format!("cannot read image {path}: {e}")))?;
                ImageInput::from_bytes(&bytes, Some(path.clone())).ok_or_else(|| RpcError::invalid_params(format!("{path} is not a PNG, JPEG, GIF or WebP image")))
            }
            ImageParam::Inline { data, name } => {
                use base64::Engine as _;
                let label = name.clone().unwrap_or_else(|| "image".to_string());
                // 4 base64 characters per 3 bytes: refuse before decoding a huge payload.
                if data.len() > ImageInput::MAX_BYTES / 3 * 4 + 4 {
                    return Err(RpcError::invalid_params(format!("image {label} is larger than {} MB", ImageInput::MAX_BYTES >> 20)));
                }
                let bytes = base64::engine::general_purpose::STANDARD.decode(data.trim()).map_err(|e| RpcError::invalid_params(format!("image {label} is not valid base64: {e}")))?;
                if bytes.len() > ImageInput::MAX_BYTES {
                    return Err(RpcError::invalid_params(format!("image {label} is larger than {} MB", ImageInput::MAX_BYTES >> 20)));
                }
                ImageInput::from_bytes(&bytes, name.clone()).ok_or_else(|| RpcError::invalid_params(format!("{label} is not a PNG, JPEG, GIF or WebP image")))
            }
        })
        .collect()
}

/// The text files of a `task/run`, read and cut at `FileInput::MAX_BYTES`:
/// paths on this machine, or their text sent inline. A missing or binary
/// file is `-32602`, so the client hears about it before a run starts.
pub(super) fn read_files(files: &[FileParam]) -> RpcResult<Vec<FileInput>> {
    if files.len() > MAX_FILES_PER_RUN {
        return Err(RpcError::invalid_params(format!("at most {MAX_FILES_PER_RUN} files per message ({} sent)", files.len())));
    }
    files
        .iter()
        .map(|file| {
            if let Some(content) = &file.content {
                let name = file
                    .name
                    .clone()
                    .or_else(|| file.path.as_deref().and_then(|p| Path::new(p).file_name()).map(|n| n.to_string_lossy().into_owned()))
                    .ok_or_else(|| RpcError::invalid_params("an inline file needs a name"))?;
                // Listed in history under its name (the client's path, when
                // it sent one, is on a machine this server cannot read).
                let shown = file.path.clone().unwrap_or_else(|| name.clone());
                return FileInput::from_bytes(name.clone(), content.as_bytes(), Some(shown)).ok_or_else(|| RpcError::invalid_params(format!("{name} is not a UTF-8 text file")));
            }
            let Some(path_text) = file.path.as_deref() else {
                return Err(RpcError::invalid_params("a file needs a path or inline content"));
            };
            let path = Path::new(path_text);
            if !path.is_absolute() {
                return Err(RpcError::invalid_params(format!("file path must be absolute: {path_text}")));
            }
            let cannot_read = |e: std::io::Error| RpcError::invalid_params(format!("cannot read file {path_text}: {e}"));
            let mut bytes = Vec::new();
            // One byte past the limit tells a cut file from one that fits.
            std::io::Read::read_to_end(&mut std::io::Read::take(std::fs::File::open(path).map_err(cannot_read)?, FileInput::MAX_BYTES as u64 + 1), &mut bytes).map_err(cannot_read)?;
            let len = std::fs::metadata(path).map_err(cannot_read)?.len();
            let name = file.name.clone().unwrap_or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path_text.to_string()));
            let mut input = FileInput::from_bytes(name, &bytes, Some(path_text.to_string()))
                .ok_or_else(|| RpcError::invalid_params(format!("{path_text} is not a UTF-8 text file")))?;
            input.omitted_bytes = len.saturating_sub(input.content.len() as u64);
            Ok(input)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Host
// ---------------------------------------------------------------------------

/// `initialize.host`: the machine this server runs on.
pub(super) fn host_info() -> HostInfo {
    let home = ctx_core::platform::home_dir().map(|p| p.display().to_string()).unwrap_or_default();
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
    HostInfo { hostname: local_hostname(), user, home_dir: home, os: std::env::consts::OS }
}

/// The machine's name: `gethostname` on Unix, else the environment.
fn local_hostname() -> String {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: `buf` is a valid, writable buffer of the given length.
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
        if rc == 0 {
            let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
            let name = String::from_utf8_lossy(&buf[..end]).trim().to_string();
            if !name.is_empty() {
                return name;
            }
        }
    }
    std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")).unwrap_or_default()
}

/// `fs/list`: the entries of a directory on this machine, for the desktop
/// app's folder picker and file tree when it runs on another machine.
/// `path` is `~`-expanded; absent or empty lists the home directory.
/// Directories first, then files, each sorted by name; `.git` and Finder
/// metadata are left out.
pub(super) fn list_directory(path: Option<&str>, dirs_only: bool) -> RpcResult<FsListing> {
    let home = ctx_core::platform::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let requested = match path.map(str::trim).filter(|p| !p.is_empty()) {
        None | Some("~") => home.clone(),
        Some(p) if p.starts_with("~/") => home.join(&p[2..]),
        Some(p) => PathBuf::from(p),
    };
    if !requested.is_absolute() {
        return Err(RpcError::invalid_params(format!("path must be absolute: {}", requested.display())));
    }
    let dir = ctx_core::platform::canonicalize(&requested).map_err(|e| RpcError::invalid_params(format!("cannot list {}: {e}", requested.display())))?;
    if !dir.is_dir() {
        return Err(RpcError::invalid_params(format!("{} is not a directory", dir.display())));
    }
    let read = std::fs::read_dir(&dir).map_err(|e| RpcError::invalid_params(format!("cannot list {}: {e}", dir.display())))?;
    let mut entries: Vec<FsEntry> = read
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".git" || name == ".DS_Store" {
                return None;
            }
            // A symlink to a directory lists as one; a broken link is a file.
            let is_dir = entry.path().is_dir();
            if dirs_only && !is_dir {
                return None;
            }
            let path = entry.path();
            let is_git = is_dir && path.join(".git").exists();
            Some(FsEntry { name: name.clone(), path: path.display().to_string(), is_dir, is_git, is_hidden: name.starts_with('.') })
        })
        .collect();
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())).then_with(|| a.name.cmp(&b.name)));
    Ok(FsListing { parent: dir.parent().map(|p| p.display().to_string()), path: dir.display().to_string(), home: home.display().to_string(), entries })
}

/// Unknown session ids surface as `-32602` rather than a backend error.
pub(super) fn unknown_id_is_invalid_params(e: RpcError) -> RpcError {
    if e.code == BACKEND_ERROR && e.data.as_ref().and_then(|d| d.get("kind")).and_then(|k| k.as_str()) == Some("invalid_argument") {
        RpcError::invalid_params(e.message)
    } else {
        e
    }
}

// ---------------------------------------------------------------------------
// Git
// ---------------------------------------------------------------------------

pub(super) async fn git(root: &Path, args: &[&str]) -> Result<std::process::Output> {
    let mut command = tokio::process::Command::new("git");
    ctx_core::platform::hide_console_window(command.as_std_mut());
    let output = command
        .args(args)
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .output()
        .await?;
    Ok(output)
}

/// The first non-empty line of git's stderr: the actual error, without the
/// kilobytes of usage text git appends to some of them.
pub(super) fn git_error_line(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("unknown error").to_string()
}

/// Parse `git status --porcelain=v1 -z`: NUL-separated `XY path` entries,
/// with the original path as an extra field for renames and copies.
pub(super) fn parse_porcelain_z(bytes: &[u8]) -> Vec<GitFileStatus> {
    let mut fields = bytes.split(|b| *b == 0).filter(|f| !f.is_empty()).map(|f| String::from_utf8_lossy(f).to_string());
    let mut out = Vec::new();
    while let Some(entry) = fields.next() {
        if entry.len() < 4 || !entry.is_char_boundary(2) || entry.as_bytes()[2] != b' ' {
            continue;
        }
        let (xy, path) = (&entry[..2], &entry[3..]);
        let x = xy.as_bytes()[0] as char;
        let y = xy.as_bytes()[1] as char;
        if matches!(x, 'R' | 'C') || matches!(y, 'R' | 'C') {
            // The next field is the original path.
            let _ = fields.next();
        }
        let status = if xy == "??" { "?".to_string() } else { xy.trim().to_string() };
        out.push(GitFileStatus { path: path.to_string(), status, staged: x != ' ' && x != '?', insertions: None, deletions: None });
    }
    out
}

/// `git diff --numstat -z`: `added\tremoved\tpath\0` per file (`-\t-` for a
/// binary file → `None`); a rename carries an empty path followed by the
/// old and the new path as two more fields and is keyed by the new one.
pub(super) fn parse_numstat_z(bytes: &[u8]) -> HashMap<String, Option<(u64, u64)>> {
    let mut fields = bytes.split(|b| *b == 0).map(|f| String::from_utf8_lossy(f).to_string());
    let mut out = HashMap::new();
    while let Some(entry) = fields.next() {
        if entry.is_empty() {
            continue;
        }
        let mut parts = entry.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let path = if path.is_empty() {
            let _old = fields.next();
            match fields.next() {
                Some(new) => new,
                None => break,
            }
        } else {
            path.to_string()
        };
        let counts = match (added.parse::<u64>(), removed.parse::<u64>()) {
            (Ok(a), Ok(r)) => Some((a, r)),
            _ => None,
        };
        out.insert(path, counts);
    }
    out
}

/// Lines of an untracked file (its whole content is "added"): `None` for a
/// binary (NUL byte), unreadable or very large file.
pub(super) fn count_text_lines(path: &Path) -> Option<u64> {
    const MAX_BYTES: u64 = 4 * 1024 * 1024;
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    let newlines = bytes.iter().filter(|b| **b == b'\n').count() as u64;
    Some(if bytes.is_empty() || bytes.ends_with(b"\n") { newlines } else { newlines + 1 })
}

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

pub(super) fn with_current_first(mut models: Vec<String>, current: &str) -> Vec<String> {
    if !models.iter().any(|m| m == current) {
        models.insert(0, current.to_string());
    }
    models
}

// ---------------------------------------------------------------------------
// Configuration patching
// ---------------------------------------------------------------------------

pub(super) fn json_to_toml(value: &Value) -> std::result::Result<toml::Value, String> {
    Ok(match value {
        Value::Null => return Err("null is only valid as a whole value (it removes the key)".into()),
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml::Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                toml::Value::Float(f)
            } else {
                return Err(format!("number {n} is out of range"));
            }
        }
        Value::String(s) => toml::Value::String(s.clone()),
        Value::Array(items) => toml::Value::Array(items.iter().map(json_to_toml).collect::<std::result::Result<Vec<_>, _>>()?),
        Value::Object(map) => {
            let mut table = toml::Table::new();
            for (k, v) in map {
                table.insert(k.clone(), json_to_toml(v)?);
            }
            toml::Value::Table(table)
        }
    })
}

/// Set (or, with `None`, remove) a dotted path in a TOML table, creating
/// intermediate tables as needed.
pub(super) fn set_path(table: &mut toml::Table, path: &[&str], value: Option<toml::Value>) {
    let Some((head, rest)) = path.split_first() else { return };
    if rest.is_empty() {
        match value {
            Some(v) => {
                table.insert(head.to_string(), v);
            }
            None => {
                table.remove(*head);
            }
        }
        return;
    }
    match value {
        Some(_) => {
            let entry = table.entry(head.to_string()).or_insert_with(|| toml::Value::Table(toml::Table::new()));
            if !entry.is_table() {
                *entry = toml::Value::Table(toml::Table::new());
            }
            if let Some(child) = entry.as_table_mut() {
                set_path(child, rest, value);
            }
        }
        None => {
            // Removing the last key of a table removes the table: an empty
            // `[pricing]` would otherwise deserialize to `Some(default)`.
            let now_empty = match table.get_mut(*head) {
                Some(toml::Value::Table(child)) => {
                    set_path(child, rest, None);
                    child.is_empty()
                }
                _ => false,
            };
            if now_empty {
                table.remove(*head);
            }
        }
    }
}

/// The shape of a configuration file, derived from the default `Config`
/// with every optional table and field present, for `check_patch_path`.
pub(super) fn patch_schema() -> std::result::Result<toml::Table, String> {
    let mut config = Config::default();
    config.pricing = Some(ctx_core::PricingConfig { input_per_million: Some(0.0), cached_input_per_million: Some(0.0), output_per_million: Some(0.0), models: Default::default() });
    config.spend.max_usd = Some(0.0);
    config.model.temperature = Some(1.0);
    config.model.reasoning_effort = Some(ctx_core::ReasoningEffort::default());
    config.model.project_header = Some("X-Project".to_string());
    config.model.credential = Some("modelgateway".to_string());
    config.embeddings.model_path = Some(PathBuf::from("model.onnx"));
    // Free-form maps and lists: an empty table means "children unchecked".
    config.mcp.servers.clear();
    config.mcp.uninstalled = vec![String::new()];
    let text = toml::to_string(&config).map_err(|e| e.to_string())?;
    text.parse::<toml::Table>().map_err(|e| e.to_string())
}

/// Every segment of a patched key must name a known field; an empty table
/// in the schema is a free-form map (`model.extra_headers`) whose children
/// are not checked. The lenient loader (`#[serde(default)]` without
/// `deny_unknown_fields`) would otherwise write `model.nmae` to disk.
pub(super) fn check_patch_path(schema: &toml::Table, segments: &[&str]) -> std::result::Result<(), String> {
    let mut node = schema;
    for (i, segment) in segments.iter().enumerate() {
        let path = segments[..=i].join(".");
        match node.get(*segment) {
            Some(toml::Value::Table(child)) => {
                if child.is_empty() {
                    return Ok(());
                }
                node = child;
            }
            Some(_) => {
                if i + 1 < segments.len() {
                    return Err(format!("configuration key `{path}` is not a table"));
                }
                return Ok(());
            }
            None => return Err(format!("configuration key `{path}` is unknown")),
        }
    }
    Ok(())
}

/// [`check_patch_path`] for a patched value: an object value is checked leaf
/// by leaf, so `{"model.capabilities": {"streming": false}}` is refused
/// exactly like the dotted `model.capabilities.streming` (children of a
/// free-form table such as `model.extra_headers` stay unchecked); every
/// other value is checked by its key alone.
pub(super) fn check_patch_value(schema: &toml::Table, segments: &[&str], value: &Value) -> std::result::Result<(), String> {
    check_patch_path(schema, segments)?;
    if let Value::Object(map) = value {
        for (key, child) in map {
            let mut path = segments.to_vec();
            path.push(key.as_str());
            check_patch_value(schema, &path, child)?;
        }
    }
    Ok(())
}

/// The file a write to `path` must replace: the target of a symlink (a
/// `~/.context-agent/config.toml` that points into a dotfiles repository
/// keeps pointing there), else `path` itself. A dangling link is followed
/// one level so the target gets created.
fn write_target(path: &Path) -> std::io::Result<PathBuf> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => match std::fs::canonicalize(path) {
            Ok(target) => Ok(target),
            Err(_) => {
                let raw = std::fs::read_link(path)?;
                Ok(if raw.is_absolute() { raw } else { path.parent().unwrap_or_else(|| Path::new(".")).join(raw) })
            }
        },
        _ => Ok(path.to_path_buf()),
    }
}

/// Write `text` to `path` atomically (temp file in the same directory, then
/// rename) with mode 0600: the file may carry `model.extra_headers`
/// credentials, and a crash mid-write must never leave a truncated file
/// that stops every later `Config::load`. A symlinked `path` is written
/// through to its target, so the link survives.
pub(crate) fn write_private_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let target = write_target(path)?;
    let path = target.as_path();
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "config".into());
    let tmp = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

// ---------------------------------------------------------------------------
// Doctor
// ---------------------------------------------------------------------------

pub(super) fn build_doctor_report(config: &Config, project: Option<&Project>, profile: Option<&CapabilityProfile>, mock: bool, mcp: &[ctx_tools::McpServerStatus]) -> Result<DoctorReport> {
    let mut report = DoctorReport::default();
    report.section("configuration");
    report.row("data dir", config.data_dir.display().to_string(), true);
    report.row("config sources", if config.sources.is_empty() { "built-in defaults only (run `ctx init`)".to_string() } else { config.sources.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ") }, true);
    match std::fs::create_dir_all(&config.data_dir) {
        Ok(_) => report.row("data dir writable", "yes", true),
        Err(e) => report.row("data dir writable", format!("NOT writable: {e}"), false),
    }
    let wrapper = ctx_security::SandboxStatus::from_config(&config.permissions.sandbox);
    report.row("permissions", format!("{}{}", config.permissions.profile.as_str(), if wrapper.enforced { format!(" · command wrapper (developer mode): {}", wrapper.description) } else { String::new() }), true);
    report.row("reasoning effort", config.model.reasoning_effort.as_ref().map(|e| format!("{}{}", e.as_str(), if e.deploys_agents() { " (deploys sub-agents)" } else { "" })).unwrap_or_else(|| "auto".into()), true);
    report.row("model resilience", format!("{} retries (backoff {}–{} ms) · fallbacks: {}", config.model.max_retries, config.model.retry_base_delay_ms, config.model.retry_max_delay_ms, config.model.fallbacks_summary()), true);
    for fb in config.model.chain().iter().skip(1) {
        // A fallback sharing the primary's key is covered by the line above.
        if (fb.api_key_env != config.model.api_key_env || fb.credential != config.model.credential) && ctx_core::resolve_api_key(&config, fb).is_none() && !fb.base_url.contains("localhost") && !fb.base_url.contains("127.0.0.1") {
            report.row("fallback key", format!("{} at {}: {} NOT SET", fb.name, fb.base_url, ctx_core::api_key_source(fb)), false);
        }
    }
    if !config.ignored_repository_keys.is_empty() {
        report.row("note", format!("repository .context-agent.toml tried to set [{}]; only trusted configuration may set these", config.ignored_repository_keys.join("], [")), true);
    }

    report.section("repository");
    let repo_profile = match project {
        Some(p) => {
            report.row("root", p.ctx.root.display().to_string(), true);
            match RepositoryDiscovery::git_info(&p.ctx.root) {
                Some(g) => report.row("git", format!("branch {} · head {} · {} uncommitted", g.branch.unwrap_or_else(|| "detached".into()), g.head.map(|h| h[..8.min(h.len())].to_string()).unwrap_or_default(), g.dirty_files.len()), true),
                None => report.row("git", "not a git repository (indexing still works; .gitignore rules are honoured if present)", true),
            }
            let stats = p.ctx.stores.index.stats()?;
            report.row("index", format!("generation {} · {} files · {} symbols · {} chunks · {} embeddings", stats.generation, stats.files, stats.symbols, stats.chunks, stats.embeddings), true);
            let profile = p.ctx.profile()?;
            report.row("languages", profile.languages.iter().map(|(l, n)| format!("{l}={n}")).collect::<Vec<_>>().join(", "), true);
            Some(profile)
        }
        None => {
            report.row("root", "no project open", true);
            None
        }
    };

    report.section("validation commands");
    match project {
        Some(p) => {
            let commands = p.ctx.validation_commands()?;
            if commands.is_empty() {
                report.row("none", "none detected (configure [validation].commands or add a build file)", true);
            }
            for c in &commands {
                let program = c.command.split_whitespace().next().unwrap_or("");
                let present = program.starts_with("./") || ctx_evaluation::spec::which(program);
                report.row(&format!("{:?}", c.kind).to_lowercase(), format!("{} (cwd {}, from {}){}", c.command, c.cwd, c.source, if present { "" } else { " — MISSING TOOL" }), present);
            }
        }
        None => report.row("none", "no project open", true),
    }

    report.section("language servers");
    for s in ctx_lsp::ServerCatalog::all_statuses() {
        let relevant = repo_profile.as_ref().map(|p| p.languages.contains_key(&s.language)).unwrap_or(true);
        if relevant {
            report.row(&s.language, format!("{} {}", s.executable, if s.installed { "installed".to_string() } else { format!("not installed ({})", s.install_hint) }), true);
        }
    }

    if !config.mcp.servers.is_empty() {
        report.section("mcp servers");
        if !config.mcp.enabled {
            report.row("mcp", "disabled (mcp.enabled = false)", true);
        }
        for status in mcp {
            let (value, ok) = mcp_doctor_line(status);
            report.row(&status.name, value, ok);
        }
    }

    report.section("model provider");
    if mock {
        report.row("endpoint", "mock provider (--mock): no network access", true);
    } else {
        let key_set = ctx_core::resolve_api_key(&config, &config.model).is_some();
        let local = config.model.base_url.contains("localhost") || config.model.base_url.contains("127.0.0.1");
        report.row("endpoint", format!("{} · model {} · key {} ({})", config.model.base_url, config.model.name, ctx_core::api_key_source(&config.model), if key_set { "set" } else { "NOT SET" }), key_set || local);
    }
    match profile {
        Some(p) => {
            report.row("probed at", p.probed_at.map(|t| t.to_rfc3339()).unwrap_or_else(|| "never".into()), true);
            for (name, status) in [
                ("connectivity", &p.connectivity),
                ("model listed", &p.model_listed),
                ("chat completion", &p.chat_completion),
                ("tool calling", &p.tool_calling),
                ("tool continuation", &p.tool_result_continuation),
                ("parallel tools", &p.parallel_tool_calls),
                ("streaming", &p.streaming),
                ("stream usage", &p.stream_usage),
                ("usage reporting", &p.usage_reporting),
                ("cached tokens", &p.cached_token_reporting),
                ("reasoning controls", &p.reasoning_controls),
                ("developer role", &p.developer_role),
            ] {
                report.row(name, format!("{} {}", status.symbol(), status.detail()), true);
            }
            report.row("max tokens field", p.max_tokens_field.clone(), true);
            for n in &p.notes {
                report.row("note", n.clone(), true);
            }
            if !p.is_agent_capable() {
                report.row("agent capable", format!("PROBLEM: endpoint is not agent-capable: {}", p.blocking_issues().join("; ")), false);
            } else {
                report.row("agent capable", "yes", true);
            }
        }
        None if mock => {}
        None => report.row("capability profile", "none cached; run doctor with probe", true),
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_bytes_cuts_on_char_boundaries() {
        assert_eq!(clip_bytes("short", 10), "short");
        assert_eq!(clip_bytes("abcdef", 3), "abc");
        // "é" is two bytes: a cut inside it backs off to before it.
        assert_eq!(clip_bytes("aé", 2), "a");
        assert_eq!(clip_bytes("aé", 3), "aé");
    }

    #[test]
    fn porcelain_z_parsing_handles_renames_and_untracked() {
        let raw = b" M src/a.rs\0A  new.rs\0R  renamed.rs\0old.rs\0?? notes.txt\0MM both.rs\0";
        let files = parse_porcelain_z(raw);
        let view: Vec<(&str, &str, bool)> = files.iter().map(|f| (f.path.as_str(), f.status.as_str(), f.staged)).collect();
        assert_eq!(view, vec![("src/a.rs", "M", false), ("new.rs", "A", true), ("renamed.rs", "R", true), ("notes.txt", "?", false), ("both.rs", "MM", true)]);
    }

    #[test]
    fn numstat_z_parsing_handles_binary_and_renames() {
        let raw = b"3\t1\tsrc/a.rs\0-\t-\timage.png\012\t0\t\0old.rs\0new.rs\0";
        let stats = parse_numstat_z(raw);
        assert_eq!(stats.get("src/a.rs"), Some(&Some((3, 1))));
        assert_eq!(stats.get("image.png"), Some(&None));
        assert_eq!(stats.get("new.rs"), Some(&Some((12, 0))));
        assert_eq!(stats.len(), 3);

        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("t.txt");
        std::fs::write(&text, "a\nb\nc").unwrap();
        assert_eq!(count_text_lines(&text), Some(3));
        std::fs::write(&text, "a\nb\n").unwrap();
        assert_eq!(count_text_lines(&text), Some(2));
        std::fs::write(&text, b"a\0b").unwrap();
        assert_eq!(count_text_lines(&text), None);
        assert_eq!(count_text_lines(&dir.path().join("missing")), None);
    }

    #[test]
    fn error_kinds_follow_message_prefixes() {
        assert_eq!(error_kind_from_message("authentication failed: 401"), "provider_auth");
        assert_eq!(error_kind_from_message("rate limited (retry after 2s)"), "provider_rate_limited");
        assert_eq!(error_kind_from_message("provider request timed out"), "provider_timeout");
        assert_eq!(error_kind_from_message("connection failure: dns"), "provider_connection");
        assert_eq!(error_kind_from_message("context length exceeded: 200k"), "provider_context_length");
        assert_eq!(error_kind_from_message("capability not supported by provider: tools"), "provider_unsupported");
        assert_eq!(error_kind_from_message("provider api error 500: boom"), "provider_api");
        assert_eq!(error_kind_from_message("retries exhausted after 3 attempts: authentication failed: nope"), "provider_auth");
    }

    #[test]
    fn dotted_paths_set_and_remove_nested_values() {
        let mut table = toml::Table::new();
        set_path(&mut table, &["model", "name"], Some(toml::Value::String("m".into())));
        set_path(&mut table, &["model", "capabilities", "streaming"], Some(toml::Value::Boolean(false)));
        assert_eq!(table["model"]["name"].as_str(), Some("m"));
        assert_eq!(table["model"]["capabilities"]["streaming"].as_bool(), Some(false));
        set_path(&mut table, &["model", "name"], None);
        assert!(table["model"].get("name").is_none());
        set_path(&mut table, &["pricing", "input_per_million"], None);
        assert!(table.get("pricing").is_none());
        set_path(&mut table, &["pricing", "output_per_million"], Some(toml::Value::Float(8.0)));
        set_path(&mut table, &["pricing", "output_per_million"], None);
        assert!(table.get("pricing").is_none(), "an emptied table is removed so `pricing` reads back as None");
        set_path(&mut table, &["model", "capabilities", "streaming"], None);
        assert!(table.get("model").is_none(), "emptied tables are pruned all the way up: {table:?}");
        let json = json!({"a": 1, "b": 2.5, "c": [true, "x"], "d": {"e": "f"}});
        let converted = json_to_toml(&json).unwrap();
        assert_eq!(converted["a"].as_integer(), Some(1));
        assert_eq!(converted["b"].as_float(), Some(2.5));
        assert_eq!(converted["c"].as_array().map(|a| a.len()), Some(2));
        assert_eq!(converted["d"]["e"].as_str(), Some("f"));
    }

    #[test]
    fn patch_paths_are_checked_against_the_configuration_shape() {
        let schema = patch_schema().unwrap();
        for ok in [
            &["model", "name"][..],
            &["model", "reasoning_effort"],
            &["model", "temperature"],
            &["model", "capabilities", "streaming"],
            &["model", "extra_headers", "X-Custom-Header"],
            &["pricing", "input_per_million"],
            &["permissions", "extra_workspace_roots"],
            &["embeddings", "model_path"],
            &["validation", "commands"],
            &["index", "exclude"],
            &["agent", "max_steps"],
        ] {
            assert!(check_patch_path(&schema, ok).is_ok(), "{ok:?} should be a known key");
        }
        assert_eq!(check_patch_path(&schema, &["model", "nmae"]).unwrap_err(), "configuration key `model.nmae` is unknown");
        assert_eq!(check_patch_path(&schema, &["index", "foo", "bar"]).unwrap_err(), "configuration key `index.foo` is unknown");
        assert_eq!(check_patch_path(&schema, &["model", "name", "x"]).unwrap_err(), "configuration key `model.name` is not a table");
    }

    #[test]
    fn object_values_are_checked_leaf_by_leaf() {
        let schema = patch_schema().unwrap();
        assert!(check_patch_value(&schema, &["model", "capabilities"], &json!({"streaming": false})).is_ok());
        assert_eq!(check_patch_value(&schema, &["model", "capabilities"], &json!({"streming": false})).unwrap_err(), "configuration key `model.capabilities.streming` is unknown");
        assert_eq!(check_patch_value(&schema, &["model"], &json!({"capabilities": {"streming": false}})).unwrap_err(), "configuration key `model.capabilities.streming` is unknown");
        assert!(check_patch_value(&schema, &["model", "extra_headers"], &json!({"X-A": "1", "Nested": {"deep": 1}})).is_ok(), "children of a free-form table are not checked");
        assert_eq!(check_patch_value(&schema, &["model", "name"], &json!({"x": 1})).unwrap_err(), "configuration key `model.name` is not a table");
        assert!(check_patch_value(&schema, &["model", "name"], &json!("m")).is_ok());
        assert!(check_patch_value(&schema, &["validation", "commands"], &json!(["cargo test"])).is_ok(), "arrays are not tables");
    }

    #[test]
    fn git_error_lines_drop_the_usage_text() {
        assert_eq!(git_error_line(b"warning: Not a git repository. Use --no-index to compare two paths outside a working tree\nusage: git diff --no-index [<options>] <path> <path>\n\nDiff output format options\n"), "warning: Not a git repository. Use --no-index to compare two paths outside a working tree");
        assert_eq!(git_error_line(b"\n  fatal: bad revision 'x'\n"), "fatal: bad revision 'x'");
        assert_eq!(git_error_line(b""), "unknown error");
    }

    #[test]
    fn private_atomic_writes_replace_the_file_and_leave_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");
        write_private_atomic(&path, "a = 1\n").unwrap();
        write_private_atomic(&path, "a = 2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a = 2\n");
        let names: Vec<String> = std::fs::read_dir(path.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["config.toml".to_string()], "no temp file left behind");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    /// A symlinked configuration file (dotfiles) keeps being a symlink: the
    /// write replaces the target, not the link; a dangling link gets its
    /// target created.
    #[cfg(unix)]
    #[test]
    fn private_atomic_writes_follow_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("dotfiles").join("ctx.toml");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, "a = 1\n").unwrap();
        let link = dir.path().join("home").join("config.toml");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_private_atomic(&link, "a = 2\n").unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link survives the write");
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "a = 2\n", "the write reached the dotfiles copy");
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "a = 2\n");
        let names: Vec<String> = std::fs::read_dir(link.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["config.toml".to_string()], "no temp file next to the link");

        let dangling = dir.path().join("home").join("dangling.toml");
        std::os::unix::fs::symlink(Path::new("../elsewhere/new.toml"), &dangling).unwrap();
        write_private_atomic(&dangling, "b = 1\n").unwrap();
        assert!(std::fs::symlink_metadata(&dangling).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(dir.path().join("elsewhere").join("new.toml")).unwrap(), "b = 1\n");
    }

    #[test]
    fn full_lens_pair_positionally_and_last_status_follows_updated_at() {
        let mut lens: HashMap<String, VecDeque<u64>> = HashMap::new();
        lens.entry("call_1".into()).or_default().extend([54, 1482]);
        assert_eq!(take_full_len(&mut lens, "call_1"), Some(54));
        assert_eq!(take_full_len(&mut lens, "call_1"), Some(1482));
        assert_eq!(take_full_len(&mut lens, "call_1"), None);
        assert_eq!(take_full_len(&mut lens, "call_2"), None);

        let store = SessionStore::in_memory().unwrap();
        let now = ctx_core::now();
        let session = SessionRecord { id: "s1".into(), repository_id: "r1".into(), mode: "desktop".into(), model: "m".into(), active_task_id: None, created_at: now, updated_at: now, metadata: json!({}) };
        store.create_session(&session).unwrap();
        let later = now + chrono::Duration::seconds(60);
        let older_but_continued = TaskRecord { id: "t1".into(), session_id: "s1".into(), repository_id: "r1".into(), objective: "Summarize".into(), category: "investigation".into(), state: "BLOCKED".into(), status: Some("blocked".into()), parent_task_id: None, summary: None, state_json: json!({}), created_at: now, updated_at: later, completed_at: None };
        let newer = TaskRecord { id: "t2".into(), session_id: "s1".into(), repository_id: "r1".into(), objective: "Cancel me".into(), category: "investigation".into(), state: "CANCELLED".into(), status: Some("cancelled".into()), parent_task_id: None, summary: None, state_json: json!({}), created_at: now + chrono::Duration::seconds(10), updated_at: now + chrono::Duration::seconds(10), completed_at: None };
        store.create_task(&older_but_continued).unwrap();
        store.create_task(&newer).unwrap();
        let summary = session_summary(&store, &session).unwrap();
        assert_eq!(summary.last_status.as_deref(), Some("blocked"), "the task updated last wins, not the one created last");
    }

    #[test]
    fn session_titles_prefer_metadata_then_first_objective() {
        let store = SessionStore::in_memory().unwrap();
        let now = ctx_core::now();
        let session = SessionRecord { id: "s1".into(), repository_id: "r1".into(), mode: "desktop".into(), model: "m".into(), active_task_id: None, created_at: now, updated_at: now, metadata: json!({}) };
        store.create_session(&session).unwrap();
        assert_eq!(session_summary(&store, &session).unwrap().title, "New chat");
        let task = TaskRecord { id: "t1".into(), session_id: "s1".into(), repository_id: "r1".into(), objective: "Summarize this repository\nsecond line".into(), category: "investigation".into(), state: "COMPLETED".into(), status: Some("completed".into()), parent_task_id: None, summary: None, state_json: json!({}), created_at: now, updated_at: now, completed_at: None };
        store.create_task(&task).unwrap();
        let summary = session_summary(&store, &session).unwrap();
        assert_eq!(summary.title, "Summarize this repository");
        assert_eq!((summary.task_count, summary.last_status.as_deref()), (1, Some("completed")));
        set_session_title(&store, "s1", Some("Renamed")).unwrap();
        let renamed = store.get_session("s1").unwrap().unwrap();
        assert_eq!(session_summary(&store, &renamed).unwrap().title, "Renamed");
        set_session_title(&store, "s1", None).unwrap();
        let cleared = store.get_session("s1").unwrap().unwrap();
        assert_eq!(session_summary(&store, &cleared).unwrap().title, "Summarize this repository");
        delete_session(&store, "s1").unwrap();
        assert!(store.get_session("s1").unwrap().is_none());
        assert!(store.get_task("t1").unwrap().is_none());
    }
}

/// One doctor line for an MCP server: its state, tool count and error.
pub(crate) fn mcp_doctor_line(status: &ctx_tools::McpServerStatus) -> (String, bool) {
    use ctx_tools::McpServerState;
    let target = format!("{} {}", status.transport, status.target);
    match status.state {
        McpServerState::Disabled => (format!("disabled · {target}"), true),
        McpServerState::Connecting => (format!("connecting… · {target}"), true),
        McpServerState::Connected => {
            let server = status.server_name.as_deref().map(|n| format!(" · {n}{}", status.server_version.as_deref().map(|v| format!(" {v}")).unwrap_or_default())).unwrap_or_default();
            (format!("connected · {} tool(s){server} · {target}", status.tools.len()), true)
        }
        McpServerState::Failed => (format!("FAILED: {} · {target}", status.error.as_deref().unwrap_or("unknown error")), false),
    }
}
