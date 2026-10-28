//! ModelGateway over HTTP, shared by `ctx serve`'s `gateway/*` methods and
//! the TUI's `/signin` and `/signout`: the `{success, message, data}` call,
//! the device-code handshake that stores the approved key in the credential
//! store (`CredentialStore::MODELGATEWAY`), sign-out, and pointing the user
//! configuration file's model endpoint at the gateway and back.

use crate::serve::write_private_atomic;
use ctx_core::paths::DataLayout;
use ctx_core::{Config, CredentialStore, CtxError, StoredCredential};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

pub const DEFAULT_ORIGIN: &str = "https://www.modelgateway.co";
/// Sent with every model request while per-project usage is on
/// (`model.project_header`); the gateway stores it with its usage rows.
pub const PROJECT_HEADER: &str = "X-ModelGateway-Project";
const TIMEOUT: Duration = Duration::from_secs(20);
const CLIENT_NAME: &str = "Context";

/// Why a gateway call failed.
#[derive(Debug)]
pub enum GatewayError {
    /// The gateway could not be reached or did not answer with its envelope.
    Transport(String),
    /// 401/403: the key was rejected (or is already revoked).
    Unauthorized { message: String, code: Option<String> },
    /// The gateway answered `success: false`.
    Gateway { message: String, code: Option<String> },
    /// No stored ModelGateway credential.
    SignedOut,
    /// A bad origin, or the credential store or configuration file.
    Local(CtxError),
}

impl std::fmt::Display for GatewayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GatewayError::Transport(message) => write!(f, "ModelGateway could not be reached: {message}"),
            GatewayError::Unauthorized { message, .. } | GatewayError::Gateway { message, .. } => f.write_str(message),
            GatewayError::SignedOut => f.write_str("not signed in to ModelGateway"),
            GatewayError::Local(e) => write!(f, "{e}"),
        }
    }
}

impl From<CtxError> for GatewayError {
    fn from(e: CtxError) -> Self {
        GatewayError::Local(e)
    }
}

pub type GatewayResult<T> = std::result::Result<T, GatewayError>;

/// `{success, message, data}`: every gateway API reply.
#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    message: String,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    data: Value,
}

/// `gateway/device/start`'s reply.
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub interval: Option<u64>,
}

/// The gateway origin without a trailing slash or `/v1`; `None` when it is
/// not an absolute http(s) URL.
pub fn origin(base_url: &str) -> Option<String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    let origin = trimmed.strip_suffix("/v1").unwrap_or(trimmed);
    if !(origin.starts_with("https://") || origin.starts_with("http://")) || origin.len() <= 8 {
        return None;
    }
    Some(origin.to_string())
}

pub fn client() -> GatewayResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(format!("context-agent/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| GatewayError::Local(CtxError::Config(format!("cannot build the HTTP client: {e}"))))
}

/// One gateway call; the reply's `data` when `success` is true.
pub async fn call(request: reqwest::RequestBuilder, what: &str) -> GatewayResult<Value> {
    let response = request.send().await.map_err(|e| GatewayError::Transport(format!("{what}: {e}")))?;
    let status = response.status();
    let body = response.text().await.map_err(|e| GatewayError::Transport(format!("{what}: {e}")))?;
    let envelope: Envelope = match serde_json::from_str(&body) {
        Ok(envelope) => envelope,
        Err(_) if !status.is_success() => return Err(GatewayError::Transport(format!("{what}: HTTP {}", status.as_u16()))),
        Err(e) => return Err(GatewayError::Transport(format!("{what}: unexpected reply ({e})"))),
    };
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        let message = if envelope.message.is_empty() { format!("{what}: the gateway rejected the API key") } else { envelope.message };
        return Err(GatewayError::Unauthorized { message, code: envelope.code });
    }
    if !envelope.success {
        let message = if envelope.message.is_empty() { format!("{what}: the gateway reported a failure (HTTP {})", status.as_u16()) } else { envelope.message };
        return Err(GatewayError::Gateway { message, code: envelope.code });
    }
    Ok(envelope.data)
}

/// The stored credential with a key, if any.
pub fn stored(layout: &DataLayout) -> GatewayResult<Option<StoredCredential>> {
    let store = CredentialStore::load(layout)?;
    Ok(store.get(CredentialStore::MODELGATEWAY).filter(|c| !c.api_key.trim().is_empty()).cloned())
}

/// Origin + key for an authenticated call: the stored credential, with
/// `base_url` (when given) overriding the stored origin.
pub fn login(layout: &DataLayout, base_url: Option<&str>) -> GatewayResult<(String, String)> {
    let credential = stored(layout)?.ok_or(GatewayError::SignedOut)?;
    let raw = base_url.map(str::trim).filter(|b| !b.is_empty()).unwrap_or(credential.origin.as_deref().unwrap_or_default());
    let origin = origin(raw).ok_or_else(|| GatewayError::Local(CtxError::Config(format!("base_url must be an absolute http(s) URL, got `{raw}`"))))?;
    Ok((origin, credential.api_key.trim().to_string()))
}

/// A call authenticated with the stored key: `method` on `path` under the
/// origin (`base_url`, when given, overrides the stored one).
async fn authed(layout: &DataLayout, base_url: Option<&str>, method: reqwest::Method, path: &str, body: Option<Value>, what: &str) -> GatewayResult<Value> {
    let (origin, key) = login(layout, base_url)?;
    let mut request = client()?.request(method, format!("{origin}{path}")).bearer_auth(key);
    if let Some(body) = body {
        request = request.json(&body);
    }
    call(request, what).await
}

/// The account summary behind the stored key: balance, used quota,
/// requests, the device's token and the active plans.
pub async fn account(layout: &DataLayout, base_url: Option<&str>) -> GatewayResult<Value> {
    let data = authed(layout, base_url, reqwest::Method::GET, "/api/device/self", None, "gateway/account").await?;
    // A key from before device ids (or from a client that sent none) is
    // identified once, then the summary is read again so it says so.
    let unidentified = data.get("device").and_then(|d| d.get("identified")).and_then(Value::as_bool) == Some(false);
    if unidentified && !IDENTIFIED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        if identify(layout, base_url).await.is_ok() {
            return authed(layout, base_url, reqwest::Method::GET, "/api/device/self", None, "gateway/account").await;
        }
    }
    Ok(data)
}

/// Whether this process already tried to identify the device, so a gateway
/// that keeps answering "unidentified" is asked once, not on every refresh.
static IDENTIFIED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Hourly usage rows per model and project, between two Unix times.
pub async fn usage(layout: &DataLayout, base_url: Option<&str>, start_timestamp: Option<i64>, end_timestamp: Option<i64>) -> GatewayResult<Value> {
    let query: Vec<String> = [("start_timestamp", start_timestamp), ("end_timestamp", end_timestamp)].iter().filter_map(|(name, value)| value.map(|v| format!("{name}={v}"))).collect();
    let path = if query.is_empty() { "/api/device/usage".to_string() } else { format!("/api/device/usage?{}", query.join("&")) };
    authed(layout, base_url, reqwest::Method::GET, &path, None, "gateway/usage").await
}

/// The balance top-up options, the plans on sale and the billing preference.
pub async fn billing(layout: &DataLayout, base_url: Option<&str>) -> GatewayResult<Value> {
    authed(layout, base_url, reqwest::Method::GET, "/api/device/billing", None, "gateway/billing").await
}

/// `quote` (`{amount}`), `topup` (`{amount}`: a checkout link) or
/// `subscribe` (`{plan_id, method}`: a checkout link, or bought from the
/// balance at once with `method: "balance"`).
pub async fn billing_post(layout: &DataLayout, base_url: Option<&str>, action: &str, body: Value) -> GatewayResult<Value> {
    authed(layout, base_url, reqwest::Method::POST, &format!("/api/device/billing/{action}"), Some(body), &format!("gateway/billing/{action}")).await
}

/// `{billing_preference}`: whether requests draw on the plan or the balance first.
pub async fn billing_preference(layout: &DataLayout, base_url: Option<&str>, body: Value) -> GatewayResult<Value> {
    authed(layout, base_url, reqwest::Method::PUT, "/api/device/billing/preference", Some(body), "gateway/billing/preference").await
}

/// The device key's price limits and spending cap.
pub async fn key_settings(layout: &DataLayout, base_url: Option<&str>) -> GatewayResult<Value> {
    authed(layout, base_url, reqwest::Method::GET, "/api/device/key", None, "gateway/key").await
}

/// `{price_limits?, unlimited_quota?, remain_quota?}`: changes the device
/// key's price limits and spending cap, leaving fields not given.
pub async fn update_key_settings(layout: &DataLayout, base_url: Option<&str>, body: Value) -> GatewayResult<Value> {
    authed(layout, base_url, reqwest::Method::PUT, "/api/device/key", Some(body), "gateway/key/update").await
}

/// Claims the welcome Context credit; the reply is the new `context_credit`
/// summary of `/api/device/self`.
pub async fn claim_context_credit(layout: &DataLayout, base_url: Option<&str>) -> GatewayResult<Value> {
    authed(layout, base_url, reqwest::Method::POST, "/api/device/context-credit/claim", Some(json!({})), "gateway/context_credit/claim").await
}

/// This machine's identity for the gateway's once-per-device rules
/// (`ctx_core::platform::device_identity`), or a random id kept in
/// `<data_dir>/device_id` on platforms without a hardware one.
pub fn device_identity(layout: &DataLayout) -> ctx_core::platform::DeviceIdentity {
    if let Some(identity) = ctx_core::platform::device_identity() {
        return identity;
    }
    let path = layout.device_id_file();
    let stored = std::fs::read_to_string(&path).ok().map(|s| s.trim().to_string()).filter(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()));
    let id = stored.unwrap_or_else(|| {
        let id = ctx_core::platform::hash_device_id(&uuid::Uuid::new_v4().to_string());
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = write_private_atomic(&path, &id);
        id
    });
    ctx_core::platform::DeviceIdentity { id, platform: "other", attested: false }
}

/// The `device_*` fields of the handshake and `identify` calls.
fn device_fields(identity: &ctx_core::platform::DeviceIdentity) -> Value {
    json!({"device_id": identity.id, "device_platform": identity.platform, "device_attested": identity.attested})
}

/// Opens a device authorization: `{device_code, user_code,
/// verification_uri, verification_uri_complete, expires_in, interval}`.
/// The machine's identity goes with it so the gateway can tie the key it
/// creates to this device.
pub async fn device_start(layout: &DataLayout, origin: &str, device_name: Option<String>) -> GatewayResult<Value> {
    let device_name = device_name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(host_name);
    let mut body = json!({"client_name": CLIENT_NAME, "device_name": device_name});
    if let (Some(body), Value::Object(fields)) = (body.as_object_mut(), device_fields(&device_identity(layout))) {
        body.extend(fields);
    }
    call(client()?.post(format!("{origin}/api/device/code")).json(&body), "gateway/device/start").await
}

/// `POST /api/device/identify`: attaches this machine's identity to the
/// stored key when its handshake carried none (a client updated past it);
/// the reply is the account's `device` block.
pub async fn identify(layout: &DataLayout, base_url: Option<&str>) -> GatewayResult<Value> {
    authed(layout, base_url, reqwest::Method::POST, "/api/device/identify", Some(device_fields(&device_identity(layout))), "gateway/device/identify").await
}

/// The Invite card: my code and link, my split, the terms, each referee
/// with their stage and earnings (`GET /api/device/referral`).
pub async fn referral(layout: &DataLayout, base_url: Option<&str>) -> GatewayResult<Value> {
    authed(layout, base_url, reqwest::Method::GET, "/api/device/referral", None, "gateway/referral").await
}

/// `{split_referee_cents}`: how much of the match pool future referees get.
pub async fn update_referral(layout: &DataLayout, base_url: Option<&str>, body: Value) -> GatewayResult<Value> {
    authed(layout, base_url, reqwest::Method::PUT, "/api/device/referral", Some(body), "gateway/referral/update").await
}

/// `{code}`: adds a referrer to a new account that signed up without one.
pub async fn attach_referral(layout: &DataLayout, base_url: Option<&str>, body: Value) -> GatewayResult<Value> {
    authed(layout, base_url, reqwest::Method::POST, "/api/device/referral/attach", Some(body), "gateway/referral/attach").await
}

/// `{status: "pending"|"approved"|"denied"|"expired", interval?, account?}`.
/// On approval the gateway's one-time key is written to the credential
/// store and taken out of the reply.
pub async fn device_poll(layout: &DataLayout, origin: &str, device_code: &str) -> GatewayResult<Value> {
    let body = json!({"device_code": device_code});
    let mut data = call(client()?.post(format!("{origin}/api/device/token")).json(&body), "gateway/device/poll").await?;
    if data.get("status").and_then(Value::as_str) == Some("approved") {
        let key = data.as_object_mut().and_then(|o| o.remove("api_key")).and_then(|v| v.as_str().map(|k| k.trim().to_string())).filter(|k| !k.is_empty());
        let Some(key) = key else {
            return Err(GatewayError::Gateway { message: "gateway/device/poll: the gateway approved the device but sent no API key".into(), code: None });
        };
        let mut store = CredentialStore::load(layout)?;
        store.set(CredentialStore::MODELGATEWAY, StoredCredential { api_key: key, origin: Some(origin.to_string()), account: account_name(&data), restore_base_url: None });
        store.save()?;
    }
    Ok(data)
}

/// The username in a poll reply's `account`.
pub fn account_name(poll: &Value) -> Option<String> {
    poll.get("account").and_then(|a| a.get("username")).and_then(Value::as_str).map(str::to_string)
}

/// Revokes the device's key on the gateway and forgets it locally. The
/// local credential goes even when the gateway cannot be reached; `false`
/// says the key may still exist there (or there was none to revoke).
pub async fn sign_out(layout: &DataLayout, base_url: Option<&str>) -> GatewayResult<bool> {
    let (origin, key) = match login(layout, base_url) {
        Ok(login) => login,
        Err(GatewayError::SignedOut) => return Ok(false),
        Err(e) => return Err(e),
    };
    let revoked = call(client()?.post(format!("{origin}/api/device/signout")).bearer_auth(key), "gateway/signout").await;
    let mut store = CredentialStore::load(layout)?;
    store.remove(CredentialStore::MODELGATEWAY);
    store.save()?;
    match revoked {
        Ok(_) => Ok(true),
        // The key is already gone on the gateway's side.
        Err(GatewayError::Unauthorized { .. }) => Ok(true),
        Err(e) => Err(e),
    }
}

/// Points the user configuration file's model endpoint at the gateway: its
/// `/v1` base URL, the stored credential as the key source and the project
/// header, the way the desktop app does on sign-in. The `model.base_url`
/// the file had before is kept with the credential for `restore_endpoint`.
pub fn use_gateway_endpoint(config: &Config, origin: &str) -> GatewayResult<()> {
    let layout = config.layout();
    let gateway_url = format!("{origin}/v1");
    edit_user_config(config, |model| {
        let previous = model.get("base_url").and_then(toml::Value::as_str).map(str::to_string);
        if previous.as_deref().and_then(self::origin).as_deref() != Some(origin) {
            let mut store = CredentialStore::load(&layout)?;
            if let Some(credential) = store.get(CredentialStore::MODELGATEWAY).cloned() {
                store.set(CredentialStore::MODELGATEWAY, StoredCredential { restore_base_url: previous, ..credential });
                store.save()?;
            }
        }
        model.insert("base_url".into(), toml::Value::String(gateway_url));
        model.insert("credential".into(), toml::Value::String(CredentialStore::MODELGATEWAY.into()));
        model.insert("project_header".into(), toml::Value::String(PROJECT_HEADER.into()));
        Ok(())
    })
}

/// Undoes `use_gateway_endpoint` when the file still points at the gateway
/// at `gateway_origin`: `restore_base_url` back (or the default endpoint
/// when there was none), credential and project header off.
pub fn restore_endpoint(config: &Config, gateway_origin: &str, restore_base_url: Option<String>) -> GatewayResult<()> {
    edit_user_config(config, |model| {
        let at_gateway = model.get("credential").and_then(toml::Value::as_str) == Some(CredentialStore::MODELGATEWAY)
            || model.get("base_url").and_then(toml::Value::as_str).and_then(origin).as_deref() == Some(gateway_origin);
        if !at_gateway {
            return Ok(());
        }
        match restore_base_url {
            Some(url) => model.insert("base_url".into(), toml::Value::String(url)),
            None => model.remove("base_url"),
        };
        model.remove("credential");
        model.remove("project_header");
        Ok(())
    })
}

/// Read-modify-validate-write of the `[model]` table of the user
/// configuration file (`<data_dir>/config.toml`).
fn edit_user_config(config: &Config, change: impl FnOnce(&mut toml::Table) -> GatewayResult<()>) -> GatewayResult<()> {
    let path = config.layout().config_file();
    let mut table: toml::Table = match std::fs::read_to_string(&path) {
        Ok(text) => text.parse().map_err(|e: toml::de::Error| CtxError::Config(format!("{}: {e}", path.display())))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(CtxError::from(e).into()),
    };
    let model = table.entry("model").or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let Some(model) = model.as_table_mut() else {
        return Err(CtxError::Config(format!("{}: `model` is not a table", path.display())).into());
    };
    change(model)?;
    let candidate: Config = toml::Value::Table(table.clone()).try_into().map_err(|e: toml::de::Error| CtxError::Config(format!("invalid configuration: {e}")))?;
    candidate.validate()?;
    let text = toml::to_string_pretty(&table).map_err(|e| CtxError::Config(format!("cannot serialize configuration: {e}")))?;
    write_private_atomic(&path, &text).map_err(CtxError::from)?;
    Ok(())
}

/// This computer's name for the gateway's key label ("Context · <host>").
fn host_name() -> String {
    ctx_core::platform::host_name().unwrap_or_else(|| format!("this {}", ctx_core::platform::machine_noun()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_strips_v1_and_slashes() {
        assert_eq!(origin("https://www.modelgateway.co/v1/").unwrap(), "https://www.modelgateway.co");
        assert_eq!(origin("https://www.modelgateway.co").unwrap(), "https://www.modelgateway.co");
        assert_eq!(origin("http://localhost:3000/").unwrap(), "http://localhost:3000");
        assert!(origin("modelgateway.co").is_none());
        assert!(origin("https://").is_none());
    }
}
