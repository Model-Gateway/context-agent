//! ModelGateway's per-model numbers for the model pickers (the TUI's
//! `/model` and the app's `models/list`): how far below the official list
//! price a model is billed, and its error rate over the last 24 hours. Both
//! come from the gateway's public endpoints (`/api/pricing?view=summary`,
//! `/api/perf-metrics/summary`) and are cached in
//! `<data_dir>/model-stats.json` for an hour, so opening a picker rarely
//! touches the network. Only shown while signed in to ModelGateway; other
//! endpoints, and a modelgateway host used with your own key, get no stats.

use ctx_core::{Config, CredentialStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long a complete fetch is reused.
const FRESH_FOR: Duration = Duration::from_secs(60 * 60);
/// How long a failed (or half-failed) fetch waits before the next try.
const RETRY_AFTER: Duration = Duration::from_secs(5 * 60);
const TIMEOUT: Duration = Duration::from_secs(10);
const CACHE_FILE: &str = "model-stats.json";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelStat {
    /// Percent below the official output price (0–100), when the gateway knows the official price.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub savings_percent: Option<f64>,
    /// Percent of requests that failed over the last 24 hours; none without traffic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_rate_24h: Option<f64>,
}

impl ModelStat {
    /// "98% off"; none below 1%.
    pub fn savings_label(&self) -> Option<String> {
        self.savings_percent.filter(|p| p.round() >= 1.0).map(|p| format!("{p:.0}% off"))
    }

    /// "1.5%", "0%", "100%".
    pub fn error_rate_label(&self) -> Option<String> {
        let rounded = (self.error_rate_24h? * 10.0).round() / 10.0;
        Some(if rounded.fract() == 0.0 { format!("{rounded:.0}%") } else { format!("{rounded:.1}%") })
    }
}

/// Model id → its stats.
pub type ModelStats = BTreeMap<String, ModelStat>;

#[derive(Serialize, Deserialize)]
struct CacheFile {
    origin: String,
    /// Unix seconds of the last fetch attempt.
    fetched_at: u64,
    /// Both endpoints answered on that attempt.
    complete: bool,
    models: ModelStats,
}

/// The gateway origin (no `/v1`) when the configured model is served by
/// ModelGateway with its signed-in credential.
pub fn gateway_origin(config: &Config) -> Option<String> {
    if config.model.credential.as_deref().map(str::trim) != Some(CredentialStore::MODELGATEWAY) {
        return None;
    }
    let trimmed = config.model.base_url.trim().trim_end_matches('/');
    let origin = trimmed.strip_suffix("/v1").unwrap_or(trimmed);
    (origin.starts_with("https://") || origin.starts_with("http://")).then(|| origin.to_string())
}

/// The stats for the configured endpoint: the disk cache while fresh, else
/// a fetch (both endpoints at once). A failed fetch keeps the last values
/// and is retried after a few minutes. Never fails; empty when unknown.
pub async fn load(config: &Config) -> ModelStats {
    let Some(origin) = gateway_origin(config) else { return ModelStats::new() };
    let path = config.layout().root.join(CACHE_FILE);
    let cached = read_cache(&path).filter(|c| c.origin == origin);
    if let Some(cache) = &cached {
        let max_age = if cache.complete { FRESH_FOR } else { RETRY_AFTER };
        if now().saturating_sub(cache.fetched_at) < max_age.as_secs() {
            return cache.models.clone();
        }
    }

    let (savings, errors) = fetch(&origin).await;
    let complete = savings.is_some() && errors.is_some();
    let mut models = cached.map(|c| c.models).unwrap_or_default();
    if let Some(savings) = savings {
        models.values_mut().for_each(|s| s.savings_percent = None);
        for (name, percent) in savings {
            models.entry(name).or_default().savings_percent = Some(percent);
        }
    }
    if let Some(errors) = errors {
        models.values_mut().for_each(|s| s.error_rate_24h = None);
        for (name, rate) in errors {
            models.entry(name).or_default().error_rate_24h = Some(rate);
        }
    }
    models.retain(|_, s| s.savings_percent.is_some() || s.error_rate_24h.is_some());
    let cache = CacheFile { origin, fetched_at: now(), complete, models };
    if let Ok(text) = serde_json::to_vec(&cache) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
    cache.models
}

fn read_cache(path: &Path) -> Option<CacheFile> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `(savings, error rates)`; each `None` when its endpoint failed.
async fn fetch(origin: &str) -> (Option<Vec<(String, f64)>>, Option<Vec<(String, f64)>>) {
    let Ok(client) = reqwest::Client::builder().timeout(TIMEOUT).user_agent(format!("context-agent/{}", env!("CARGO_PKG_VERSION"))).build() else {
        return (None, None);
    };
    let (pricing_url, health_url) = (format!("{origin}/api/pricing?view=summary"), format!("{origin}/api/perf-metrics/summary?hours=24"));
    let (pricing, health) = tokio::join!(get(&client, &pricing_url), get(&client, &health_url));
    (pricing.map(|v| parse_savings(&v)), health.map(|v| parse_error_rates(&v)))
}

/// The JSON body of a `{success: true, …}` reply.
async fn get(client: &reqwest::Client, url: &str) -> Option<Value> {
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body: Value = response.json().await.ok()?;
    (body.get("success").and_then(Value::as_bool) == Some(true)).then_some(body)
}

/// `data[]` of `/api/pricing`: `model_name` and `discount_percent`.
fn parse_savings(body: &Value) -> Vec<(String, f64)> {
    let rows = body.get("data").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    rows.iter().filter_map(|m| Some((m.get("model_name")?.as_str()?.to_string(), m.get("discount_percent")?.as_f64()?))).collect()
}

/// `data.windows["24h"].models[]` (else `data.models[]`) of
/// `/api/perf-metrics/summary`: `model_name` and `error_rate`, for models
/// that served requests.
fn parse_error_rates(body: &Value) -> Vec<(String, f64)> {
    let data = body.get("data").unwrap_or(&Value::Null);
    let rows = data.pointer("/windows/24h/models").or_else(|| data.get("models")).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    rows.iter()
        .filter(|m| m.get("request_count").and_then(Value::as_u64).unwrap_or(0) > 0)
        .filter_map(|m| Some((m.get("model_name")?.as_str()?.to_string(), m.get("error_rate")?.as_f64()?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_signed_in_modelgateway_endpoints_have_stats() {
        let mut config = Config::default();
        config.model.base_url = "https://www.modelgateway.co/v1".into();
        assert_eq!(gateway_origin(&config), None);
        config.model.credential = Some(CredentialStore::MODELGATEWAY.into());
        assert_eq!(gateway_origin(&config).as_deref(), Some("https://www.modelgateway.co"));
        config.model.base_url = "http://localhost:3000/v1/".into();
        assert_eq!(gateway_origin(&config).as_deref(), Some("http://localhost:3000"));
    }

    #[test]
    fn parses_discounts_and_24h_error_rates() {
        let pricing = json!({"success": true, "data": [{"model_name": "a", "discount_percent": 98.0}, {"model_name": "b"}]});
        assert_eq!(parse_savings(&pricing), vec![("a".to_string(), 98.0)]);
        let perf = json!({"success": true, "data": {"models": [{"model_name": "x", "error_rate": 50.0, "request_count": 2}], "windows": {"24h": {"models": [
            {"model_name": "a", "error_rate": 1.5, "request_count": 10},
            {"model_name": "idle", "error_rate": 0.0, "request_count": 0}
        ]}}}});
        assert_eq!(parse_error_rates(&perf), vec![("a".to_string(), 1.5)]);
        assert_eq!(ModelStat { savings_percent: Some(97.6), error_rate_24h: None }.savings_label().as_deref(), Some("98% off"));
        assert_eq!(ModelStat { savings_percent: Some(0.2), error_rate_24h: None }.savings_label(), None);
    }
}
