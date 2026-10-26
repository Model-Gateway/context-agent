//! Undo points: a snapshot of the working tree is taken before every run
//! (`task/run`) and recorded against the session, and `session/rewind`
//! removes a user message with everything after it, optionally restoring
//! the files to the snapshot taken before that message.
//!
//! Other sessions may be editing the same checkout, so a restore only puts
//! back the files this session's runs wrote with their file tools (recorded
//! with each snapshot), and of those only the ones nobody changed since:
//! the rest are reported as skipped. Snapshots recorded before that list
//! existed restore the whole tree, as they always did.
//!
//! Snapshots are git tree objects written through a scratch index (seeded
//! from the real one so unchanged files are not re-hashed, unless the user's
//! git converts line endings), with no line-ending conversion, so the user's
//! own index and HEAD are never touched. They live in the repository's
//! object store and are not referenced; `git gc` prunes unreachable objects
//! after `gc.pruneExpire` (two weeks by default), after which a rewind can
//! only keep the files.

use super::*;
use rusqlite::{params, OptionalExtension};
use ctx_core::content_hash;
use std::collections::BTreeMap;

/// `sessions.metadata.snapshots`: `[{ "after_message_id": <highest conversation
/// id of the session when the run began>, "tree": "<oid>" | null, "created_at",
/// "files": { "<path>": "<hash after the run>" | null (deleted) } }]`; `files`
/// is added when the run ends.
const METADATA_KEY: &str = "snapshots";
/// Kept per session; the oldest entries are dropped beyond this.
const MAX_SNAPSHOTS: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Snapshot {
    pub(super) after_message_id: i64,
    pub(super) tree: Option<String>,
    /// The files the run wrote, path → content hash after it (`None`:
    /// deleted); `None` for runs recorded before this list existed.
    pub(super) files: Option<BTreeMap<String, Option<String>>>,
}

impl Snapshot {
    fn to_json(&self) -> Value {
        let mut entry = json!({"after_message_id": self.after_message_id, "tree": self.tree});
        if let Some(files) = &self.files {
            entry["files"] = json!(files);
        }
        entry
    }
}

// ---------------------------------------------------------------------------
// git
// ---------------------------------------------------------------------------

async fn git_with_index(root: &Path, index: &Path, args: &[&str], stdin: Option<Vec<u8>>) -> std::io::Result<std::process::Output> {
    use tokio::io::AsyncWriteExt;
    let mut command = tokio::process::Command::new("git");
    ctx_core::platform::hide_console_window(command.as_std_mut());
    command
        // A snapshot restores the bytes it saw. Git for Windows sets
        // `core.autocrlf=true` system-wide, which would hand an LF file back
        // with CRLF endings; with it off, `add` and `checkout-index` copy
        // files as they are (the default everywhere else).
        .args(["-c", "core.autocrlf=false"])
        .args(args)
        .current_dir(root)
        .env("GIT_INDEX_FILE", index)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .stdin(if stdin.is_some() { std::process::Stdio::piped() } else { std::process::Stdio::null() })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn()?;
    if let Some(bytes) = stdin {
        if let Some(mut pipe) = child.stdin.take() {
            pipe.write_all(&bytes).await?;
            pipe.shutdown().await?;
        }
    }
    child.wait_with_output().await
}

/// The repository's git directory, when `root` is a work tree.
async fn git_dir(root: &Path) -> Option<PathBuf> {
    let out = git(root, &["rev-parse", "--git-dir"]).await.ok()?;
    if !out.status.success() {
        return None;
    }
    let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if dir.is_empty() {
        return None;
    }
    Some(root.join(dir))
}

/// Whether the user's `git add` converts line endings (`core.autocrlf` true
/// or input, Git for Windows' default). The real index then holds converted
/// blobs that its stat cache would carry into a snapshot unchanged, so it
/// cannot seed a snapshot index, which stores files as they are.
async fn converts_line_endings(root: &Path) -> bool {
    match git(root, &["config", "--get", "core.autocrlf"]).await {
        Ok(out) if out.status.success() => !matches!(String::from_utf8_lossy(&out.stdout).trim().to_ascii_lowercase().as_str(), "" | "false" | "no" | "off" | "0"),
        _ => false,
    }
}

/// A scratch index seeded from the real one (stat cache), removed by the guard.
struct ScratchIndex(PathBuf);

impl ScratchIndex {
    fn new(git_dir: &Path, seed: bool) -> Self {
        // Unique per use: runs of several sessions snapshot at the same time.
        static USES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = USES.fetch_add(1, Ordering::Relaxed);
        let path = git_dir.join(format!("ctx-snapshot-{}-{n}.index", std::process::id()));
        let _ = std::fs::remove_file(&path);
        if seed {
            let _ = std::fs::copy(git_dir.join("index"), &path);
        }
        Self(path)
    }
}

impl Drop for ScratchIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The working tree as a git tree object: `git add -A` into a scratch
/// index, then `git write-tree`. Untracked files are included, ignored
/// files are not. `None` when `root` is not a git work tree or git fails;
/// the run then proceeds without an undo point.
pub(super) async fn take_tree(root: &Path) -> Option<String> {
    let dir = git_dir(root).await?;
    let scratch = ScratchIndex::new(&dir, !converts_line_endings(root).await);
    let add = git_with_index(root, &scratch.0, &["add", "-A", "--ignore-errors", "--", "."], None).await.ok()?;
    if !add.status.success() {
        tracing::warn!(root = %root.display(), error = %git_error_line(&add.stderr), "snapshot: git add failed");
        return None;
    }
    let write = git_with_index(root, &scratch.0, &["write-tree"], None).await.ok()?;
    if !write.status.success() {
        tracing::warn!(root = %root.display(), error = %git_error_line(&write.stderr), "snapshot: git write-tree failed");
        return None;
    }
    let tree = String::from_utf8_lossy(&write.stdout).trim().to_string();
    (!tree.is_empty()).then_some(tree)
}

/// Whether `tree` still exists in the object store (`git gc` may have pruned it).
pub(super) async fn tree_exists(root: &Path, tree: &str) -> bool {
    match git(root, &["cat-file", "-t", tree]).await {
        Ok(out) => out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "tree",
        Err(_) => false,
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub(super) struct RestoreReport {
    /// Files written back from the snapshot (added, modified or type-changed since).
    pub(super) restored: usize,
    /// Files created since the snapshot, removed again.
    pub(super) deleted: usize,
    /// Files the session wrote that changed again since (another session,
    /// the user): left as they are.
    pub(super) skipped: Vec<String>,
}

fn nul_paths(bytes: &[u8]) -> Vec<String> {
    bytes.split(|b| *b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).to_string()).collect()
}

/// Bring the working tree back to `tree`: files that differ are written
/// from the snapshot, files that did not exist then are deleted (ignored
/// files are left alone, as is the user's index and HEAD).
pub(super) async fn restore_tree(root: &Path, tree: &str) -> Result<RestoreReport> {
    if !tree_exists(root, tree).await {
        return Err(CtxError::NotFound(format!("snapshot {} is no longer in the repository (pruned by git gc)", &tree[..tree.len().min(12)])));
    }
    let current = take_tree(root).await.ok_or_else(|| CtxError::other("could not snapshot the current working tree"))?;
    if current == tree {
        return Ok(RestoreReport::default());
    }
    let dir = git_dir(root).await.ok_or_else(|| CtxError::other("not a git work tree"))?;
    let changed = git(root, &["diff-tree", "-r", "-z", "--no-renames", "--name-only", "--diff-filter=AMT", &current, tree]).await?;
    if !changed.status.success() {
        return Err(CtxError::other(format!("git diff-tree failed: {}", git_error_line(&changed.stderr))));
    }
    let to_write = nul_paths(&changed.stdout);
    let removed = git(root, &["diff-tree", "-r", "-z", "--no-renames", "--name-only", "--diff-filter=D", &current, tree]).await?;
    if !removed.status.success() {
        return Err(CtxError::other(format!("git diff-tree failed: {}", git_error_line(&removed.stderr))));
    }
    let to_delete = nul_paths(&removed.stdout);

    if !to_write.is_empty() {
        let scratch = ScratchIndex::new(&dir, false);
        let read = git_with_index(root, &scratch.0, &["read-tree", tree], None).await?;
        if !read.status.success() {
            return Err(CtxError::other(format!("git read-tree failed: {}", git_error_line(&read.stderr))));
        }
        let mut stdin = Vec::new();
        for path in &to_write {
            stdin.extend_from_slice(path.as_bytes());
            stdin.push(0);
        }
        let out = git_with_index(root, &scratch.0, &["checkout-index", "-f", "-z", "--stdin"], Some(stdin)).await?;
        if !out.status.success() {
            return Err(CtxError::other(format!("git checkout-index failed: {}", git_error_line(&out.stderr))));
        }
    }
    let mut deleted = 0;
    for path in &to_delete {
        let full = root.join(path);
        match std::fs::remove_file(&full) {
            Ok(()) => deleted += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!(path = %full.display(), error = %e, "rewind: could not delete file"),
        }
        // Directories the snapshot did not have go too, as far up as they are empty.
        let mut parent = full.parent();
        while let Some(dir) = parent {
            if dir == root || std::fs::remove_dir(dir).is_err() {
                break;
            }
            parent = dir.parent();
        }
    }
    Ok(RestoreReport { restored: to_write.len(), deleted, skipped: Vec::new() })
}

/// Bring the session's own files back to `tree`: each of `files` (path →
/// the content hash the session left it at, `None`: it deleted it) that
/// still is as the session left it is written from the snapshot, or removed
/// when the snapshot did not have it. A file changed since by anyone else
/// is skipped and reported.
pub(super) async fn restore_files(root: &Path, tree: &str, files: &BTreeMap<String, Option<String>>) -> Result<RestoreReport> {
    if !tree_exists(root, tree).await {
        return Err(CtxError::NotFound(format!("snapshot {} is no longer in the repository (pruned by git gc)", &tree[..tree.len().min(12)])));
    }
    let mut report = RestoreReport::default();
    let mut ours = Vec::new();
    for (path, left) in files {
        let current = std::fs::read(root.join(path)).ok().map(|b| content_hash(&b));
        if &current == left {
            ours.push(path.clone());
        } else {
            report.skipped.push(path.clone());
        }
    }
    if ours.is_empty() {
        return Ok(report);
    }
    // Which of them the snapshot has.
    let mut args = vec!["ls-tree", "-r", "-z", "--name-only", tree, "--"];
    args.extend(ours.iter().map(String::as_str));
    let listed = git(root, &args).await?;
    if !listed.status.success() {
        return Err(CtxError::other(format!("git ls-tree failed: {}", git_error_line(&listed.stderr))));
    }
    let in_tree: HashSet<String> = nul_paths(&listed.stdout).into_iter().collect();
    let (to_write, to_delete): (Vec<String>, Vec<String>) = ours.into_iter().partition(|p| in_tree.contains(p));
    if !to_write.is_empty() {
        let dir = git_dir(root).await.ok_or_else(|| CtxError::other("not a git work tree"))?;
        let scratch = ScratchIndex::new(&dir, false);
        let read = git_with_index(root, &scratch.0, &["read-tree", tree], None).await?;
        if !read.status.success() {
            return Err(CtxError::other(format!("git read-tree failed: {}", git_error_line(&read.stderr))));
        }
        let mut stdin = Vec::new();
        for path in &to_write {
            stdin.extend_from_slice(path.as_bytes());
            stdin.push(0);
        }
        let out = git_with_index(root, &scratch.0, &["checkout-index", "-f", "-z", "--stdin"], Some(stdin)).await?;
        if !out.status.success() {
            return Err(CtxError::other(format!("git checkout-index failed: {}", git_error_line(&out.stderr))));
        }
        report.restored = to_write.len();
    }
    for path in &to_delete {
        let full = root.join(path);
        match std::fs::remove_file(&full) {
            Ok(()) => report.deleted += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!(path = %full.display(), error = %e, "rewind: could not delete file"),
        }
        let mut parent = full.parent();
        while let Some(dir) = parent {
            if dir == root || std::fs::remove_dir(dir).is_err() {
                break;
            }
            parent = dir.parent();
        }
    }
    Ok(report)
}

// ---------------------------------------------------------------------------
// storage
// ---------------------------------------------------------------------------

pub(super) fn snapshots_from(metadata: &Value) -> Vec<Snapshot> {
    metadata
        .get(METADATA_KEY)
        .and_then(|v| v.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|e| {
                    Some(Snapshot {
                        after_message_id: e.get("after_message_id")?.as_i64()?,
                        tree: e.get("tree").and_then(|t| t.as_str()).map(str::to_string),
                        files: e.get("files").and_then(|f| serde_json::from_value(f.clone()).ok()),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The snapshot taken before the run that produced user message
/// `message_id`: the entry with the highest `after_message_id` below it.
pub(super) fn snapshot_for(snapshots: &[Snapshot], message_id: i64) -> Option<&Snapshot> {
    snapshots.iter().filter(|s| s.after_message_id < message_id).max_by_key(|s| s.after_message_id)
}

/// The highest conversation id of the session (0 when empty).
pub(super) fn last_session_message_id(store: &SessionStore, session_id: &str) -> Result<i64> {
    store.database().with(|c| c.query_row("SELECT COALESCE(MAX(id), 0) FROM conversation WHERE session_id = ?1", params![session_id], |r| r.get(0)))
}

/// The first user message a run inserted (the one its prompt became).
pub(super) fn first_user_message_after(store: &SessionStore, session_id: &str, after: i64) -> Result<Option<i64>> {
    store.database().with(|c| {
        c.query_row(
            "SELECT MIN(id) FROM conversation WHERE session_id = ?1 AND id > ?2 AND json_extract(message, '$.kind') = 'user'",
            params![session_id, after],
            |r| r.get::<_, Option<i64>>(0),
        )
    })
}

/// Append the run's snapshot to the session's metadata. A previous entry
/// with the same `after_message_id` (a run cancelled before it wrote
/// anything) is replaced.
pub(super) fn record(store: &SessionStore, session_id: &str, after_message_id: i64, tree: Option<&str>) -> Result<()> {
    let session = store.get_session(session_id)?.ok_or_else(|| CtxError::InvalidArgument(format!("unknown session {session_id}")))?;
    let mut metadata = match session.metadata {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    let mut entries: Vec<Value> = metadata.get(METADATA_KEY).and_then(|v| v.as_array()).cloned().unwrap_or_default();
    entries.retain(|e| e.get("after_message_id").and_then(|v| v.as_i64()) != Some(after_message_id));
    entries.push(json!({"after_message_id": after_message_id, "tree": tree, "created_at": ctx_core::now().to_rfc3339()}));
    if entries.len() > MAX_SNAPSHOTS {
        entries.drain(..entries.len() - MAX_SNAPSHOTS);
    }
    metadata.insert(METADATA_KEY.into(), Value::Array(entries));
    let metadata = Value::Object(metadata).to_string();
    store.database().with(|c| {
        c.execute("UPDATE sessions SET metadata = ?1 WHERE id = ?2", params![metadata, session_id])?;
        Ok(())
    })
}

/// Add the files a run wrote to its snapshot entry (`after_message_id`).
pub(super) fn record_files(store: &SessionStore, session_id: &str, after_message_id: i64, files: &BTreeMap<String, Option<String>>) -> Result<()> {
    let Some(session) = store.get_session(session_id)? else { return Ok(()) };
    let mut metadata = match session.metadata {
        Value::Object(map) => map,
        _ => return Ok(()),
    };
    let Some(entries) = metadata.get_mut(METADATA_KEY).and_then(|v| v.as_array_mut()) else { return Ok(()) };
    let Some(entry) = entries.iter_mut().rev().find(|e| e.get("after_message_id").and_then(|v| v.as_i64()) == Some(after_message_id)) else { return Ok(()) };
    entry["files"] = json!(files);
    let metadata = Value::Object(metadata).to_string();
    store.database().with(|c| {
        c.execute("UPDATE sessions SET metadata = ?1 WHERE id = ?2", params![metadata, session_id])?;
        Ok(())
    })
}

/// What a restore for the snapshot `matched` puts back: the files the
/// session's runs since then wrote, path → what they left it at (the latest
/// run's word). `None` when one of those runs predates the list: restore
/// the whole tree.
pub(super) fn files_since(snapshots: &[Snapshot], matched: &Snapshot) -> Option<BTreeMap<String, Option<String>>> {
    let mut runs: Vec<&Snapshot> = snapshots.iter().filter(|s| s.after_message_id >= matched.after_message_id).collect();
    runs.sort_by_key(|s| s.after_message_id);
    let mut files = BTreeMap::new();
    for run in runs {
        files.extend(run.files.clone()?);
    }
    Some(files)
}

/// `session/rewind`'s result.
#[derive(Serialize)]
pub(super) struct RewindResult {
    pub(super) removed_tasks: usize,
    pub(super) removed_messages: usize,
    /// Present when files were restored.
    pub(super) files: Option<RestoreReport>,
    pub(super) session: SessionSummary,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct RewindReport {
    pub(super) removed_tasks: usize,
    pub(super) removed_messages: usize,
    pub(super) snapshot: Option<Snapshot>,
}

/// What `session/rewind` will do, without doing it: the snapshot for the
/// message (so a restore can be checked before anything is deleted) and
/// the files a restore would put back ([`files_since`]).
pub(super) fn plan(store: &SessionStore, session_id: &str, message_id: i64) -> Result<Option<(Snapshot, Option<BTreeMap<String, Option<String>>>)>> {
    let session = store.get_session(session_id)?.ok_or_else(|| CtxError::InvalidArgument(format!("unknown session {session_id}")))?;
    locate_user_message(store, session_id, message_id)?;
    let snapshots = snapshots_from(&session.metadata);
    Ok(snapshot_for(&snapshots, message_id).map(|s| (s.clone(), files_since(&snapshots, s))))
}

/// (task id, created_at) of a user message of the session.
fn locate_user_message(store: &SessionStore, session_id: &str, message_id: i64) -> Result<(String, String)> {
    let row: Option<(Option<String>, Option<String>, String)> = store.database().with(|c| {
        c.query_row(
            "SELECT task_id, json_extract(message, '$.kind'), created_at FROM conversation WHERE id = ?1 AND session_id = ?2",
            params![message_id, session_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
    })?;
    match row {
        None => Err(CtxError::InvalidArgument(format!("unknown message {message_id} in session {session_id}"))),
        Some((_, kind, _)) if kind.as_deref() != Some("user") => Err(CtxError::InvalidArgument(format!("message {message_id} is not a user message"))),
        Some((None, _, _)) => Err(CtxError::InvalidArgument(format!("message {message_id} belongs to no task"))),
        Some((Some(task), _, created_at)) => Ok((task, created_at)),
    }
}

/// Remove user message `message_id` and everything the session recorded
/// after it, in one transaction: later tasks entirely (with their sub-agent
/// sessions), and the message's own task either entirely (when the message
/// started it) or from that message on, with its status put back to the
/// last `task_completed` it still has. The session's active task and its
/// snapshot list follow.
pub(super) fn rewind(store: &SessionStore, session_id: &str, message_id: i64) -> Result<RewindReport> {
    let session = store.get_session(session_id)?.ok_or_else(|| CtxError::InvalidArgument(format!("unknown session {session_id}")))?;
    let (task_id, created_at) = locate_user_message(store, session_id, message_id)?;
    let tasks = store.tasks_for_session(session_id)?;
    let position = tasks.iter().position(|t| t.id == task_id).ok_or_else(|| CtxError::InvalidArgument(format!("task {task_id} is not part of session {session_id}")))?;
    let first_id: i64 = store.database().with(|c| c.query_row("SELECT COALESCE(MIN(id), 0) FROM conversation WHERE task_id = ?1", params![&task_id], |r| r.get(0)))?;
    let starts_task = first_id == message_id;

    let snapshots = snapshots_from(&session.metadata);
    let matched = snapshot_for(&snapshots, message_id).cloned();
    let kept: Vec<Value> = snapshots
        .iter()
        .filter(|s| match &matched {
            Some(m) => s.after_message_id < m.after_message_id,
            None => s.after_message_id < message_id,
        })
        .map(Snapshot::to_json)
        .collect();
    let mut metadata = match session.metadata.clone() {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    metadata.insert(METADATA_KEY.into(), Value::Array(kept));
    let metadata = Value::Object(metadata).to_string();

    let whole: Vec<&TaskRecord> = tasks.iter().skip(if starts_task { position } else { position + 1 }).collect();
    let remaining_active: Option<String> = if starts_task { tasks[..position].last().map(|t| t.id.clone()) } else { Some(task_id.clone()) };
    let now = ctx_core::now().to_rfc3339();

    let removed_messages = store.database().with_tx(|tx| {
        let mut removed_messages = 0usize;
        for task in &whole {
            removed_messages += tx.query_row("SELECT COUNT(*) FROM conversation WHERE task_id = ?1", params![&task.id], |r| r.get::<_, i64>(0))? as usize;
            for child in child_sessions_of_task(tx, &task.id, None)? {
                delete_session_rows(tx, &child)?;
            }
            delete_task_rows(tx, &task.id)?;
        }
        if !starts_task {
            removed_messages += tx.execute("DELETE FROM conversation WHERE task_id = ?1 AND id >= ?2", params![&task_id, message_id])?;
            for child in child_sessions_of_task(tx, &task_id, Some(&created_at))? {
                delete_session_rows(tx, &child)?;
            }
            tx.execute("DELETE FROM events WHERE task_id = ?1 AND created_at >= ?2", params![&task_id, &created_at])?;
            tx.execute("DELETE FROM model_requests WHERE task_id = ?1 AND created_at >= ?2", params![&task_id, &created_at])?;
            tx.execute("DELETE FROM validation_runs WHERE task_id = ?1 AND created_at >= ?2", params![&task_id, &created_at])?;
            tx.execute("DELETE FROM metrics WHERE task_id = ?1 AND created_at >= ?2", params![&task_id, &created_at])?;
            tx.execute("DELETE FROM checkpoints WHERE task_id = ?1 AND created_at >= ?2", params![&task_id, &created_at])?;
            // The task's status as of its last remaining completion.
            let last: Option<(String, String)> = tx
                .query_row("SELECT payload, created_at FROM events WHERE task_id = ?1 AND kind = 'task_completed' ORDER BY id DESC LIMIT 1", params![&task_id], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
            let outcome = last.as_ref().and_then(|(payload, _)| serde_json::from_str::<AgentEvent>(payload).ok()).and_then(|e| match e {
                AgentEvent::TaskCompleted { outcome, .. } => Some(outcome),
                _ => None,
            });
            match (outcome, last) {
                (Some(outcome), Some((_, completed_at))) => {
                    let state = match outcome.status {
                        TaskStatus::Completed => "COMPLETED",
                        TaskStatus::Blocked => "BLOCKED",
                        TaskStatus::Failed => "FAILED",
                        TaskStatus::Cancelled => "CANCELLED",
                    };
                    tx.execute(
                        "UPDATE tasks SET state = ?1, status = ?2, summary = ?3, completed_at = ?4, updated_at = ?5 WHERE id = ?6",
                        params![state, outcome.status.as_str(), outcome.summary, completed_at, &now, &task_id],
                    )?;
                }
                _ => {
                    tx.execute("UPDATE tasks SET state = 'IDLE', status = NULL, summary = NULL, completed_at = NULL, updated_at = ?1 WHERE id = ?2", params![&now, &task_id])?;
                }
            }
        }
        tx.execute("UPDATE sessions SET active_task_id = ?1, metadata = ?2, updated_at = ?3 WHERE id = ?4", params![&remaining_active, &metadata, &now, session_id])?;
        Ok(removed_messages)
    })?;
    Ok(RewindReport { removed_tasks: whole.len(), removed_messages, snapshot: matched })
}

/// Sub-agent sessions of a task (`metadata.parent_task`), optionally only
/// those created at or after `since`.
fn child_sessions_of_task(c: &rusqlite::Connection, task_id: &str, since: Option<&str>) -> rusqlite::Result<Vec<String>> {
    let mut stmt = c.prepare("SELECT id FROM sessions WHERE json_extract(metadata, '$.parent_task') = ?1 AND (?2 IS NULL OR created_at >= ?2)")?;
    let rows = stmt.query_map(params![task_id, since], |r| r.get::<_, String>(0))?;
    rows.collect()
}

/// Everything that belongs to one task. Episodic memories are kept (they
/// are repository knowledge) but lose their task link, as do tasks that
/// pointed at this one as their parent.
pub(super) fn delete_task_rows(c: &rusqlite::Connection, task_id: &str) -> rusqlite::Result<()> {
    c.execute("DELETE FROM metrics WHERE task_id = ?1", params![task_id])?;
    c.execute("DELETE FROM validation_runs WHERE task_id = ?1", params![task_id])?;
    c.execute("DELETE FROM model_requests WHERE task_id = ?1", params![task_id])?;
    c.execute("DELETE FROM checkpoints WHERE task_id = ?1", params![task_id])?;
    c.execute("DELETE FROM events WHERE task_id = ?1", params![task_id])?;
    c.execute("DELETE FROM conversation WHERE task_id = ?1", params![task_id])?;
    c.execute("UPDATE memories SET task_id = NULL WHERE task_id = ?1", params![task_id])?;
    c.execute("UPDATE tasks SET parent_task_id = NULL WHERE parent_task_id = ?1", params![task_id])?;
    c.execute("DELETE FROM tasks WHERE id = ?1", params![task_id])?;
    Ok(())
}

/// A session and everything that belongs to it (`session/delete`). No table
/// declares foreign keys, so nothing cascades on its own.
pub(super) fn delete_session_rows(c: &rusqlite::Connection, session_id: &str) -> rusqlite::Result<()> {
    const TASKS: &str = "(SELECT id FROM tasks WHERE session_id = ?1)";
    c.execute(&format!("DELETE FROM metrics WHERE task_id IN {TASKS}"), params![session_id])?;
    c.execute(&format!("DELETE FROM validation_runs WHERE task_id IN {TASKS}"), params![session_id])?;
    c.execute(&format!("DELETE FROM model_requests WHERE session_id = ?1 OR task_id IN {TASKS}"), params![session_id])?;
    c.execute(&format!("DELETE FROM checkpoints WHERE session_id = ?1 OR task_id IN {TASKS}"), params![session_id])?;
    c.execute(&format!("DELETE FROM events WHERE session_id = ?1 OR task_id IN {TASKS}"), params![session_id])?;
    c.execute("DELETE FROM conversation WHERE session_id = ?1", params![session_id])?;
    c.execute(&format!("UPDATE memories SET task_id = NULL WHERE task_id IN {TASKS}"), params![session_id])?;
    c.execute(&format!("UPDATE tasks SET parent_task_id = NULL WHERE parent_task_id IN {TASKS}"), params![session_id])?;
    c.execute("DELETE FROM tasks WHERE session_id = ?1", params![session_id])?;
    c.execute("DELETE FROM sessions WHERE id = ?1", params![session_id])?;
    Ok(())
}

/// `git init` is only ever run for a plain project folder: never for the
/// root, a home directory or its parent, where a repository would swallow
/// everything the user owns.
pub(super) fn git_init_allowed(root: &Path) -> bool {
    let Some(parent) = root.parent() else { return false };
    if let Some(home) = ctx_core::platform::home_dir() {
        let home = ctx_core::platform::canonicalize(&home).unwrap_or(home);
        if root == home || Some(root) == home.parent() {
            return false;
        }
    }
    if parent.parent().is_some() {
        return true;
    }
    // A top-level directory. On Unix that is never a project (`/tmp`,
    // `/Volumes`, `/Users`); on Windows projects often sit right under a
    // drive (`C:\src`, `D:\work`), so only the system folders are refused.
    cfg!(windows) && !is_windows_system_folder(root)
}

/// `C:\Users`, `C:\Windows`, `C:\Program Files`… directly under a drive.
fn is_windows_system_folder(root: &Path) -> bool {
    let name = root.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    matches!(name.as_str(), "" | "users" | "windows" | "program files" | "program files (x86)" | "programdata" | "$recycle.bin" | "system volume information" | "recovery" | "perflogs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_lookup_picks_the_run_before_the_message() {
        let list = vec![
            Snapshot { after_message_id: 0, tree: Some("a".into()), files: None },
            Snapshot { after_message_id: 5, tree: None, files: None },
            Snapshot { after_message_id: 9, tree: Some("c".into()), files: None },
        ];
        assert_eq!(snapshot_for(&list, 1).unwrap().tree.as_deref(), Some("a"));
        assert_eq!(snapshot_for(&list, 6).unwrap().tree, None);
        assert_eq!(snapshot_for(&list, 10).unwrap().tree.as_deref(), Some("c"));
        assert_eq!(snapshot_for(&list, 0), None);
    }

    #[test]
    fn git_init_is_refused_for_the_root_and_home() {
        assert!(!git_init_allowed(Path::new("/")));
        if let Some(home) = ctx_core::platform::home_dir() {
            let home = ctx_core::platform::canonicalize(&home).unwrap_or(home);
            assert!(!git_init_allowed(&home));
            assert!(!git_init_allowed(home.parent().unwrap()));
            assert!(git_init_allowed(&home.join("Projects").join("demo")));
        }
        assert!(git_init_allowed(Path::new("/tmp/some/project")));
        #[cfg(windows)]
        {
            assert!(git_init_allowed(Path::new(r"D:\work")));
            assert!(!git_init_allowed(Path::new(r"C:\Users")));
            assert!(!git_init_allowed(Path::new(r"C:\Program Files")));
            assert!(!git_init_allowed(Path::new(r"C:\")));
        }
    }

    #[tokio::test]
    async fn snapshots_round_trip_through_the_working_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git").args(args).current_dir(root).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
            assert!(status.success(), "git {args:?}");
        };
        run(&["init", "-q"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/b.txt"), "two\n").unwrap();
        std::fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(root.join("ignored.txt"), "keep me\n").unwrap();
        let before = take_tree(root).await.expect("snapshot");
        assert!(tree_exists(root, &before).await);

        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        std::fs::remove_file(root.join("sub/b.txt")).unwrap();
        std::fs::create_dir_all(root.join("new/deep")).unwrap();
        std::fs::write(root.join("new/deep/c.txt"), "three\n").unwrap();
        std::fs::write(root.join("ignored.txt"), "still ignored\n").unwrap();
        let after = take_tree(root).await.unwrap();
        assert_ne!(before, after);

        let report = restore_tree(root, &before).await.unwrap();
        assert_eq!(report, RestoreReport { restored: 2, deleted: 1, skipped: vec![] });
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\n");
        assert_eq!(std::fs::read_to_string(root.join("sub/b.txt")).unwrap(), "two\n");
        assert!(!root.join("new").exists(), "empty directories created since are removed");
        assert_eq!(std::fs::read_to_string(root.join("ignored.txt")).unwrap(), "still ignored\n", "ignored files are untouched");
        assert_eq!(take_tree(root).await.unwrap(), before);
        let scratch = std::fs::read_dir(root.join(".git")).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().starts_with("ctx-snapshot-")).count();
        assert_eq!(scratch, 0, "the scratch index is removed");

        assert!(!tree_exists(root, "0000000000000000000000000000000000000000").await);
        assert!(take_tree(tempfile::tempdir().unwrap().path()).await.is_none(), "not a repository");
    }

    /// With `core.autocrlf=true` (Git for Windows' default) a restore puts
    /// back the exact bytes: an LF file stays LF and a committed CRLF file,
    /// unchanged since `git add`, stays CRLF.
    #[tokio::test]
    async fn snapshots_keep_line_endings_under_autocrlf() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git").args(args).current_dir(root).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
            assert!(status.success(), "git {args:?}");
        };
        run(&["init", "-q"]);
        run(&["config", "core.autocrlf", "true"]);
        std::fs::write(root.join("crlf.txt"), "one\r\ntwo\r\n").unwrap();
        // Older than the index, so git trusts its stat cache for it.
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        std::fs::File::options().write(true).open(root.join("crlf.txt")).unwrap().set_modified(past).unwrap();
        run(&["add", "crlf.txt"]);
        run(&["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "-m", "init"]);
        std::fs::write(root.join("lf.txt"), "one\ntwo\n").unwrap();
        let before = take_tree(root).await.expect("snapshot");

        std::fs::write(root.join("crlf.txt"), "changed\r\n").unwrap();
        std::fs::write(root.join("lf.txt"), "changed\n").unwrap();
        restore_tree(root, &before).await.unwrap();
        assert_eq!(std::fs::read(root.join("crlf.txt")).unwrap(), b"one\r\ntwo\r\n");
        assert_eq!(std::fs::read(root.join("lf.txt")).unwrap(), b"one\ntwo\n");
    }

    /// Only the files the session wrote go back, and only those still as it
    /// left them; whatever another session changed stays.
    #[tokio::test]
    async fn a_session_restore_leaves_other_sessions_work_alone() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let status = std::process::Command::new("git").args(["init", "-q"]).current_dir(root).status().unwrap();
        assert!(status.success());
        for (name, text) in [("a.txt", "a0\n"), ("b.txt", "b0\n"), ("c.txt", "c0\n"), ("d.txt", "d0\n")] {
            std::fs::write(root.join(name), text).unwrap();
        }
        let before = take_tree(root).await.expect("snapshot");

        // This session edits a, deletes b, creates new and edits d...
        std::fs::write(root.join("a.txt"), "a1\n").unwrap();
        std::fs::remove_file(root.join("b.txt")).unwrap();
        std::fs::create_dir_all(root.join("gen")).unwrap();
        std::fs::write(root.join("gen/new.txt"), "new\n").unwrap();
        std::fs::write(root.join("d.txt"), "d1\n").unwrap();
        let files: BTreeMap<String, Option<String>> = [
            ("a.txt", Some(content_hash(b"a1\n"))),
            ("b.txt", None),
            ("gen/new.txt", Some(content_hash(b"new\n"))),
            ("d.txt", Some(content_hash(b"d1\n"))),
        ]
        .into_iter()
        .map(|(p, h)| (p.to_string(), h))
        .collect();
        // ...while another one edits c and then d on top of it.
        std::fs::write(root.join("c.txt"), "c2\n").unwrap();
        std::fs::write(root.join("d.txt"), "d2\n").unwrap();

        let report = restore_files(root, &before, &files).await.unwrap();
        assert_eq!(report, RestoreReport { restored: 2, deleted: 1, skipped: vec!["d.txt".into()] });
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "a0\n");
        assert_eq!(std::fs::read_to_string(root.join("b.txt")).unwrap(), "b0\n");
        assert!(!root.join("gen").exists());
        assert_eq!(std::fs::read_to_string(root.join("c.txt")).unwrap(), "c2\n", "not the session's file");
        assert_eq!(std::fs::read_to_string(root.join("d.txt")).unwrap(), "d2\n", "changed since the session wrote it");

        let snapshots = vec![
            Snapshot { after_message_id: 1, tree: Some(before.clone()), files: Some(files.clone()) },
            Snapshot { after_message_id: 7, tree: None, files: Some([("a.txt".to_string(), None)].into_iter().collect()) },
        ];
        let union = files_since(&snapshots, &snapshots[0]).unwrap();
        assert_eq!(union.get("a.txt"), Some(&None), "the later run's word wins");
        assert_eq!(union.len(), 4);
        let legacy = vec![Snapshot { after_message_id: 1, tree: Some(before), files: None }];
        assert!(files_since(&legacy, &legacy[0]).is_none(), "an old snapshot restores the whole tree");
    }
}
