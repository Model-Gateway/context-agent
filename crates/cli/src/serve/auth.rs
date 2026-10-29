//! Forwarding the desktop app's sign-in to a `ctx serve` on a remote host
//! (docs/desktop-protocol.md "Remote hosts"), the way Claude Code's SSH
//! sessions use the account of the computer they were opened from:
//! `auth/export` on the app's local backend hands out the model endpoint
//! and the keys it uses; `auth/forward` on the host's backend keeps the
//! keys in memory (`ctx_core::Forwarded`, never written to the host's
//! disk) and, when the host has no key of its own, points its `[model]`
//! table at the same endpoint (no secret in the file).

use super::*;
use ctx_core::{CredentialStore, Forwarded, StoredCredential};

/// The endpoint part of `[model]` that travels with the keys: no secret.
#[derive(Serialize, Deserialize, Default)]
pub(super) struct ForwardedModel {
    pub(super) name: String,
    pub(super) base_url: String,
    pub(super) api_key_env: String,
    #[serde(default)]
    pub(super) credential: Option<String>,
    #[serde(default)]
    pub(super) project_header: Option<String>,
}

/// `auth/export`'s result and `auth/forward`'s params.
#[derive(Serialize, Deserialize, Default)]
pub(super) struct AuthBundle {
    #[serde(default)]
    pub(super) model: Option<ForwardedModel>,
    #[serde(default, flatten)]
    pub(super) secrets: Forwarded,
}

impl Server {
    /// `auth/export`: this backend's model endpoint and the keys its model
    /// chain (and the ModelGateway sign-in) use. Only the desktop app asks,
    /// to hand them to a remote host's backend; it stores none of it.
    pub(super) async fn auth_export(&self) -> RpcResult<Value> {
        let config = self.config.read().clone();
        let mut secrets = Forwarded::default();
        for model in config.model.chain() {
            let credential = model.credential.as_deref().map(str::trim).filter(|n| !n.is_empty());
            match credential.and_then(|name| ctx_core::credential_with_key(&config, name).map(|c| (name, c))) {
                Some((name, c)) => {
                    secrets.credentials.insert(name.to_string(), forwardable(c));
                }
                None => {
                    if let Some(key) = ctx_core::resolve_api_key(&config, &model).filter(|_| !model.api_key_env.is_empty()) {
                        secrets.env.insert(model.api_key_env.clone(), key);
                    }
                }
            }
        }
        if let Some(c) = ctx_core::credential_with_key(&config, CredentialStore::MODELGATEWAY) {
            secrets.credentials.entry(CredentialStore::MODELGATEWAY.to_string()).or_insert_with(|| forwardable(c));
        }
        let m = &config.model;
        let model = ForwardedModel { name: m.name.clone(), base_url: m.base_url.clone(), api_key_env: m.api_key_env.clone(), credential: m.credential.clone(), project_header: m.project_header.clone() };
        to_value(&AuthBundle { model: Some(model), secrets })
    }

    /// `auth/forward`: keeps the forwarded keys for this process (replacing
    /// any forwarded before). When the host's own configuration has no key
    /// for its model and the forwarded keys cover the forwarded endpoint,
    /// `[model]` follows that endpoint (`model.name` too when the endpoint
    /// changes; a model chosen on the same endpoint stays). Returns
    /// `initialize`'s result with `model.key_forwarded`.
    pub(super) async fn auth_forward(self: &Arc<Self>, params: AuthBundle) -> RpcResult<Value> {
        let _serialized = self.patch_lock.lock().await;
        let config = self.config.read().clone();
        let has_own_key = ctx_core::resolve_own_api_key(&config, &config.model).is_some();
        Forwarded::install(params.secrets);
        self.models_cache.lock().clear();
        if let Some(model) = params.model.filter(|_| !has_own_key) {
            let mut probe = config.model.clone();
            probe.base_url = model.base_url.clone();
            probe.api_key_env = model.api_key_env.clone();
            probe.credential = model.credential.clone();
            let same_endpoint = config.model.base_url.trim_end_matches('/') == model.base_url.trim_end_matches('/');
            let already = same_endpoint && config.model.api_key_env == model.api_key_env && config.model.credential == model.credential && config.model.project_header == model.project_header;
            if !already && model.base_url.trim().len() > 0 && ctx_core::resolve_api_key(&config, &probe).is_some() {
                let mut values = serde_json::Map::new();
                values.insert("model.base_url".into(), json!(model.base_url));
                values.insert("model.api_key_env".into(), json!(model.api_key_env));
                values.insert("model.credential".into(), model.credential.map_or(Value::Null, Value::from));
                values.insert("model.project_header".into(), model.project_header.map_or(Value::Null, Value::from));
                if !same_endpoint && !model.name.trim().is_empty() {
                    values.insert("model.name".into(), json!(model.name));
                }
                let server = self.clone();
                blocking_rpc(move || server.apply_config_patch(ConfigPatchParams { values })).await?;
                self.log("info", format!("using the forwarded sign-in for {}", self.config.read().model.base_url));
            }
        }
        to_value(&self.config_info())
    }
}

/// The parts of a stored credential another machine needs.
fn forwardable(c: StoredCredential) -> StoredCredential {
    StoredCredential { api_key: c.api_key, origin: c.origin, account: c.account, restore_base_url: None }
}
