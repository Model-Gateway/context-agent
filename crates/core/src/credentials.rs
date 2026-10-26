//! Stored API credentials: `<data_dir>/credentials.toml` (mode 0600), one
//! named entry per account the user signed in to (for now the ModelGateway
//! device key, `CredentialStore::MODELGATEWAY`). `model.credential` names
//! the entry whose key a model uses instead of `model.api_key_env`, so a
//! signed-in key never has to pass through an environment variable.

use crate::config::{Config, ModelConfig};
use crate::error::{CtxError, Result};
use crate::paths::DataLayout;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

/// The API key a model authenticates with: the stored credential named by
/// `model.credential` when it exists and holds a key, else the environment
/// variable named by `model.api_key_env`.
pub fn resolve_api_key(config: &Config, model: &ModelConfig) -> Option<String> {
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
        let store = CredentialStore::load(&config.layout()).ok()?;
        let credential = store.get(CredentialStore::MODELGATEWAY)?;
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
}
