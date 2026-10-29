//! Stored API credentials: `<data_dir>/credentials.toml` (mode 0600), one
//! named entry per account the user signed in to (for now the ModelGateway
//! device key, `CredentialStore::MODELGATEWAY`). `model.credential` names
//! the entry whose key a model uses instead of `model.api_key_env`, so a
//! signed-in key never has to pass through an environment variable.
//!
//! A `ctx serve` on a remote host can also hold the desktop app's sign-in
//! for the session ([`Forwarded`], `auth/forward`): credentials and key
//! variables the host does not have itself, kept in memory and never
//! written to the host's disk.

use crate::config::{Config, ModelConfig};
use crate::error::{CtxError, Result};
use crate::paths::DataLayout;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StoredCredential {
    pub api_key: String,
    /// The service the key belongs to (its origin), for display and sanity checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// The signed-in account (a username), for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// The user configuration file's `model.base_url` from before the TUI's
    /// `/signin` pointed it at the service; `/signout` puts it back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore_base_url: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CredentialStore {
    path: PathBuf,
    entries: BTreeMap<String, StoredCredential>,
}

impl CredentialStore {
    /// The ModelGateway device key written by `gateway/device/poll`.
    pub const MODELGATEWAY: &'static str = "modelgateway";

    pub fn path(layout: &DataLayout) -> PathBuf {
        layout.root.join("credentials.toml")
    }

    /// The store on disk; a missing file is an empty store.
    pub fn load(layout: &DataLayout) -> Result<Self> {
        let path = Self::path(layout);
        let entries = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).map_err(|e| CtxError::Config(format!("{}: {e}", path.display())))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(CtxError::Config(format!("{}: {e}", path.display()))),
        };
        Ok(Self { path, entries })
    }

    pub fn get(&self, name: &str) -> Option<&StoredCredential> {
        self.entries.get(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    pub fn set(&mut self, name: &str, credential: StoredCredential) {
        self.entries.insert(name.to_string(), credential);
    }

    pub fn remove(&mut self, name: &str) -> Option<StoredCredential> {
        self.entries.remove(name)
    }

    /// Writes the store atomically (temp file + rename) with mode 0600.
    pub fn save(&self) -> Result<()> {
        let text = toml::to_string_pretty(&self.entries).map_err(|e| CtxError::Config(format!("cannot serialise credentials: {e}")))?;
        write_private(&self.path, &text).map_err(|e| CtxError::Config(format!("{}: {e}", self.path.display())))
    }
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        std::io::Write::write_all(&mut file, text.as_bytes())?;
        file.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&temp, path)
}

/// The sign-in a desktop app forwarded to this process (`auth/forward` of
/// `ctx serve` on a remote host): stored credentials by name and values of
/// key variables (`OPENAI_API_KEY`). Process memory only; it fills gaps,
/// so the host's own `credentials.toml` and environment win.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Forwarded {
    #[serde(default)]
    pub credentials: BTreeMap<String, StoredCredential>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl Forwarded {
    fn slot() -> &'static RwLock<Forwarded> {
        static SLOT: OnceLock<RwLock<Forwarded>> = OnceLock::new();
        SLOT.get_or_init(Default::default)
    }

    /// Replaces the process's forwarded sign-in (empty values dropped).
    pub fn install(mut forwarded: Forwarded) {
        forwarded.credentials.retain(|_, c| !c.api_key.trim().is_empty());
        forwarded.env.retain(|_, v| !v.trim().is_empty());
        *Self::slot().write().unwrap_or_else(|e| e.into_inner()) = forwarded;
    }

    /// Forgets the forwarded credential `name`; whether there was one.
    pub fn remove_credential(name: &str) -> bool {
        Self::slot().write().unwrap_or_else(|e| e.into_inner()).credentials.remove(name).is_some()
    }

    pub fn credential(name: &str) -> Option<StoredCredential> {
        Self::slot().read().unwrap_or_else(|e| e.into_inner()).credentials.get(name).cloned()
    }

    pub fn env(name: &str) -> Option<String> {
        Self::slot().read().unwrap_or_else(|e| e.into_inner()).env.get(name).map(|v| v.trim().to_string())
    }

    /// Every forwarded secret, for redactors.
    pub fn secrets() -> Vec<String> {
        let slot = Self::slot().read().unwrap_or_else(|e| e.into_inner());
        slot.credentials.values().map(|c| c.api_key.trim().to_string()).chain(slot.env.values().map(|v| v.trim().to_string())).collect()
    }
}

/// The credential `name` with a key: the host's own store first, then the
/// forwarded sign-in.
pub fn credential_with_key(config: &Config, name: &str) -> Option<StoredCredential> {
    let own = CredentialStore::load(&config.layout()).ok().and_then(|store| store.get(name).cloned()).filter(|c| !c.api_key.trim().is_empty());
    own.or_else(|| Forwarded::credential(name))
}

/// The API key a model authenticates with: the stored credential named by
/// `model.credential` when it exists and holds a key, else the environment
/// variable named by `model.api_key_env` (a forwarded value when the host
/// has none).
pub fn resolve_api_key(config: &Config, model: &ModelConfig) -> Option<String> {
    if let Some(name) = model.credential.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        if let Some(credential) = credential_with_key(config, name) {
            return Some(credential.api_key.trim().to_string());
        }
    }
    model.api_key().or_else(|| if model.api_key_env.is_empty() { None } else { Forwarded::env(&model.api_key_env) })
}

/// [`resolve_api_key`] without the forwarded sign-in: whether the host has
/// a key of its own.
pub fn resolve_own_api_key(config: &Config, model: &ModelConfig) -> Option<String> {
    if let Some(name) = model.credential.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        if let Ok(store) = CredentialStore::load(&config.layout()) {
            if let Some(key) = store.get(name).map(|c| c.api_key.trim()).filter(|k| !k.is_empty()) {
                return Some(key.to_string());
            }
        }
    }
    model.api_key()
}

/// A signed-in ModelGateway account, for tools that call the gateway
/// directly (`web_search`): its origin (no `/v1`), the device key, and the
/// headers model requests carry too (the project header).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayAccess {
    pub origin: String,
    pub api_key: String,
    pub headers: BTreeMap<String, String>,
}

impl GatewayAccess {
    /// The stored ModelGateway credential, when it has a key and an http(s)
    /// origin; `project` fills `model.project_header` like model requests.
    pub fn load(config: &Config, project: &str) -> Option<GatewayAccess> {
        let credential = credential_with_key(config, CredentialStore::MODELGATEWAY)?;
        let api_key = credential.api_key.trim().to_string();
        let origin = credential.origin.as_deref()?.trim().trim_end_matches('/');
        let origin = origin.strip_suffix("/v1").unwrap_or(origin).to_string();
        if api_key.is_empty() || !(origin.starts_with("https://") || origin.starts_with("http://")) {
            return None;
        }
        let mut headers = BTreeMap::new();
        if let Some(name) = config.model.project_header.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            if let Some(value) = config.model.for_project(project).extra_headers.get(name) {
                headers.insert(name.to_string(), value.clone());
            }
        }
        Some(GatewayAccess { origin, api_key, headers })
    }
}

/// "credential `modelgateway`" or "env OPENAI_API_KEY": where a model's key comes from, for reports.
pub fn api_key_source(model: &ModelConfig) -> String {
    match model.credential.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => format!("credential `{name}`"),
        None => format!("env {}", model.api_key_env),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_resolves() {
        let dir = tempfile::tempdir().unwrap();
        let layout = DataLayout::new(dir.path().to_path_buf());
        let mut store = CredentialStore::load(&layout).unwrap();
        assert!(store.get(CredentialStore::MODELGATEWAY).is_none());
        store.set(CredentialStore::MODELGATEWAY, StoredCredential { api_key: "sk-test".into(), origin: Some("https://g.example".into()), account: Some("alex".into()), restore_base_url: None });
        store.save().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(CredentialStore::path(&layout)).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let reloaded = CredentialStore::load(&layout).unwrap();
        assert_eq!(reloaded.get(CredentialStore::MODELGATEWAY).unwrap().account.as_deref(), Some("alex"));

        let mut config = Config::default();
        config.data_dir = dir.path().to_path_buf();
        config.model.api_key_env = "CTX_CREDENTIALS_TEST_UNSET".into();
        assert_eq!(resolve_api_key(&config, &config.model), None);
        config.model.credential = Some(CredentialStore::MODELGATEWAY.into());
        assert_eq!(resolve_api_key(&config, &config.model).as_deref(), Some("sk-test"));
        config.model.credential = Some("missing".into());
        assert_eq!(resolve_api_key(&config, &config.model), None, "an unknown credential falls back to the environment");

        config.model.project_header = Some("X-ModelGateway-Project".into());
        let access = GatewayAccess::load(&config, "context-agent").unwrap();
        assert_eq!((access.origin.as_str(), access.api_key.as_str()), ("https://g.example", "sk-test"));
        assert_eq!(access.headers.get("X-ModelGateway-Project").map(String::as_str), Some("context-agent"));
        let search = crate::WebSearchConfig::default();
        assert!(matches!(search.resolve(Some(&access)).unwrap(), crate::SearchBackend::Gateway(_)), "auto prefers the signed-in gateway");
        let explicit = crate::WebSearchConfig { provider: crate::WebSearchProvider::Gateway, ..Default::default() };
        assert!(explicit.resolve(None).is_err(), "an explicit gateway provider needs a sign-in");
    }

    #[test]
    fn forwarded_sign_in_fills_gaps_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.data_dir = dir.path().to_path_buf();
        config.model.credential = Some("forwarded-test".into());
        config.model.api_key_env = "CTX_FORWARDED_TEST_UNSET".into();
        assert_eq!(resolve_api_key(&config, &config.model), None);
        let mut forwarded = Forwarded::default();
        forwarded.credentials.insert("forwarded-test".into(), StoredCredential { api_key: "sk-forwarded".into(), ..Default::default() });
        forwarded.env.insert("CTX_FORWARDED_TEST_UNSET".into(), "sk-env-forwarded".into());
        Forwarded::install(forwarded);
        assert_eq!(resolve_api_key(&config, &config.model).as_deref(), Some("sk-forwarded"));
        assert_eq!(resolve_own_api_key(&config, &config.model), None, "the forwarded key is not the host's own");
        let mut store = CredentialStore::load(&config.layout()).unwrap();
        store.set("forwarded-test", StoredCredential { api_key: "sk-own".into(), ..Default::default() });
        store.save().unwrap();
        assert_eq!(resolve_api_key(&config, &config.model).as_deref(), Some("sk-own"), "the host's own credential wins");
        assert!(!std::fs::read_to_string(CredentialStore::path(&config.layout())).unwrap().contains("sk-forwarded"), "never written to disk");
        config.model.credential = None;
        assert_eq!(resolve_api_key(&config, &config.model).as_deref(), Some("sk-env-forwarded"));
        assert!(Forwarded::secrets().contains(&"sk-forwarded".to_string()));
        Forwarded::install(Forwarded::default());
    }
}
