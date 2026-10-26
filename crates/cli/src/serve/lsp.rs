//! Language servers for the desktop editor (`lsp/*`, docs/desktop-protocol.md):
//! one `LspClient` per project and language, started the first time a
//! document of that language is opened and shut down with the project.
//! Documents are synced from the editor's buffer (not the disk), so
//! completion, hover and diagnostics see unsaved text.

use super::{Outbox, Project, RpcError, RpcResult, Server};
use ctx_core::CtxError;
use ctx_lsp::{LspClient, NotificationHandler, ServerCatalog};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

const LSP_TIMEOUT: Duration = Duration::from_secs(15);

/// `project_id\nlanguage` → client.
#[derive(Default)]
pub(super) struct LspSessions {
    clients: tokio::sync::Mutex<HashMap<String, Arc<LspClient>>>,
}

impl LspSessions {
    /// Shuts the project's servers down (in the background).
    pub(super) fn close_project(self: &Arc<Self>, project_id: &str) {
        let sessions = self.clone();
        let prefix = format!("{project_id}\n");
        tokio::spawn(async move {
            let clients: Vec<Arc<LspClient>> = {
                let mut map = sessions.clients.lock().await;
                let keys: Vec<String> = map.keys().filter(|k| k.starts_with(&prefix)).cloned().collect();
                keys.into_iter().filter_map(|k| map.remove(&k)).collect()
            };
            for client in clients {
                tokio::time::timeout(Duration::from_secs(3), client.shutdown()).await.ok();
            }
        });
    }
}

#[derive(Deserialize)]
pub(super) struct LspPathParams {
    pub(super) project_id: String,
    pub(super) path: String,
}

#[derive(Deserialize)]
pub(super) struct LspTextParams {
    pub(super) project_id: String,
    pub(super) path: String,
    pub(super) text: String,
}

#[derive(Deserialize)]
pub(super) struct LspPositionParams {
    pub(super) project_id: String,
    pub(super) path: String,
    /// 0-based, as in the protocol.
    pub(super) line: usize,
    pub(super) character: usize,
}

/// What the server says about `path`'s language before anything is started.
struct Resolved {
    relative: String,
    language: Option<String>,
    command: Option<(String, Vec<String>)>,
}

impl Server {
    fn lsp_resolve(&self, project: &Project, path: &str) -> RpcResult<Resolved> {
        let paths = project.ctx.policy.paths();
        let absolute = paths.resolve(path).map_err(RpcError::from)?;
        let relative = paths.relative(&absolute);
        let language = project.ctx.registry.language_of(&relative).map(|l| l.as_str().to_string()).filter(|l| l != "other");
        let command = language.as_deref().and_then(ServerCatalog::command_for);
        Ok(Resolved { relative, language, command })
    }

    /// The project's client for the path's language, started on first use;
    /// `None` when no server is known or installed for the language.
    async fn lsp_client(&self, project: &Arc<Project>, resolved: &Resolved) -> RpcResult<Option<Arc<LspClient>>> {
        let (Some(language), Some((executable, args))) = (&resolved.language, &resolved.command) else { return Ok(None) };
        let key = format!("{}\n{language}", project.id);
        let mut clients = self.lsp.clients.lock().await;
        if let Some(client) = clients.get(&key) {
            return Ok(Some(client.clone()));
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let handler = self.lsp_notification_handler(project);
        let client = LspClient::start_with(executable, &args, &project.ctx.root, language, LSP_TIMEOUT, Some(handler))
            .await
            .map_err(|e| RpcError::from(CtxError::other(format!("language server for {language} did not start: {e}"))))?;
        self.log("info", format!("started {executable} for {language} in {}", project.ctx.root.display()));
        clients.insert(key, client.clone());
        Ok(Some(client))
    }

    /// Forwards `textDocument/publishDiagnostics` as `lsp/diagnostics`.
    fn lsp_notification_handler(&self, project: &Arc<Project>) -> NotificationHandler {
        let out: Outbox = self.out.clone();
        let project_id = project.id.clone();
        let root = project.ctx.root.clone();
        Arc::new(move |message: Value| {
            if message.get("method").and_then(|m| m.as_str()) != Some("textDocument/publishDiagnostics") {
                return;
            }
            let Some(params) = message.get("params") else { return };
            let Some(uri) = params.get("uri").and_then(|u| u.as_str()) else { return };
            let absolute = ctx_lsp::client::uri_to_path(uri);
            let path = ctx_lsp::client::relative_to(&root, &absolute);
            let diagnostics: Vec<Value> = params
                .get("diagnostics")
                .and_then(|d| d.as_array())
                .map(|list| list.iter().filter_map(diagnostic_json).collect())
                .unwrap_or_default();
            out.notify("lsp/diagnostics", json!({"project_id": project_id, "path": path, "diagnostics": diagnostics}));
        })
    }

    pub(super) async fn lsp_status(&self, params: LspPathParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let resolved = self.lsp_resolve(&project, &params.path)?;
        let status = resolved.language.as_deref().and_then(ServerCatalog::status);
        let running = match &resolved.language {
            Some(language) => self.lsp.clients.lock().await.contains_key(&format!("{}\n{language}", project.id)),
            None => false,
        };
        Ok(json!({
            "path": resolved.relative,
            "language": resolved.language,
            "server": status.as_ref().map(|s| s.executable.clone()),
            "installed": resolved.command.is_some(),
            "running": running,
            "install_hint": status.as_ref().map(|s| s.install_hint.clone()),
        }))
    }

    /// `didOpen` (or `didChange`) with the editor's text; starts the server
    /// when needed. `server: null` when none is available for the language.
    pub(super) async fn lsp_open(&self, params: LspTextParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let resolved = self.lsp_resolve(&project, &params.path)?;
        let Some(client) = self.lsp_client(&project, &resolved).await? else {
            return Ok(json!({"path": resolved.relative, "language": resolved.language, "server": null, "version": null, "trigger_characters": []}));
        };
        let version = client.open_document(&resolved.relative, &params.text).await.map_err(RpcError::from)?;
        Ok(json!({
            "path": resolved.relative,
            "language": resolved.language,
            "server": resolved.command.as_ref().map(|(e, _)| e.clone()),
            "version": version,
            "trigger_characters": client.completion_trigger_characters(),
        }))
    }

    pub(super) async fn lsp_change(&self, params: LspTextParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let resolved = self.lsp_resolve(&project, &params.path)?;
        let Some(client) = self.lsp_client(&project, &resolved).await? else { return Ok(json!({"version": null})) };
        let version = client.change_document(&resolved.relative, &params.text).await.map_err(RpcError::from)?;
        Ok(json!({"version": version}))
    }

    pub(super) async fn lsp_close(&self, params: LspPathParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let resolved = self.lsp_resolve(&project, &params.path)?;
        if let Some(language) = &resolved.language {
            let client = self.lsp.clients.lock().await.get(&format!("{}\n{language}", project.id)).cloned();
            if let Some(client) = client {
                client.close_document(&resolved.relative).await.map_err(RpcError::from)?;
            }
        }
        Ok(json!({}))
    }

    pub(super) async fn lsp_completion(&self, params: LspPositionParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let resolved = self.lsp_resolve(&project, &params.path)?;
        let Some(client) = self.lsp_client(&project, &resolved).await? else { return Ok(json!({"items": [], "incomplete": false, "server": null})) };
        let result = client.position_request_raw("textDocument/completion", &resolved.relative, params.line, params.character, json!({})).await.map_err(RpcError::from)?;
        let (raw_items, incomplete) = match &result {
            Value::Array(items) => (items.clone(), false),
            Value::Object(o) => (o.get("items").and_then(|i| i.as_array()).cloned().unwrap_or_default(), o.get("isIncomplete").and_then(|b| b.as_bool()).unwrap_or(false)),
            _ => (Vec::new(), false),
        };
        let mut items: Vec<Value> = raw_items.iter().filter_map(completion_item_json).collect();
        items.sort_by(|a, b| {
            let key = |v: &Value| (v["sort_text"].as_str().unwrap_or("").to_string(), v["label"].as_str().unwrap_or("").to_string());
            key(a).cmp(&key(b))
        });
        items.truncate(200);
        Ok(json!({"items": items, "incomplete": incomplete, "server": resolved.command.as_ref().map(|(e, _)| e.clone())}))
    }

    pub(super) async fn lsp_hover(&self, params: LspPositionParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let resolved = self.lsp_resolve(&project, &params.path)?;
        let Some(client) = self.lsp_client(&project, &resolved).await? else { return Ok(json!({"contents": null})) };
        let result = client.position_request_raw("textDocument/hover", &resolved.relative, params.line, params.character, json!({})).await.map_err(RpcError::from)?;
        let contents = result.get("contents").map(hover_text).filter(|t| !t.trim().is_empty());
        Ok(json!({"contents": contents}))
    }

    pub(super) async fn lsp_definition(&self, params: LspPositionParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let resolved = self.lsp_resolve(&project, &params.path)?;
        let Some(client) = self.lsp_client(&project, &resolved).await? else { return Ok(json!({"locations": []})) };
        let result = client.position_request_raw("textDocument/definition", &resolved.relative, params.line, params.character, json!({})).await.map_err(RpcError::from)?;
        Ok(json!({"locations": client.parse_locations(&result)}))
    }
}

/// The client's shape of one `CompletionItem`: `kind` by name, the text to
/// insert with snippet placeholders reduced, and the server's replace range
/// when it gave one.
fn completion_item_json(item: &Value) -> Option<Value> {
    let label = item.get("label")?.as_str()?.to_string();
    let snippet = item.get("insertTextFormat").and_then(|f| f.as_u64()) == Some(2);
    let text_edit = item.get("textEdit");
    let raw_insert = text_edit
        .and_then(|e| e.get("newText"))
        .or_else(|| item.get("insertText"))
        .and_then(|t| t.as_str())
        .map(String::from)
        .unwrap_or_else(|| label.clone());
    let insert_text = if snippet { strip_snippet(&raw_insert) } else { raw_insert };
    let range = text_edit.and_then(|e| e.get("range").or_else(|| e.get("replace")).or_else(|| e.get("insert"))).and_then(|r| {
        Some(json!({
            "start_line": r.pointer("/start/line")?.as_u64()?,
            "start_character": r.pointer("/start/character")?.as_u64()?,
            "end_line": r.pointer("/end/line")?.as_u64()?,
            "end_character": r.pointer("/end/character")?.as_u64()?,
        }))
    });
    let documentation = item.get("documentation").map(|d| match d {
        Value::String(s) => s.clone(),
        other => other.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string(),
    });
    Some(json!({
        "label": label,
        "kind": completion_kind_name(item.get("kind").and_then(|k| k.as_u64()).unwrap_or(0)),
        "detail": item.get("detail").and_then(|d| d.as_str()),
        "documentation": documentation.filter(|d| !d.is_empty()),
        "insert_text": insert_text,
        "filter_text": item.get("filterText").and_then(|f| f.as_str()),
        "sort_text": item.get("sortText").and_then(|s| s.as_str()),
        "range": range,
        "deprecated": item.get("deprecated").and_then(|d| d.as_bool()).unwrap_or(false),
    }))
}

/// `${1:name}` → `name`, `$0` / `$1` → nothing, `\$` → `$`.
fn strip_snippet(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() => {
                out.push(chars[i + 1]);
                i += 2;
            }
            '$' if i + 1 < chars.len() && chars[i + 1] == '{' => {
                let mut depth = 0;
                let mut j = i + 1;
                let mut placeholder = String::new();
                let mut seen_colon = false;
                let mut choice = false;
                while j < chars.len() {
                    match chars[j] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        ':' if depth == 1 && !seen_colon => {
                            seen_colon = true;
                            j += 1;
                            continue;
                        }
                        // `${1|one,two|}`: the first choice stands in.
                        '|' if depth == 1 && !seen_colon => {
                            seen_colon = true;
                            choice = true;
                            j += 1;
                            continue;
                        }
                        c if seen_colon => placeholder.push(c),
                        _ => {}
                    }
                    j += 1;
                }
                let placeholder = if choice { placeholder.split(|c| c == ',' || c == '|').next().unwrap_or("").to_string() } else { placeholder };
                // A nested `${2:x}` inside the placeholder is reduced the same way.
                out.push_str(&strip_snippet(&placeholder));
                i = j + 1;
            }
            '$' if i + 1 < chars.len() && chars[i + 1].is_ascii_digit() => {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn completion_kind_name(kind: u64) -> &'static str {
    match kind {
        1 => "text",
        2 => "method",
        3 => "function",
        4 => "constructor",
        5 => "field",
        6 => "variable",
        7 => "class",
        8 => "interface",
        9 => "module",
        10 => "property",
        11 => "unit",
        12 => "value",
        13 => "enum",
        14 => "keyword",
        15 => "snippet",
        16 => "color",
        17 => "file",
        18 => "reference",
        19 => "folder",
        20 => "enum_member",
        21 => "constant",
        22 => "struct",
        23 => "event",
        24 => "operator",
        25 => "type_parameter",
        _ => "text",
    }
}

/// `MarkedString | MarkedString[] | MarkupContent` → one text.
fn hover_text(contents: &Value) -> String {
    match contents {
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(hover_text).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n\n"),
        Value::Object(o) => o.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        _ => String::new(),
    }
}

fn diagnostic_json(diagnostic: &Value) -> Option<Value> {
    let range = diagnostic.get("range")?;
    let severity = match diagnostic.get("severity").and_then(|s| s.as_u64()).unwrap_or(1) {
        1 => "error",
        2 => "warning",
        3 => "information",
        _ => "hint",
    };
    Some(json!({
        "line": range.pointer("/start/line")?.as_u64()?,
        "character": range.pointer("/start/character")?.as_u64()?,
        "end_line": range.pointer("/end/line")?.as_u64()?,
        "end_character": range.pointer("/end/character")?.as_u64()?,
        "severity": severity,
        "message": diagnostic.get("message").and_then(|m| m.as_str()).unwrap_or(""),
        "source": diagnostic.get("source").and_then(|s| s.as_str()),
        "code": diagnostic.get("code").map(|c| match c { Value::String(s) => s.clone(), other => other.to_string() }),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippets_lose_their_placeholders() {
        assert_eq!(strip_snippet("foo(${1:a}, ${2:b})$0"), "foo(a, b)");
        assert_eq!(strip_snippet("println!(\\$\"{}\", $1)"), "println!($\"{}\", )");
        assert_eq!(strip_snippet("plain"), "plain");
        assert_eq!(strip_snippet("${1|one,two|}"), "one");
    }

    #[test]
    fn hover_contents_flatten() {
        assert_eq!(hover_text(&json!("x")), "x");
        assert_eq!(hover_text(&json!({"kind": "markdown", "value": "```rust\nfn a()\n```"})), "```rust\nfn a()\n```");
        assert_eq!(hover_text(&json!([{"language": "rust", "value": "fn a()"}, "docs"])), "fn a()\n\ndocs");
    }
}
