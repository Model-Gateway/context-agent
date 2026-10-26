//! Concurrent sessions in one project: what each session has seen of the
//! files it works on, which files it wrote, and which sessions are running.
//!
//! Nothing here locks a file. The file tools use it to refuse an edit built
//! on a read that another session (or anything else) has since overtaken,
//! and the runtime turns it into notes: files that changed under the agent
//! and the other sessions working in the same checkout.

use ctx_core::content_hash;
use parking_lot::Mutex;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// Largest file whose content is kept so a change note can show a diff.
const DIFF_CONTENT_LIMIT: usize = 64 * 1024;
/// Content kept per session for diffs; past it, notes go without one.
const SESSION_CONTENT_BUDGET: usize = 4 * 1024 * 1024;
/// Diff lines a change note shows per file.
const NOTE_DIFF_LINES: usize = 40;
/// Diff lines one change note shows in all.
const NOTE_DIFF_BUDGET: usize = 160;
/// Files listed per session in the other-sessions note.
const NOTE_FILES_PER_SESSION: usize = 8;

/// One project's sessions, shared by every runtime working in its checkout.
#[derive(Default)]
pub struct ProjectActivity {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, SessionEntry>,
    /// path → (session that last wrote it with a file tool, hash it wrote).
    writers: HashMap<String, (String, String)>,
}

#[derive(Default)]
struct SessionEntry {
    label: String,
    parent: Option<String>,
    running: bool,
    /// What the session last saw of each file (read or wrote).
    seen: HashMap<String, Seen>,
    content_bytes: usize,
    /// Files written during the current run: path → hash after, `None`
    /// when the file was deleted. A sub-agent's writes count for its lead.
    run_writes: BTreeMap<String, Option<String>>,
}

#[derive(Clone)]
struct Seen {
    abs: PathBuf,
    hash: String,
    /// Modification time and length when seen, so an untouched file is
    /// never re-hashed.
    stamp: Option<(SystemTime, u64)>,
    content: Option<Arc<str>>,
    /// The changed hash a note already reported.
    noted: Option<String>,
}

/// A file that changed since the session last saw it.
#[derive(Debug, Clone, PartialEq)]
pub struct FileChange {
    pub path: String,
    /// Who changed it, as it reads in a sentence ("session 'Fix login'").
    pub by: Option<String>,
    pub deleted: bool,
    /// Unified diff from what the session saw, when its content was kept.
    pub diff: Option<String>,
}

/// Another session running in the project.
#[derive(Debug, Clone, PartialEq)]
pub struct OtherSession {
    pub label: String,
    /// Files it wrote during its current run, most recent last.
    pub files: Vec<String>,
}

/// How old a modification time must be before its stamp is trusted. NTFS
/// takes file times from the system clock, which advances in ~16 ms ticks
/// (FAT and some network shares are coarser still), so a file rewritten to
/// the same length shortly after it was seen can keep its stamp. A recent
/// stamp is not recorded and the file is hashed instead, like git's "racy
/// clean" entries. Other platforms' file systems keep nanoseconds.
const RACY_STAMP: std::time::Duration = if cfg!(windows) { std::time::Duration::from_secs(2) } else { std::time::Duration::ZERO };

fn stamp(abs: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(abs).ok()?;
    let modified = meta.modified().ok()?;
    if !RACY_STAMP.is_zero() && SystemTime::now().duration_since(modified).map_or(true, |age| age < RACY_STAMP) {
        return None;
    }
    Some((modified, meta.len()))
}

impl ProjectActivity {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The handle a runtime gives its tools for `session_id`.
    pub fn session(self: &Arc<Self>, session_id: &str) -> SessionFiles {
        SessionFiles { activity: self.clone(), session: session_id.to_string() }
    }

    /// Mark `session_id` as running until the guard drops. `parent` is the
    /// lead session of a sub-agent.
    pub fn begin_run(self: &Arc<Self>, session_id: &str, label: &str, parent: Option<&str>) -> RunGuard {
        let mut inner = self.inner.lock();
        let entry = inner.sessions.entry(session_id.to_string()).or_default();
        entry.label = label.to_string();
        entry.parent = parent.map(str::to_string);
        entry.running = true;
        entry.run_writes.clear();
        RunGuard { activity: self.clone(), session: session_id.to_string() }
    }

    /// The files the session's last run wrote (with its sub-agents'),
    /// path → hash after (`None`: deleted).
    pub fn run_writes(&self, session_id: &str) -> BTreeMap<String, Option<String>> {
        self.inner.lock().sessions.get(session_id).map(|e| e.run_writes.clone()).unwrap_or_default()
    }

    /// Drop what the session has seen (its conversation was rewound or
    /// deleted, so the agent no longer knows those reads).
    pub fn forget(&self, session_id: &str) {
        let mut inner = self.inner.lock();
        if let Some(entry) = inner.sessions.get_mut(session_id) {
            entry.seen.clear();
            entry.content_bytes = 0;
            if !entry.running {
                inner.sessions.remove(session_id);
            }
        }
    }

    /// Whether any session other than the given ones is running.
    pub fn others_running(&self, except: &[&str]) -> bool {
        self.inner.lock().sessions.iter().any(|(id, e)| e.running && !except.contains(&id.as_str()))
    }
}

/// Clears a session's running flag however its run ends.
pub struct RunGuard {
    activity: Arc<ProjectActivity>,
    session: String,
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        if let Some(entry) = self.activity.inner.lock().sessions.get_mut(&self.session) {
            entry.running = false;
        }
    }
}

/// One session's view, handed to its tools.
#[derive(Clone)]
pub struct SessionFiles {
    activity: Arc<ProjectActivity>,
    session: String,
}

impl SessionFiles {
    /// A session of its own (tests, one-off tool calls).
    pub fn detached() -> Self {
        ProjectActivity::new().session("detached")
    }

    pub fn session_id(&self) -> &str {
        &self.session
    }

    /// The session saw `bytes` as the content of `rel`.
    pub fn saw(&self, rel: &str, abs: &Path, bytes: &[u8]) {
        let hash = content_hash(bytes);
        let content = (bytes.len() <= DIFF_CONTENT_LIMIT).then(|| std::str::from_utf8(bytes).ok().map(Arc::<str>::from)).flatten();
        let mut inner = self.activity.inner.lock();
        let entry = inner.sessions.entry(self.session.clone()).or_default();
        remember(entry, rel, Seen { abs: abs.to_path_buf(), hash, stamp: stamp(abs), content, noted: None });
    }

    /// The session saw part of `rel` (a retrieved snippet, a search hit):
    /// recorded at the file's current content unless already known.
    pub fn saw_part(&self, rel: &str, abs: &Path) {
        if self.activity.inner.lock().sessions.get(&self.session).is_some_and(|e| e.seen.contains_key(rel)) {
            return;
        }
        if let Ok(bytes) = std::fs::read(abs) {
            self.saw(rel, abs, &bytes);
        }
    }

    /// Whether an edit of `rel`, whose content now hashes to `current`, is
    /// based on what the session last saw; the error explains what to do.
    pub fn check_edit(&self, rel: &str, current: &str) -> std::result::Result<(), String> {
        let inner = self.activity.inner.lock();
        let seen = inner.sessions.get(&self.session).and_then(|e| e.seen.get(rel));
        match seen {
            None => Err(format!("{rel} has not been read in this session. Read it with read_file first so the edit is based on its current content.")),
            Some(s) if s.hash == current => Ok(()),
            Some(_) => {
                let by = attribution(&inner, &self.session, rel, Some(current));
                Err(format!(
                    "{rel} changed since you last read it ({}). Re-read it and rebuild the edit on top of the current content; do not undo changes you did not make.",
                    by.unwrap_or_else(|| "not through your file edits: a command, the user or another program".into())
                ))
            }
        }
    }

    /// The session wrote `rel` (`None`: deleted it).
    pub fn wrote(&self, rel: &str, abs: &Path, content: Option<&str>) {
        let hash = content.map(|c| content_hash(c.as_bytes()));
        let mut inner = self.activity.inner.lock();
        let mut id = Some(self.session.clone());
        // The run's writes count for its lead session too (a rewind there
        // restores what its sub-agents wrote).
        while let Some(current) = id.take() {
            let Some(entry) = inner.sessions.get_mut(&current) else { break };
            entry.run_writes.insert(rel.to_string(), hash.clone());
            id = entry.parent.clone();
        }
        match &hash {
            Some(h) => {
                inner.writers.insert(rel.to_string(), (self.session.clone(), h.clone()));
                let kept = content.filter(|c| c.len() <= DIFF_CONTENT_LIMIT).map(Arc::<str>::from);
                let entry = inner.sessions.entry(self.session.clone()).or_default();
                remember(entry, rel, Seen { abs: abs.to_path_buf(), hash: h.clone(), stamp: stamp(abs), content: kept, noted: None });
            }
            None => {
                inner.writers.remove(rel);
                if let Some(entry) = inner.sessions.get_mut(&self.session) {
                    if let Some(old) = entry.seen.remove(rel) {
                        entry.content_bytes -= old.content.map(|c| c.len()).unwrap_or(0);
                    }
                }
            }
        }
    }

    /// Files that changed since the session last saw them and have not been
    /// reported yet. Each change is reported once; an edit of the file is
    /// still refused until it is read again.
    pub fn changes(&self) -> Vec<FileChange> {
        let seen: Vec<(String, Seen)> = match self.activity.inner.lock().sessions.get(&self.session) {
            Some(e) => e.seen.iter().map(|(p, s)| (p.clone(), s.clone())).collect(),
            None => return Vec::new(),
        };
        // Disk access happens outside the lock.
        let mut found: Vec<(String, Option<String>, Option<(SystemTime, u64)>, Option<String>, Seen)> = Vec::new();
        for (path, s) in seen {
            let now = stamp(&s.abs);
            if now.is_some() && now == s.stamp {
                continue;
            }
            let bytes = std::fs::read(&s.abs).ok();
            let hash = bytes.as_deref().map(content_hash);
            if hash.as_deref() == Some(s.hash.as_str()) {
                found.push((path, hash, now, None, s));
                continue;
            }
            let text = bytes.and_then(|b| String::from_utf8(b).ok());
            found.push((path, hash, now, text, s));
        }
        let mut inner = self.activity.inner.lock();
        let mut changes = Vec::new();
        for (path, hash, now, text, s) in found {
            let unchanged = hash.as_deref() == Some(s.hash.as_str());
            let noted_key = hash.clone().unwrap_or_else(|| "deleted".into());
            let by = if unchanged || s.noted.as_deref() == Some(noted_key.as_str()) { None } else { attribution(&inner, &self.session, &path, hash.as_deref()) };
            let Some(entry) = inner.sessions.get_mut(&self.session) else { break };
            let Some(current) = entry.seen.get_mut(&path) else { continue };
            if current.hash != s.hash {
                // The session wrote or re-read it meanwhile.
                continue;
            }
            if unchanged {
                current.stamp = now;
                continue;
            }
            if current.noted.as_deref() == Some(noted_key.as_str()) {
                continue;
            }
            current.noted = Some(noted_key);
            let diff = match (&s.content, &text) {
                (Some(old), Some(new)) => Some(clip_diff(&crate::registry::unified_diff(&path, old, new))),
                _ => None,
            };
            changes.push(FileChange { path, by, deleted: hash.is_none(), diff });
        }
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        changes
    }

    /// The other sessions running in the project: neither this session nor
    /// its lead or its own sub-agents.
    pub fn others(&self) -> Vec<OtherSession> {
        let inner = self.activity.inner.lock();
        let related = related(&inner, &self.session);
        let mut others: Vec<OtherSession> = inner
            .sessions
            .iter()
            .filter(|(id, e)| e.running && !related.contains(id.as_str()))
            .map(|(_, e)| OtherSession { label: e.label.clone(), files: e.run_writes.keys().cloned().collect() })
            .collect();
        others.sort_by(|a, b| a.label.cmp(&b.label));
        others
    }
}

fn remember(entry: &mut SessionEntry, rel: &str, mut seen: Seen) {
    if let Some(old) = entry.seen.remove(rel) {
        entry.content_bytes -= old.content.map(|c| c.len()).unwrap_or(0);
    }
    if let Some(c) = &seen.content {
        if entry.content_bytes + c.len() > SESSION_CONTENT_BUDGET {
            seen.content = None;
        } else {
            entry.content_bytes += c.len();
        }
    }
    entry.seen.insert(rel.to_string(), seen);
}

/// The session itself, its leads and its sub-agents.
fn related(inner: &Inner, session: &str) -> HashSet<String> {
    inner.sessions.keys().filter(|id| id.as_str() == session || is_ancestor(inner, id, session) || is_ancestor(inner, session, id)).cloned().collect()
}

/// Whether `candidate` is a lead (at any depth) of `of`.
fn is_ancestor(inner: &Inner, candidate: &str, of: &str) -> bool {
    let mut up = inner.sessions.get(of).and_then(|e| e.parent.clone());
    // Bounded: a parent cycle can only come from corrupt metadata.
    for _ in 0..16 {
        let Some(id) = up else { return false };
        if id == candidate {
            return true;
        }
        up = inner.sessions.get(&id).and_then(|e| e.parent.clone());
    }
    false
}

/// Who wrote `rel` at `hash`, unless it was `session` itself.
fn attribution(inner: &Inner, session: &str, rel: &str, hash: Option<&str>) -> Option<String> {
    let (writer, written) = inner.writers.get(rel)?;
    if writer == session || Some(written.as_str()) != hash {
        return None;
    }
    let entry = inner.sessions.get(writer)?;
    let mine = inner.sessions.get(session);
    let who = if mine.and_then(|m| m.parent.as_deref()) == Some(writer.as_str()) {
        "your lead agent".to_string()
    } else if entry.parent.as_deref() == Some(session) {
        format!("your sub-agent '{}'", entry.label)
    } else {
        format!("'{}'", entry.label)
    };
    Some(format!("edited by {who}"))
}

fn clip_diff(diff: &str) -> String {
    let lines: Vec<&str> = diff.lines().collect();
    if lines.len() <= NOTE_DIFF_LINES {
        return diff.trim_end().to_string();
    }
    format!("{}\n… ({} more diff lines)", lines[..NOTE_DIFF_LINES].join("\n"), lines.len() - NOTE_DIFF_LINES)
}

/// The runtime note for files that changed under the agent.
pub fn changes_note(changes: &[FileChange]) -> String {
    let mut text = String::from("Files you read changed since you last saw them:\n");
    let mut diff_lines = 0;
    for c in changes {
        let what = if c.deleted { "deleted" } else { "changed" };
        match &c.by {
            Some(by) => text.push_str(&format!("- {} was {what}, {by}.\n", c.path)),
            None => text.push_str(&format!("- {} was {what} (not by your file edits).\n", c.path)),
        }
        // Diffs up to a budget; past it the paths alone.
        if let Some(diff) = c.diff.as_ref().filter(|_| diff_lines < NOTE_DIFF_BUDGET) {
            diff_lines += diff.lines().count();
            text.push_str(diff);
            text.push('\n');
        }
    }
    text.push_str("Work from the current content: read a file again before editing it, and keep changes you did not make.");
    text
}

/// The task-instruction line for the other sessions running in the checkout.
pub fn others_note(others: &[OtherSession]) -> String {
    let list: Vec<String> = others
        .iter()
        .map(|o| {
            if o.files.is_empty() {
                format!("'{}'", o.label)
            } else {
                let start = o.files.len().saturating_sub(NOTE_FILES_PER_SESSION);
                format!("'{}' (editing {})", o.label, o.files[start..].join(", "))
            }
        })
        .collect();
    format!(
        "Other sessions are working in this checkout at the same time: {}. Stay out of their files where you can; when you must touch one, read it first and keep their changes. Do not run commands that rewrite the whole tree (git add -A, git commit -a, git stash, git checkout ., git reset --hard, git clean, repository-wide formatters); stage and format only the files you changed. ",
        list.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, text: &str) -> PathBuf {
        let abs = dir.join(rel);
        std::fs::write(&abs, text).unwrap();
        abs
    }

    #[test]
    fn edits_need_a_current_read_and_changes_are_reported_once() {
        let dir = tempfile::tempdir().unwrap();
        let activity = ProjectActivity::new();
        let _a = activity.begin_run("a", "Fix login", None);
        let _b = activity.begin_run("b", "Add docs", None);
        let (a, b) = (activity.session("a"), activity.session("b"));
        let abs = write(dir.path(), "lib.rs", "one\n");

        assert!(a.check_edit("lib.rs", &content_hash(b"one\n")).unwrap_err().contains("has not been read"));
        a.saw("lib.rs", &abs, b"one\n");
        b.saw("lib.rs", &abs, b"one\n");
        assert!(a.check_edit("lib.rs", &content_hash(b"one\n")).is_ok());

        // b edits the file: a is told who did it, once, and must re-read.
        std::fs::write(&abs, "one\ntwo\n").unwrap();
        b.wrote("lib.rs", &abs, Some("one\ntwo\n"));
        let err = a.check_edit("lib.rs", &content_hash(b"one\ntwo\n")).unwrap_err();
        assert!(err.contains("edited by 'Add docs'"), "{err}");
        let changes = a.changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].by.as_deref(), Some("edited by 'Add docs'"));
        assert!(changes[0].diff.as_deref().unwrap().contains("+two"));
        assert!(a.changes().is_empty(), "reported once");
        assert!(b.changes().is_empty(), "its own write is not a change");

        assert_eq!(a.others(), vec![OtherSession { label: "Add docs".into(), files: vec!["lib.rs".into()] }]);
        assert!(others_note(&a.others()).contains("'Add docs' (editing lib.rs)"));
        drop(_b);
        assert!(a.others().is_empty());
    }

    #[test]
    fn sub_agent_writes_count_for_the_lead_and_are_not_other_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let activity = ProjectActivity::new();
        let _lead = activity.begin_run("lead", "Refactor", None);
        let _agent = activity.begin_run("agent", "sub-agent impl", Some("lead"));
        let abs = write(dir.path(), "x.rs", "a\n");
        activity.session("lead").saw("x.rs", &abs, b"a\n");
        std::fs::write(&abs, "b\n").unwrap();
        activity.session("agent").wrote("x.rs", &abs, Some("b\n"));
        assert_eq!(activity.run_writes("lead").get("x.rs"), Some(&Some(content_hash(b"b\n"))));
        assert!(activity.session("lead").others().is_empty());
        assert!(activity.session("agent").others().is_empty());
        let changes = activity.session("lead").changes();
        assert_eq!(changes[0].by.as_deref(), Some("edited by your sub-agent 'sub-agent impl'"));
    }
}
