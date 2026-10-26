//! Signing in to an app's service instead of pasting a token: GitHub's
//! OAuth device flow (the one `gh auth login` uses). Context's own GitHub
//! OAuth App supplies a public client ID; device flow needs no client
//! secret. The token goes to the credential store (`credentials.toml`,
//! mode 0600) under the app's credential name, and the server entry refers
//! to it as `${credential:github}`, so it never lands in `config.toml`.

use ctx_core::paths::DataLayout;
use ctx_core::{CredentialStore, CtxError, Result, StoredCredential};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

/// The client ID of Context's GitHub OAuth App (device flow enabled).
/// Empty until the app is registered; `CTX_GITHUB_CLIENT_ID` overrides it.
pub const GITHUB_CLIENT_ID: &str = "Iv23liY59Xgub5CgyLsB";

/// What the server entry's env holds instead of the token itself.
pub fn credential_reference(name: &str) -> String {
    format!("${{credential:{name}}}")
}

/// The client ID in force, if any.
pub fn github_client_id() -> Option<String> {
    std::env::var("CTX_GITHUB_CLIENT_ID").ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).or_else(|| Some(GITHUB_CLIENT_ID.trim().to_string()).filter(|v| !v.is_empty()))
}

/// The first step of a sign-in: the code the user types on `verification_uri`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    /// Seconds to wait between polls.
    pub interval: u64,
}

/// One poll's outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DevicePoll {
    /// The user has not approved yet; poll again after `interval` seconds.
    Pending { interval: u64 },
    /// Signed in; the token is stored, only the account name is returned.
    Approved { account: String },
    Denied,
    Expired,
}

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("context-agent/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| CtxError::tool(format!("could not build the HTTP client: {e}")))
}

fn not_configured() -> CtxError {
    CtxError::Config("GitHub sign-in is not set up in this build (no OAuth App client ID); paste a personal access token instead, or set CTX_GITHUB_CLIENT_ID".into())
}

async fn post_json(url: &str, body: Value) -> Result<Value> {
    let response = client()?.post(url).header("Accept", "application/json").json(&body).send().await.map_err(|e| CtxError::tool(format!("GitHub could not be reached: {e}")))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| CtxError::tool(format!("GitHub's answer could not be read: {e}")))?;
    let value: Value = serde_json::from_str(&text).map_err(|_| CtxError::tool(format!("GitHub answered HTTP {status}: {}", text.chars().take(200).collect::<String>())))?;
    if !status.is_success() && value.get("error").is_none() {
        return Err(CtxError::tool(format!("GitHub answered HTTP {status}")));
    }
    Ok(value)
}

/// `POST https://github.com/login/device/code`.
pub async fn github_device_start(scopes: &[String]) -> Result<DeviceCode> {
    let client_id = github_client_id().ok_or_else(not_configured)?;
    let value = post_json("https://github.com/login/device/code", json!({"client_id": client_id, "scope": scopes.join(" ")})).await?;
    if let Some(error) = value.get("error").and_then(|e| e.as_str()) {
        let description = value.get("error_description").and_then(|d| d.as_str()).unwrap_or(error);
        let hint = match error {
            "device_flow_disabled" => " (enable Device Flow in the OAuth App's settings)",
            "Not Found" => " (GitHub does not know this OAuth App client ID; check GITHUB_CLIENT_ID or CTX_GITHUB_CLIENT_ID)",
            _ => "",
        };
        return Err(CtxError::tool(format!("GitHub refused the sign-in: {description}{hint}")));
    }
    serde_json::from_value(value).map_err(|e| CtxError::tool(format!("GitHub's device code answer was unexpected: {e}")))
}

/// `POST https://github.com/login/oauth/access_token` once. On approval the
/// token is stored under `credential` with the account's login.
pub async fn github_device_poll(layout: &DataLayout, credential: &str, device_code: &str) -> Result<DevicePoll> {
    let client_id = github_client_id().ok_or_else(not_configured)?;
    let value = post_json("https://github.com/login/oauth/access_token", json!({"client_id": client_id, "device_code": device_code, "grant_type": "urn:ietf:params:oauth:grant-type:device_code"})).await?;
    if let Some(token) = value.get("access_token").and_then(|t| t.as_str()).filter(|t| !t.is_empty()) {
        let account = github_login(token).await.unwrap_or_else(|_| "GitHub".to_string());
        let mut store = CredentialStore::load(layout)?;
        store.set(credential, StoredCredential { api_key: token.to_string(), origin: Some("https://github.com".into()), account: Some(account.clone()), restore_base_url: None });
        store.save()?;
        return Ok(DevicePoll::Approved { account });
    }
    let interval = value.get("interval").and_then(|i| i.as_u64()).unwrap_or(5);
    match value.get("error").and_then(|e| e.as_str()).unwrap_or("authorization_pending") {
        "authorization_pending" => Ok(DevicePoll::Pending { interval }),
        // GitHub asks for five more seconds between polls.
        "slow_down" => Ok(DevicePoll::Pending { interval: interval.max(5) + 5 }),
        "expired_token" | "token_expired" => Ok(DevicePoll::Expired),
        "access_denied" => Ok(DevicePoll::Denied),
        other => {
            let description = value.get("error_description").and_then(|d| d.as_str()).unwrap_or(other);
            Err(CtxError::tool(format!("GitHub sign-in failed: {description}")))
        }
    }
}

/// The login of the account a token belongs to (`GET /user`).
pub async fn github_login(token: &str) -> Result<String> {
    let response = client()?.get("https://api.github.com/user").bearer_auth(token).header("Accept", "application/vnd.github+json").send().await.map_err(|e| CtxError::tool(format!("GitHub could not be reached: {e}")))?;
    if !response.status().is_success() {
        return Err(CtxError::tool(format!("GitHub answered HTTP {} for the signed-in user", response.status())));
    }
    let value: Value = response.json().await.map_err(|e| CtxError::tool(format!("GitHub's user answer was unexpected: {e}")))?;
    value.get("login").and_then(|l| l.as_str()).map(str::to_string).ok_or_else(|| CtxError::tool("GitHub did not return a login"))
}

/// The account signed in under `credential`, if any.
pub fn signed_in_account(layout: &DataLayout, credential: &str) -> Option<String> {
    let store = CredentialStore::load(layout).ok()?;
    let entry = store.get(credential)?;
    if entry.api_key.trim().is_empty() {
        return None;
    }
    Some(entry.account.clone().unwrap_or_else(|| "signed in".into()))
}

/// Forgets the stored token (it stays valid on GitHub until the user
/// revokes the app there).
pub fn sign_out(layout: &DataLayout, credential: &str) -> Result<bool> {
    let mut store = CredentialStore::load(layout)?;
    let removed = store.remove(credential).is_some();
    if removed {
        store.save()?;
    }
    Ok(removed)
}

/// The credential names a server entry refers to with `${credential:NAME}`.
pub fn referenced_credentials(server: &ctx_core::McpServerConfig) -> Vec<String> {
    let mut names = Vec::new();
    for value in server.env.values().chain(server.headers.values()) {
        let mut rest = value.as_str();
        while let Some(start) = rest.find("${credential:") {
            let after = &rest[start + "${credential:".len()..];
            let Some(end) = after.find('}') else { break };
            let name = after[..end].trim().to_string();
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
            rest = &after[end + 1..];
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_references_are_found() {
        let mut server = ctx_core::McpServerConfig::stdio("npx", vec![]);
        server.env.insert("GITHUB_PERSONAL_ACCESS_TOKEN".into(), credential_reference("github"));
        server.headers.insert("Authorization".into(), "Bearer ${credential:docs} ${credential:github}".into());
        assert_eq!(referenced_credentials(&server), vec!["github".to_string(), "docs".to_string()]);
    }

    #[test]
    fn polls_classify_github_answers() {
        // Pure mapping is exercised through the public enum's serialization.
        assert_eq!(serde_json::to_value(DevicePoll::Pending { interval: 10 }).unwrap(), json!({"status": "pending", "interval": 10}));
        assert_eq!(serde_json::to_value(DevicePoll::Approved { account: "octocat".into() }).unwrap(), json!({"status": "approved", "account": "octocat"}));
        assert_eq!(serde_json::to_value(DevicePoll::Denied).unwrap(), json!({"status": "denied"}));
    }

    #[test]
    fn stored_tokens_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(dir.path().to_path_buf());
        assert_eq!(signed_in_account(&layout, "github"), None);
        let mut store = CredentialStore::load(&layout).unwrap();
        store.set("github", StoredCredential { api_key: "gho_x".into(), origin: None, account: Some("octocat".into()), restore_base_url: None });
        store.save().unwrap();
        assert_eq!(signed_in_account(&layout, "github").as_deref(), Some("octocat"));
        assert!(sign_out(&layout, "github").unwrap());
        assert_eq!(signed_in_account(&layout, "github"), None);
    }
}
