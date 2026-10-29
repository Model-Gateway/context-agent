//! File tools: read_file, list_files, apply_patch, write_file (spec §10.4).

use crate::registry::{unified_diff, Tool, ToolContext, ToolEffect, ToolOutput};
use async_trait::async_trait;
use ctx_core::{content_hash, CtxError, IndexConfig, Result, SourceRef};
use ctx_indexing::Walker;
use ctx_languages::LanguageRegistry;
use ctx_security::{Action, ActionKind};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn short_hash(hash: &str) -> &str {
    let h = hash.trim_start_matches("sha256:");
    &h[..h.len().min(12)]
}

/// Write atomically: temp file in the same directory, then rename.
pub fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Unique per write: two sessions of one process may write the same file.
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("ctx-tmp-{}-{n}", std::process::id()));
    std::fs::write(&tmp, content)?;
    #[cfg(unix)]
    if let Ok(meta) = std::fs::metadata(path) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(meta.permissions().mode()));
    }
    std::fs::rename(&tmp, path)
}

fn after_write(ctx: &ToolContext, abs: &Path, rel: &str, content: &str, new_hash: &str) {
    ctx.files.wrote(rel, abs, Some(content));
    if let Some(indexer) = &ctx.indexer {
        if let Err(e) = indexer.update_paths(&[abs.to_path_buf()]) {
            tracing::warn!(error = %e, path = rel, "incremental index update failed after edit");
        }
    }
    if let Some(memory) = &ctx.memory {
        if let Err(e) = memory.invalidate_for_path(rel, Some(new_hash), "file edited during the task") {
            tracing::warn!(error = %e, "memory invalidation failed");
        }
    }
    ctx.working_set.write().retain(|p| p != rel);
    ctx.working_set.write().push(rel.to_string());
}

fn resolve(ctx: &ToolContext, path: &str) -> Result<(PathBuf, String)> {
    let abs = ctx.policy.resolve_path(path)?;
    let rel = ctx.relative(&abs);
    Ok((abs, rel))
}

fn suggest_paths(ctx: &ToolContext, path: &str) -> String {
    let name = Path::new(path).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    let Some(indexer) = &ctx.indexer else { return String::new() };
    let stem = name.split('.').next().unwrap_or(&name);
    match indexer.store().search_paths(stem, 5) {
        Ok(files) if !files.is_empty() => format!(" Similar indexed paths: {}", files.iter().map(|f| f.path.clone()).collect::<Vec<_>>().join(", ")),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------

pub struct ReadFile;

#[derive(Deserialize)]
struct ReadArgs {
    path: String,
    start_line: Option<usize>,
    end_line: Option<usize>,
    max_lines: Option<usize>,
}

#[async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read a file (or a line range) from the repository with line numbers. Returns the file hash so later edits can be verified. Defaults to the first 400 lines; pass start_line/end_line to read more."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1, "description": "Repository-relative path"},
                "start_line": {"type": "integer", "minimum": 1, "description": "First line to show (1-based)"},
                "end_line": {"type": "integer", "minimum": 1, "description": "Last line to show (inclusive)"},
                "max_lines": {"type": "integer", "minimum": 1, "maximum": 4000, "description": "Cap on lines returned (default 400)"}
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    fn action(&self, args: &Value, _ctx: &ToolContext) -> Result<Action> {
        let path = args["path"].as_str().unwrap_or_default().to_string();
        Ok(Action { tool: "read_file".into(), kind: ActionKind::ReadFile { path: path.clone() }, summary: format!("read {path}"), details: String::new() })
    }

    async fn invoke(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let a: ReadArgs = serde_json::from_value(args)?;
        let (abs, rel) = resolve(ctx, &a.path)?;
        if !abs.exists() {
            return Ok(ToolOutput::error(format!("{} does not exist.{}", a.path, suggest_paths(ctx, &a.path))));
        }
        if abs.is_dir() {
            return Ok(ToolOutput::error(format!("{} is a directory; use list_files", a.path)));
        }
        let bytes = std::fs::read(&abs)?;
        if bytes.iter().take(8192).any(|b| *b == 0) {
            return Ok(ToolOutput::error(format!("{} appears to be binary ({} bytes)", rel, bytes.len())));
        }
        let text = String::from_utf8_lossy(&bytes).to_string();
        let hash = content_hash(&bytes);
        ctx.files.saw(&rel, &abs, &bytes);
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let total = lines.len();
        let max_lines = a.max_lines.unwrap_or(400).min(4000);
        let start = a.start_line.unwrap_or(1).max(1);
        let end = a.end_line.unwrap_or(start + max_lines - 1).min(total.max(1));
        if start > total && total > 0 {
            return Ok(ToolOutput::error(format!("{rel} has only {total} lines; start_line {start} is past the end")));
        }
        if let Some(requested_end) = a.end_line {
            if requested_end < start {
                return Ok(ToolOutput::error(format!("end_line ({requested_end}) is before start_line ({start}); pass start_line <= end_line")));
            }
        }
        let end = end.min(start + max_lines - 1);
        let mut out = String::new();
        out.push_str(&format!("# {rel} — {total} lines, hash sha256:{}", short_hash(&hash)));
        if total > 0 {
            out.push_str(&format!(", showing lines {start}-{end}"));
        }
        out.push('\n');
        let width = total.to_string().len().max(3);
        let mut start_byte = 0usize;
        let mut end_byte = 0usize;
        let mut offset = 0usize;
        for (i, line) in lines.iter().enumerate() {
            let n = i + 1;
            if n == start {
                start_byte = offset;
            }
            if n >= start && n <= end {
                out.push_str(&format!("{:>width$} | {}", n, line.trim_end_matches('\n')));
                out.push('\n');
                end_byte = offset + line.len();
            }
            offset += line.len();
        }
        if end < total {
            out.push_str(&format!("… {} more line(s); call read_file with start_line={} to continue.\n", total - end, end + 1));
        }
        // An empty file (or empty range) leaves end_byte at 0; never slice a
        // reversed range.
        let end_byte = end_byte.max(start_byte).min(text.len());
        let start_byte = start_byte.min(end_byte);
        let range_hash = content_hash(&text.as_bytes()[start_byte..end_byte]);
        let source = SourceRef { path: rel.clone(), start_byte, end_byte, start_line: start, end_line: end, content_hash: range_hash, file_hash: hash };
        ctx.working_set.write().retain(|p| p != &rel);
        ctx.working_set.write().push(rel.clone());
        Ok(ToolOutput::ok(out, format!("read {rel} lines {start}-{end} of {total}")).with_effect(ToolEffect::FileRead { path: rel }).with_effect(ToolEffect::SourceRead { source }))
    }
}

// ---------------------------------------------------------------------------

pub struct ListFiles;

#[derive(Deserialize)]
struct ListArgs {
    pattern: Option<String>,
    directory: Option<String>,
    limit: Option<usize>,
}

pub fn walk_files(ctx: &ToolContext) -> Vec<ctx_indexing::WalkedFile> {
    match &ctx.indexer {
        Some(ix) => ix.walker().walk(ix.registry()),
        None => match Walker::new(&ctx.root, &IndexConfig::default()) {
            Ok(w) => w.walk(&LanguageRegistry::with_defaults()),
            Err(_) => Vec::new(),
        },
    }
}

#[async_trait]
impl Tool for ListFiles {
    fn name(&self) -> &str {
        "list_files"
    }

    fn description(&self) -> &str {
        "List repository files (respecting .gitignore and exclusions), optionally filtered by a directory and/or a glob pattern such as `*.test.ts` or `src/**/*.py`."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Glob matched against the relative path; a bare file pattern matches in any directory"},
                "directory": {"type": "string", "description": "Restrict to this directory (relative)"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 2000, "description": "Maximum entries (default 200)"}
            },
            "additionalProperties": false
        })
    }

    fn action(&self, _args: &Value, _ctx: &ToolContext) -> Result<Action> {
        Ok(Action { tool: "list_files".into(), kind: ActionKind::ListFiles, summary: "list files".into(), details: String::new() })
    }

    async fn invoke(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let a: ListArgs = serde_json::from_value(args)?;
        let limit = a.limit.unwrap_or(200);
        let dir = a.directory.as_deref().and_then(normalize_prefix);
        let glob = match &a.pattern {
            Some(p) => {
                // globset reads `\\` as an escape; the walker's paths use `/`.
                let p = p.replace('\\', "/");
                let p = if p.contains('/') { p } else { format!("**/{p}") };
                let mut b = globset::GlobSetBuilder::new();
                b.add(globset::Glob::new(&p).map_err(|e| CtxError::invalid(format!("invalid glob: {e}")))?);
                if !p.starts_with("**/") {
                    b.add(globset::Glob::new(&format!("**/{p}")).map_err(|e| CtxError::invalid(format!("invalid glob: {e}")))?);
                }
                Some(b.build().map_err(|e| CtxError::invalid(e.to_string()))?)
            }
            None => None,
        };
        let files = walk_files(ctx);
        let mut matched: Vec<&ctx_indexing::WalkedFile> = files
            .iter()
            .filter(|f| dir.as_ref().map(|d| under_prefix(&f.relative, d)).unwrap_or(true))
            .filter(|f| glob.as_ref().map(|g| g.is_match(&f.relative)).unwrap_or(true))
            .collect();
        let total = matched.len();
        matched.truncate(limit);
        let mut out = format!("{total} file(s){}{}\n", dir.as_ref().map(|d| format!(" under {d}")).unwrap_or_default(), a.pattern.as_ref().map(|p| format!(" matching {p}")).unwrap_or_default());
        for f in &matched {
            out.push_str(&format!("{}\t{} bytes\t{}\n", f.relative, f.size, f.language.clone().unwrap_or_default()));
        }
        if total > limit {
            out.push_str(&format!("… {} more; narrow with directory/pattern or raise limit.\n", total - limit));
        }
        Ok(ToolOutput::ok(out, format!("listed {} of {total} files", matched.len())))
    }
}

// ---------------------------------------------------------------------------

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Edit {
    pub old_text: String,
    pub new_text: String,
    /// When `old_text` occurs more than once: change the occurrence
    /// nearest this line.
    #[serde(default)]
    pub near_line: Option<usize>,
    /// Change every occurrence of `old_text`.
    #[serde(default)]
    pub replace_all: bool,
}

#[derive(Deserialize)]
struct PatchArgs {
    path: String,
    edits: Vec<Edit>,
    expected_file_hash: Option<String>,
    #[serde(default)]
    create_if_missing: bool,
}

/// Apply edits to `content`; returns the new content or a descriptive error.
pub fn apply_edits(content: &str, edits: &[Edit]) -> std::result::Result<String, String> {
    let mut current = content.to_string();
    for (i, edit) in edits.iter().enumerate() {
        if edit.old_text.is_empty() {
            if current.is_empty() {
                current = edit.new_text.clone();
                continue;
            }
            return Err(format!("edit {} has empty old_text but the file is not empty; provide the exact text to replace", i + 1));
        }
        if edit.replace_all && edit.near_line.is_some() {
            return Err(format!("edit {}: set either near_line or replace_all, not both", i + 1));
        }
        let (old_text, new_text) = line_ending_adapted(&current, &edit.old_text, &edit.new_text);
        let starts: Vec<usize> = current.match_indices(old_text.as_ref()).map(|(at, _)| at).collect();
        match starts.len() {
            0 => {
                return Err(format!("edit {}: old_text was not found in the current file.{}", i + 1, nearest_hint(&current, &edit.old_text)));
            }
            1 => {
                current = current.replacen(old_text.as_ref(), new_text.as_ref(), 1);
            }
            _ if edit.replace_all => {
                current = current.replace(old_text.as_ref(), new_text.as_ref());
            }
            _ => {
                let Some(near) = edit.near_line else {
                    return Err(duplicate_error(i, &current, &starts, &old_text));
                };
                // Distance from `near` to each match's span of lines, so a
                // line inside the match counts as on it.
                let span_lines = old_text.trim_end_matches('\n').matches('\n').count();
                let distance = |at: usize| {
                    let first = line_of(&current, at);
                    let last = first + span_lines;
                    if near < first { first - near } else { near.saturating_sub(last) }
                };
                let mut by_distance: Vec<(usize, usize)> = starts.iter().map(|&at| (distance(at), at)).collect();
                by_distance.sort();
                if by_distance[0].0 == by_distance[1].0 {
                    return Err(format!(
                        "edit {}: near_line {near} is equally close to the occurrences of old_text at lines {} and {}; give the line of the one to change",
                        i + 1,
                        line_of(&current, by_distance[0].1),
                        line_of(&current, by_distance[1].1)
                    ));
                }
                let at = by_distance[0].1;
                current.replace_range(at..at + old_text.len(), new_text.as_ref());
            }
        }
    }
    Ok(current)
}

/// The 1-based line that byte offset `at` of `content` falls on.
fn line_of(content: &str, at: usize) -> usize {
    content[..at].matches('\n').count() + 1
}

/// Where each occurrence of an ambiguous `old_text` is, so the model can
/// see the copies it has not read and say which one it means.
fn duplicate_error(i: usize, current: &str, starts: &[usize], old_text: &str) -> String {
    const SHOWN: usize = 5;
    let lines: Vec<&str> = current.lines().collect();
    let mut s = format!("edit {}: old_text occurs {} times but must match once", i + 1, starts.len());
    if i > 0 {
        s.push_str(&format!(" (line numbers count edits 1-{i} of this patch as applied)"));
    }
    s.push_str(":\n");
    for &at in starts.iter().take(SHOWN) {
        let line = line_of(current, at);
        let first_match = line + old_text[..old_text.len() - old_text.trim_start().len()].matches('\n').count();
        match enclosing_definition(&lines, first_match) {
            Some(def) => s.push_str(&format!("- line {first_match}, in `{def}`:\n")),
            None => s.push_str(&format!("- line {first_match}:\n")),
        }
        let lo = first_match.saturating_sub(2).max(1);
        for n in lo..=first_match.min(lines.len()) {
            s.push_str(&format!("{n:>6} | {}\n", lines[n - 1]));
        }
    }
    if starts.len() > SHOWN {
        s.push_str(&format!("- … and {} more\n", starts.len() - SHOWN));
    }
    s.push_str("Set near_line to the line of the one to change, set replace_all=true to change all of them, or extend old_text with nearby lines that differ between the copies.");
    s
}

/// The nearest top-level line above `line` that starts a definition (`fn`,
/// `def`, `class`, ...: unindented and starting with a word); `None` when
/// `line` is itself top-level.
fn enclosing_definition(lines: &[&str], line: usize) -> Option<String> {
    let starts_word = |l: &str| l.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_');
    if lines.get(line.wrapping_sub(1)).is_some_and(|l| starts_word(l)) {
        return None;
    }
    let idx = lines[..line.saturating_sub(1).min(lines.len())].iter().rposition(|l| starts_word(l))?;
    let def = lines[idx].trim_end();
    Some(if def.chars().count() > 80 { format!("{}…", def.chars().take(80).collect::<String>()) } else { def.to_string() })
}

/// The edit as it applies to `current`. Models write `\n`; a file checked
/// out with CRLF line endings (Git for Windows' default) would never match
/// a multi-line `old_text`, and a matching single line would get LF-only
/// replacement lines. When the edit does not match as written, the file
/// uses CRLF and the edit has bare LFs, both sides are converted to CRLF.
fn line_ending_adapted<'a>(current: &str, old_text: &'a str, new_text: &'a str) -> (std::borrow::Cow<'a, str>, std::borrow::Cow<'a, str>) {
    use std::borrow::Cow;
    let as_written = (Cow::Borrowed(old_text), Cow::Borrowed(new_text));
    if !current.contains("\r\n") || old_text.contains('\r') || current.contains(old_text) {
        if current.contains("\r\n") && current.contains(old_text) && !old_text.contains('\n') && new_text.contains('\n') && !new_text.contains('\r') {
            // A single-line match whose replacement spans lines.
            return (Cow::Borrowed(old_text), Cow::Owned(new_text.replace('\n', "\r\n")));
        }
        return as_written;
    }
    if !old_text.contains('\n') {
        return as_written;
    }
    let old_crlf = old_text.replace('\n', "\r\n");
    if !current.contains(&old_crlf) {
        return as_written;
    }
    let new_crlf = new_text.replace("\r\n", "\n").replace('\n', "\r\n");
    (Cow::Owned(old_crlf), Cow::Owned(new_crlf))
}

fn nearest_hint(content: &str, old_text: &str) -> String {
    let probe = old_text.lines().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("");
    if probe.is_empty() {
        return String::new();
    }
    let lines: Vec<&str> = content.lines().collect();
    if let Some(idx) = lines.iter().position(|l| l.trim() == probe) {
        let lo = idx.saturating_sub(2);
        let hi = (idx + old_text.lines().count() + 2).min(lines.len());
        let mut s = format!(" The first line matches at line {} but the rest differs (check whitespace/indentation). Current text there:\n", idx + 1);
        for (i, l) in lines[lo..hi].iter().enumerate() {
            s.push_str(&format!("{:>4} | {l}\n", lo + i + 1));
        }
        return s;
    }
    if let Some(idx) = lines.iter().position(|l| l.contains(probe)) {
        return format!(" A similar line exists at line {}: `{}`. Re-read the file and copy the exact text.", idx + 1, lines[idx].trim());
    }
    " Re-read the file with read_file and copy the exact current text.".into()
}

pub struct ApplyPatch;

#[async_trait]
impl Tool for ApplyPatch {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing exact text spans. Each edit's old_text must match the current file exactly once (include enough context to be unique); when the same text occurs more than once, set near_line to the line number (from read_file) of the occurrence to change, or replace_all to change every one. Edits apply in order, each to the result of the ones before. Read the file first: the patch is rejected if this session has not read it or if it changed since the last read (other sessions may be editing the same checkout)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1},
                "edits": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_text": {"type": "string", "description": "Exact current text to replace (may span lines)"},
                            "new_text": {"type": "string", "description": "Replacement text"},
                            "near_line": {"type": "integer", "minimum": 1, "description": "Only when old_text occurs more than once: the line number of the occurrence to change; the nearest one is changed"},
                            "replace_all": {"type": "boolean", "description": "Change every occurrence of old_text (default false)"}
                        },
                        "required": ["old_text", "new_text"],
                        "additionalProperties": false
                    }
                },
                "expected_file_hash": {"type": "string", "description": "sha256:... from read_file; the edit fails if the file hash differs"},
                "create_if_missing": {"type": "boolean", "description": "Create the file when it does not exist (old_text must then be empty)"}
            },
            "required": ["path", "edits"],
            "additionalProperties": false
        })
    }

    fn action(&self, args: &Value, ctx: &ToolContext) -> Result<Action> {
        let a: PatchArgs = serde_json::from_value(args.clone())?;
        let (abs, rel) = resolve(ctx, &a.path)?;
        let details = match std::fs::read_to_string(&abs) {
            Ok(current) => match apply_edits(&current, &a.edits) {
                Ok(new) => unified_diff(&rel, &current, &new),
                Err(e) => format!("(patch will fail: {e})"),
            },
            Err(_) => format!("(new file) {} edit(s)", a.edits.len()),
        };
        Ok(Action { tool: "apply_patch".into(), kind: ActionKind::Patch { path: a.path.clone() }, summary: format!("edit {rel} ({} edit(s))", a.edits.len()), details })
    }

    async fn invoke(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let a: PatchArgs = serde_json::from_value(args)?;
        let (abs, rel) = resolve(ctx, &a.path)?;
        let existed = abs.exists();
        if !existed && !a.create_if_missing {
            return Ok(ToolOutput::error(format!("{} does not exist; pass create_if_missing=true or use write_file.{}", a.path, suggest_paths(ctx, &a.path))));
        }
        let current = if existed { std::fs::read_to_string(&abs)? } else { String::new() };
        let current_hash = content_hash(current.as_bytes());
        if existed {
            if let Err(e) = ctx.files.check_edit(&rel, &current_hash) {
                return Ok(ToolOutput::error(e));
            }
        }
        if let Some(expected) = &a.expected_file_hash {
            let expected_full = if expected.starts_with("sha256:") { expected.clone() } else { format!("sha256:{expected}") };
            if !current_hash.starts_with(expected_full.trim_end_matches('…')) && current_hash != expected_full {
                return Ok(ToolOutput::error(format!(
                    "{rel} changed since you read it (expected {}, current sha256:{}). Re-read the file and rebuild the patch; do not overwrite blindly.",
                    expected,
                    short_hash(&current_hash)
                )));
            }
        }
        let new_content = match apply_edits(&current, &a.edits) {
            Ok(c) => c,
            Err(e) => return Ok(ToolOutput::error(format!("apply_patch on {rel} failed: {e}"))),
        };
        if new_content == current {
            return Ok(ToolOutput::error(format!("apply_patch on {rel} made no change (old_text equals new_text)")));
        }
        atomic_write(&abs, &new_content)?;
        let new_hash = content_hash(new_content.as_bytes());
        after_write(ctx, &abs, &rel, &new_content, &new_hash);
        let diff = unified_diff(&rel, &current, &new_content);
        let diff_lines: Vec<&str> = diff.lines().collect();
        let shown = if diff_lines.len() > 200 { format!("{}\n… ({} more diff lines)", diff_lines[..200].join("\n"), diff_lines.len() - 200) } else { diff };
        let out = format!("Applied {} edit(s) to {rel}. New hash sha256:{} ({} lines).\n{}", a.edits.len(), short_hash(&new_hash), new_content.lines().count(), shown);
        let effect = if existed { ToolEffect::FileModified { path: rel.clone() } } else { ToolEffect::FileCreated { path: rel.clone() } };
        Ok(ToolOutput::ok(out, format!("patched {rel} ({} edit(s))", a.edits.len())).with_effect(effect))
    }
}

// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
    #[serde(default)]
    overwrite: bool,
    expected_file_hash: Option<String>,
}

pub struct WriteFile;

#[async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Create a new file, or replace an existing file's entire content when overwrite=true (read it first; the overwrite is rejected if it changed since). Prefer apply_patch for edits to existing files."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1},
                "content": {"type": "string"},
                "overwrite": {"type": "boolean", "description": "Allow replacing an existing file (default false)"},
                "expected_file_hash": {"type": "string", "description": "When overwriting, the hash the file must currently have"}
            },
            "required": ["path", "content"],
            "additionalProperties": false
        })
    }

    fn action(&self, args: &Value, ctx: &ToolContext) -> Result<Action> {
        let a: WriteArgs = serde_json::from_value(args.clone())?;
        let (abs, rel) = resolve(ctx, &a.path)?;
        let creates = !abs.exists();
        let preview: String = a.content.lines().take(40).collect::<Vec<_>>().join("\n");
        let details = format!("{} {} ({} bytes, {} lines)\n{}{}", if creates { "create" } else { "overwrite" }, rel, a.content.len(), a.content.lines().count(), preview, if a.content.lines().count() > 40 { "\n…" } else { "" });
        Ok(Action { tool: "write_file".into(), kind: ActionKind::WriteFile { path: a.path.clone(), creates }, summary: format!("{} {rel}", if creates { "create" } else { "overwrite" }), details })
    }

    async fn invoke(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let a: WriteArgs = serde_json::from_value(args)?;
        let (abs, rel) = resolve(ctx, &a.path)?;
        let existed = abs.exists();
        if existed && abs.is_dir() {
            return Ok(ToolOutput::error(format!("{rel} is a directory")));
        }
        if existed && !a.overwrite {
            return Ok(ToolOutput::error(format!("{rel} already exists; use apply_patch for edits or pass overwrite=true to replace it entirely")));
        }
        if existed {
            let current = std::fs::read(&abs)?;
            let current_hash = content_hash(&current);
            if let Err(e) = ctx.files.check_edit(&rel, &current_hash) {
                return Ok(ToolOutput::error(e));
            }
            if let Some(expected) = &a.expected_file_hash {
                let expected_full = if expected.starts_with("sha256:") { expected.clone() } else { format!("sha256:{expected}") };
                if current_hash != expected_full && !current_hash.starts_with(&expected_full) {
                    return Ok(ToolOutput::error(format!("{rel} changed since you read it (current sha256:{}); re-read before overwriting", short_hash(&current_hash))));
                }
            }
        }
        atomic_write(&abs, &a.content)?;
        let new_hash = content_hash(a.content.as_bytes());
        after_write(ctx, &abs, &rel, &a.content, &new_hash);
        let effect = if existed { ToolEffect::FileModified { path: rel.clone() } } else { ToolEffect::FileCreated { path: rel.clone() } };
        Ok(ToolOutput::ok(format!("{} {rel} ({} bytes, {} lines, hash sha256:{})", if existed { "Replaced" } else { "Created" }, a.content.len(), a.content.lines().count(), short_hash(&new_hash)), format!("{} {rel}", if existed { "replaced" } else { "created" })).with_effect(effect))
    }
}


/// A repository-relative directory prefix as the walker writes paths:
/// forward slashes, no `./`, no leading or trailing `/`. `None` for the
/// whole repository. Models on Windows often pass `src\\api` or `.\\src`.
pub fn normalize_prefix(prefix: &str) -> Option<String> {
    let p = prefix.trim().replace('\\', "/");
    let mut p = p.as_str();
    while let Some(rest) = p.strip_prefix("./") {
        p = rest;
    }
    let p = p.trim_matches('/');
    if p.is_empty() || p == "." {
        None
    } else {
        Some(p.to_string())
    }
}

/// Whether the walker path `relative` is `prefix` or lies under it.
/// Case-insensitive on Windows, whose file systems are.
pub fn under_prefix(relative: &str, prefix: &str) -> bool {
    let (relative, prefix) = if cfg!(windows) { (relative.to_ascii_lowercase(), prefix.to_ascii_lowercase()) } else { (relative.to_string(), prefix.to_string()) };
    relative == prefix || relative.starts_with(&format!("{prefix}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_follow_the_file_line_endings() {
        let crlf = "fn a() {\r\n    1\r\n}\r\n";
        let edits = vec![Edit { old_text: "fn a() {\n    1\n}".into(), new_text: "fn a() {\n    2\n    3\n}".into(), ..Default::default() }];
        let out = apply_edits(crlf, &edits).unwrap();
        assert_eq!(out, "fn a() {\r\n    2\r\n    3\r\n}\r\n");
        let one_line = vec![Edit { old_text: "    1".into(), new_text: "    1\n    1b".into(), ..Default::default() }];
        assert_eq!(apply_edits(crlf, &one_line).unwrap(), "fn a() {\r\n    1\r\n    1b\r\n}\r\n");
        let lf = "fn a() {\n    1\n}\n";
        assert_eq!(apply_edits(lf, &edits).unwrap(), "fn a() {\n    2\n    3\n}\n", "LF files are untouched");
    }

    #[test]
    fn prefixes_accept_windows_separators() {
        assert_eq!(normalize_prefix(r"src\api\"), Some("src/api".to_string()));
        assert_eq!(normalize_prefix(r".\src"), Some("src".to_string()));
        assert_eq!(normalize_prefix("./"), None);
        assert!(under_prefix("src/api/a.rs", "src/api"));
        assert!(!under_prefix("src/apix/a.rs", "src/api"));
    }

    #[test]
    fn edits_require_unique_matches() {
        let src = "a\nb\nc\nb\n";
        assert_eq!(apply_edits(src, &[Edit { old_text: "a\n".into(), new_text: "A\n".into(), ..Default::default() }]).unwrap(), "A\nb\nc\nb\n");
        let err = apply_edits(src, &[Edit { old_text: "b\n".into(), new_text: "B\n".into(), ..Default::default() }]).unwrap_err();
        assert!(err.contains("occurs 2 times"));
        let err = apply_edits(src, &[Edit { old_text: "zzz".into(), new_text: "y".into(), ..Default::default() }]).unwrap_err();
        assert!(err.contains("not found"));
        let err = apply_edits("fn main() {\n    let x = 1;\n}\n", &[Edit { old_text: "fn main() {\n  let x = 1;\n}".into(), new_text: "".into(), ..Default::default() }]).unwrap_err();
        assert!(err.contains("indentation"), "{err}");
    }

    #[test]
    fn duplicate_matches_are_located_and_can_be_picked() {
        let src = "def a():\n    x = [\n        cred.host,\n    ]\n\ndef b():\n    y = [\n        cred.host,\n    ]\n";
        let edit = |near_line, replace_all| Edit { old_text: "        cred.host,\n    ]".into(), new_text: "        cred.id,\n    ]".into(), near_line, replace_all };
        let err = apply_edits(src, &[edit(None, false)]).unwrap_err();
        assert!(err.contains("occurs 2 times"), "{err}");
        assert!(err.contains("line 3, in `def a():`") && err.contains("line 8, in `def b():`"), "{err}");
        assert!(err.contains("near_line") && err.contains("replace_all"), "{err}");

        let second = apply_edits(src, &[edit(Some(9), false)]).unwrap();
        assert_eq!(second, src.replacen("def b():\n    y = [\n        cred.host", "def b():\n    y = [\n        cred.id", 1));
        let first = apply_edits(src, &[edit(Some(4), false)]).unwrap();
        assert!(first.starts_with("def a():\n    x = [\n        cred.id,"), "{first}");
        assert!(apply_edits(src, &[edit(Some(6), false)]).unwrap_err().contains("equally close"));
        assert_eq!(apply_edits(src, &[edit(None, true)]).unwrap(), src.replace("cred.host", "cred.id"));
        assert!(apply_edits(src, &[edit(Some(3), true)]).unwrap_err().contains("not both"));
        assert_eq!(apply_edits(src, &[Edit { old_text: "x = [".into(), new_text: "x = (".into(), near_line: Some(40), ..Default::default() }]).unwrap(), src.replace("x = [", "x = ("), "a unique match ignores near_line");
    }
}
