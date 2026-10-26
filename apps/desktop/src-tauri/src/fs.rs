//! The few places the app touches the disk itself (ARCHITECTURE.md
//! "Toolchain and rules"): the Inspector's file tree (the listing rules of
//! `FilesTab.swift`: `.git` and `.DS_Store` hidden, folders first, names in
//! Finder order), reading attachments, pasted images (`PastedFiles` and
//! `AttachmentStore` in `AttachmentUpload.swift`: `<app data>/Pasted Images`,
//! pruned of files older than 30 days the first time the folder is written
//! to after launch), the project folder picker, and the "Reveal in Finder" /
//! "Show in Explorer", "Open with Default Editor" and link actions
//! (`PathAction` in `AppState.swift`) through the opener plugin.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use base64::Engine;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::backend::lock;

/// `FileTree.hiddenNames`.
pub const HIDDEN_NAMES: &[&str] = &[".git", ".DS_Store"];
/// `PastedFiles.directory`'s last component, under the app data directory.
pub const PASTED_IMAGES_DIRECTORY: &str = "Pasted Images";
/// `AttachmentStore.retention`.
pub const RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// `fs_read_text`'s cap when the caller gives none.
pub const DEFAULT_MAX_BYTES: u64 = 1024 * 1024;

/// One row of `fs_list_dir` (`FileNode` in `FilesTab.swift`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirEntryInfo {
    pub name: String,
    /// The entry's full path.
    pub path: String,
    pub is_dir: bool,
    /// Bytes for files, 0 for folders.
    pub size: u64,
}

/// `fs_read_text`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextFile {
    pub content: String,
    /// True when the file is longer than `max_bytes`.
    pub truncated: bool,
    /// The whole file's size in bytes.
    pub size: u64,
}

/// `fs_file_info`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileInfo {
    pub size: u64,
    pub is_dir: bool,
    /// Modification time, RFC 3339.
    pub modified: Option<String>,
    /// Guessed from the extension for images, PDFs and common text files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
}

// MARK: - Listing

/// One level of `path`: `.git` and `.DS_Store` hidden, folders first, then
/// names in Finder order (case-insensitive, digit runs compared as numbers).
pub fn list_dir(path: &Path) -> Result<Vec<DirEntryInfo>, String> {
    let entries = fs::read_dir(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let mut rows = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if HIDDEN_NAMES.contains(&name.as_str()) {
            continue;
        }
        let full = entry.path();
        // Through symlinks, like `URLResourceKey.isDirectoryKey`.
        let metadata = fs::metadata(&full).ok();
        let is_dir = metadata.as_ref().is_some_and(fs::Metadata::is_dir);
        let size = metadata.as_ref().filter(|m| m.is_file()).map_or(0, fs::Metadata::len);
        rows.push(DirEntryInfo {
            name,
            path: full.to_string_lossy().into_owned(),
            is_dir,
            size,
        });
    }
    rows.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| natural_cmp(&a.name, &b.name)));
    Ok(rows)
}

/// `localizedStandardCompare`: case-insensitive, digit runs compared by
/// value (`file2` before `file10`), equal names ordered by their bytes.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let (mut i, mut j) = (0, 0);
    while i < a_chars.len() && j < b_chars.len() {
        let (ca, cb) = (a_chars[i], b_chars[j]);
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let start_a = i;
            while i < a_chars.len() && a_chars[i].is_ascii_digit() {
                i += 1;
            }
            let start_b = j;
            while j < b_chars.len() && b_chars[j].is_ascii_digit() {
                j += 1;
            }
            let na: String = a_chars[start_a..i].iter().collect();
            let nb: String = b_chars[start_b..j].iter().collect();
            let (na, nb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
            let ordering = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb));
            if ordering != Ordering::Equal {
                return ordering;
            }
        } else {
            let ordering = ca.to_lowercase().cmp(cb.to_lowercase());
            if ordering != Ordering::Equal {
                return ordering;
            }
            i += 1;
            j += 1;
        }
    }
    (a_chars.len() - i).cmp(&(b_chars.len() - j)).then_with(|| a.cmp(b))
}

// MARK: - Reading

/// Up to `max_bytes` of `path` as text; a multi-byte character cut by the
/// limit is dropped rather than replaced.
pub fn read_text(path: &Path, max_bytes: u64) -> Result<TextFile, String> {
    let mut file = fs::File::open(path).map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut bytes = Vec::with_capacity(usize::try_from(size.min(max_bytes)).unwrap_or(0).saturating_add(1));
    file.by_ref()
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let limit = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    let truncated = bytes.len() > limit;
    if truncated {
        bytes.truncate(limit);
        if let Err(error) = std::str::from_utf8(&bytes) {
            if error.error_len().is_none() {
                bytes.truncate(error.valid_up_to());
            }
        }
    }
    let content = String::from_utf8(bytes).unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned());
    Ok(TextFile { content, truncated, size })
}

/// Size, kind, modification time and a guessed MIME type.
pub fn file_info(path: &Path) -> Result<FileInfo, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("Could not stat {}: {error}", path.display()))?;
    let modified = metadata.modified().ok().map(rfc3339);
    Ok(FileInfo {
        size: if metadata.is_file() { metadata.len() } else { 0 },
        is_dir: metadata.is_dir(),
        modified,
        mime: if metadata.is_dir() { None } else { mime_for(path) },
    })
}

/// A MIME type from the extension for what the viewers and the composer
/// care about (images, PDFs, common text); `None` otherwise.
pub fn mime_for(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "tif" | "tiff" => "image/tiff",
        "heic" => "image/heic",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "md" | "markdown" => "text/markdown",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "xml" => "application/xml",
        "js" | "mjs" | "cjs" => "text/javascript",
        "txt" | "text" | "log" | "toml" | "yaml" | "yml" | "rs" | "py" | "go" | "java" | "ts" | "tsx" | "jsx"
        | "swift" | "sh" | "zsh" | "bash" | "c" | "h" | "cpp" | "hpp" | "cc" | "rb" | "php" | "sql" | "ini" | "cfg"
        | "conf" => "text/plain",
        _ => return None,
    };
    Some(mime.to_owned())
}

fn rfc3339(time: SystemTime) -> String {
    DateTime::<Utc>::from(time).to_rfc3339_opts(SecondsFormat::Millis, true)
}

// MARK: - Pasted images

/// Writes `bytes` as `<base_dir>/Pasted Images/<timestamp>.<extension>`
/// (atomically: temp file + rename) and returns the path. The folder is
/// pruned of files older than [`RETENTION`] the first time it is written
/// to after launch, like `AttachmentStore.write`.
pub fn write_temp_image(base_dir: &Path, bytes: &[u8], extension: &str) -> Result<PathBuf, String> {
    let directory = base_dir.join(PASTED_IMAGES_DIRECTORY);
    fs::create_dir_all(&directory).map_err(|error| format!("Could not create {}: {error}", directory.display()))?;
    prune_once(&directory);

    let extension = sanitize_extension(extension);
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f").to_string();
    let mut path = directory.join(format!("{stamp}.{extension}"));
    let mut counter = 1;
    while path.exists() {
        path = directory.join(format!("{stamp}-{counter}.{extension}"));
        counter += 1;
    }
    let temp = directory.join(format!(".{stamp}-{counter}.{extension}.part"));
    fs::write(&temp, bytes).map_err(|error| format!("Could not write {}: {error}", temp.display()))?;
    if let Err(error) = fs::rename(&temp, &path) {
        let _ = fs::remove_file(&temp);
        return Err(format!("Could not write {}: {error}", path.display()));
    }
    Ok(path)
}

/// Removes files in `directory` modified more than [`RETENTION`] ago.
pub fn prune(directory: &Path) {
    let Some(cutoff) = SystemTime::now().checked_sub(RETENTION) else {
        return;
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.is_file() && metadata.modified().is_ok_and(|modified| modified < cutoff) {
            let _ = fs::remove_file(&path);
        }
    }
}

fn prune_once(directory: &Path) {
    static PRUNED: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    let pruned = PRUNED.get_or_init(|| Mutex::new(HashSet::new()));
    if lock(pruned).insert(directory.to_path_buf()) {
        prune(directory);
    }
}

/// `png` by default; letters and digits only, lower-cased, no leading dot.
fn sanitize_extension(extension: &str) -> String {
    let cleaned: String = extension
        .trim()
        .trim_start_matches('.')
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .take(8)
        .collect();
    if cleaned.is_empty() {
        "png".to_owned()
    } else {
        cleaned
    }
}

/// Decodes standard base64, accepting a `data:…;base64,` prefix as well.
fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    let text = text.trim();
    let payload = match text.strip_prefix("data:").and_then(|rest| rest.split_once(',')) {
        Some((_, payload)) => payload,
        None => text,
    };
    base64::engine::general_purpose::STANDARD
        .decode(payload.as_bytes())
        .map_err(|error| format!("Invalid image data: {error}"))
}

// MARK: - Commands

async fn blocking<T, F>(task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| format!("File task failed: {error}"))?
}

/// `fs_list_dir({ path })` → `[{ name, path, is_dir, size }]`.
#[tauri::command]
pub async fn fs_list_dir(path: String) -> Result<Vec<DirEntryInfo>, String> {
    blocking(move || list_dir(Path::new(&path))).await
}

/// `fs_read_text({ path, max_bytes })` → `{ content, truncated, size }`.
#[tauri::command(rename_all = "snake_case")]
pub async fn fs_read_text(path: String, max_bytes: Option<u64>) -> Result<TextFile, String> {
    let max_bytes = max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    blocking(move || read_text(Path::new(&path), max_bytes)).await
}

/// `fs_file_info({ path })` → `{ size, is_dir, modified, mime? }`.
#[tauri::command]
pub async fn fs_file_info(path: String) -> Result<FileInfo, String> {
    blocking(move || file_info(Path::new(&path))).await
}

/// `fs_write_temp_image({ bytes_base64, extension })` → the saved path under
/// `<app data dir>/Pasted Images`.
#[tauri::command(rename_all = "snake_case")]
pub async fn fs_write_temp_image(app: AppHandle, bytes_base64: String, extension: Option<String>) -> Result<String, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("No application data directory: {error}"))?;
    let bytes = decode_base64(&bytes_base64)?;
    let extension = extension.unwrap_or_default();
    blocking(move || write_temp_image(&base, &bytes, &extension).map(|path| path.to_string_lossy().into_owned())).await
}

/// `fs_pick_project()` → the chosen folder, or `null` when dismissed
/// (`NSOpenPanel`, directories only, in `AppState.chooseProject`).
#[tauri::command]
pub async fn fs_pick_project(app: AppHandle) -> Result<Option<String>, String> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file().set_title("Open Project");
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.set_parent(&window);
        }
        dialog.blocking_pick_folder()
    })
    .await
    .map_err(|error| format!("Folder picker failed: {error}"))?;
    match picked {
        None => Ok(None),
        Some(file) => file
            .into_path()
            .map(|path| Some(path.to_string_lossy().into_owned()))
            .map_err(|error| format!("Folder picker returned an unusable path: {error}")),
    }
}

/// `fs_save_text({ title, default_name, content, extension? })` → the path the
/// text was written to, or `null` when the save dialog was dismissed
/// (`NSSavePanel` in "Export Chat as Markdown…" and the full-output sheet's
/// "Save…"). Only ever writes where the user chose in the dialog.
#[tauri::command(rename_all = "snake_case")]
pub async fn fs_save_text(app: AppHandle, title: String, default_name: String, content: String, extension: Option<String>) -> Result<Option<String>, String> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file().set_title(&title).set_file_name(&default_name);
        if let Some(ext) = extension.as_deref().map(|e| e.trim_start_matches('.')).filter(|e| !e.is_empty()) {
            dialog = dialog.add_filter(ext.to_uppercase(), &[ext]);
        }
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.set_parent(&window);
        }
        dialog.blocking_save_file()
    })
    .await
    .map_err(|error| format!("Save dialog failed: {error}"))?;
    let Some(file) = picked else { return Ok(None) };
    let path = file.into_path().map_err(|error| format!("The save dialog returned an unusable path: {error}"))?;
    blocking(move || {
        std::fs::write(&path, content.as_bytes()).map_err(|error| format!("Could not save {}: {error}", path.display()))?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
}

/// `reveal_in_file_manager({ path })`: Finder / Explorer with the item selected.
#[tauri::command]
pub async fn reveal_in_file_manager(path: String) -> Result<(), String> {
    blocking(move || {
        tauri_plugin_opener::reveal_item_in_dir(&path).map_err(|error| format!("Could not reveal {path}: {error}"))
    })
    .await
}

/// `open_with_default_app({ path })`.
#[tauri::command]
pub async fn open_with_default_app(path: String) -> Result<(), String> {
    blocking(move || {
        tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|error| format!("Could not open {path}: {error}"))
    })
    .await
}

/// `open_url({ url })`: the system browser.
#[tauri::command]
pub async fn open_url(url: String) -> Result<(), String> {
    blocking(move || {
        tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|error| format!("Could not open {url}: {error}"))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_matches_finder() {
        let mut names = vec!["file10", "File2", "file1", "alpha", "Beta", "file02"];
        names.sort_by(|a, b| natural_cmp(a, b));
        // Equal numbers (2 and 02) fall back to byte order: "File2" < "file02".
        assert_eq!(names, vec!["alpha", "Beta", "file1", "File2", "file02", "file10"]);
    }

    #[test]
    fn listing_hides_git_and_sorts_folders_first() {
        let root = std::env::temp_dir().join(format!("ctx-fs-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("zeta")).unwrap();
        fs::write(root.join("Alpha.txt"), "a").unwrap();
        fs::write(root.join(".DS_Store"), "").unwrap();
        fs::write(root.join("beta.txt"), "bb").unwrap();
        let rows = list_dir(&root).unwrap();
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, vec!["zeta", "Alpha.txt", "beta.txt"]);
        assert!(rows[0].is_dir);
        assert_eq!(rows[2].size, 2);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reading_caps_and_keeps_utf8_boundaries() {
        let root = std::env::temp_dir().join(format!("ctx-fs-read-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("text.txt");
        fs::write(&path, "héllo wörld").unwrap();
        let whole = read_text(&path, 1024).unwrap();
        assert_eq!(whole.content, "héllo wörld");
        assert!(!whole.truncated);
        let cut = read_text(&path, 2).unwrap();
        assert_eq!(cut.content, "h");
        assert!(cut.truncated);
        assert_eq!(cut.size, "héllo wörld".len() as u64);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn pasted_images_land_in_the_folder_with_a_clean_extension() {
        let root = std::env::temp_dir().join(format!("ctx-fs-paste-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let path = write_temp_image(&root, b"\x89PNG", ".PNG").unwrap();
        assert!(path.starts_with(root.join(PASTED_IMAGES_DIRECTORY)));
        assert_eq!(path.extension().unwrap(), "png");
        assert_eq!(fs::read(&path).unwrap(), b"\x89PNG");
        let second = write_temp_image(&root, b"x", "").unwrap();
        assert_ne!(path, second);
        assert!(!fs::read_dir(root.join(PASTED_IMAGES_DIRECTORY)).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".part")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn base64_accepts_data_urls() {
        assert_eq!(decode_base64("aGk=").unwrap(), b"hi");
        assert_eq!(decode_base64("data:image/png;base64,aGk=").unwrap(), b"hi");
        assert!(decode_base64("***").is_err());
    }
}
