//! models.dev metadata (https://models.dev/api.json) for the `/model`
//! picker's detail card: a model's name, lab, one-line description, limits,
//! capability flags and price. The same lookups as the desktop app's
//! `ModelsDevCatalog`. The raw api.json is cached in
//! `<data_dir>/models-dev.json` for a day; parsing runs off the UI thread.

use ctx_core::Config;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

const API_URL: &str = "https://models.dev/api.json";
const FRESH_FOR: Duration = Duration::from_secs(24 * 60 * 60);
const TIMEOUT: Duration = Duration::from_secs(30);
const CACHE_FILE: &str = "models-dev.json";

/// What the detail card shows for one model.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub lab_name: String,
    pub context_window: Option<u64>,
    pub max_output: Option<u64>,
    pub tool_call: bool,
    pub reasoning: bool,
    /// The `reasoning_effort` values the model accepts, weakest first:
    /// empty without effort control, `None` when models.dev does not say.
    pub effort_levels: Option<Vec<String>>,
    pub vision: bool,
    pub open_weights: bool,
    pub knowledge: Option<String>,
    /// USD per million tokens.
    pub input_cost: Option<f64>,
    pub output_cost: Option<f64>,
    pub cache_read_cost: Option<f64>,
}

impl ModelInfo {
    /// "$2 in · $8 out per M"; none without a price.
    pub fn cost_line(&self) -> Option<String> {
        let money = |v: f64| if v.round() == v { format!("${v:.0}") } else { format!("${v:.2}") };
        Some(format!("{} in · {} out per M", money(self.input_cost?), money(self.output_cost?)))
    }
}

/// "1M", "1.5M", "200K", "32K" (powers of two read as such).
pub fn fmt_limit(count: u64) -> String {
    if count >= 1_000_000 {
        let millions = format!("{:.1}", count as f64 / 1_000_000.0);
        return format!("{}M", millions.strip_suffix(".0").unwrap_or(&millions));
    }
    if count >= 1_000 {
        return format!("{}K", if count % 1_024 == 0 { count / 1_024 } else { (count as f64 / 1_000.0).round() as u64 });
    }
    count.to_string()
}

/// The index from the disk cache while fresh, else from models.dev (and
/// written back). A failed fetch falls back to a stale cache; `None` when
/// there is neither.
pub async fn load(config: &Config) -> Option<ModelsDevIndex> {
    let path = config.layout().root.join(CACHE_FILE);
    let age = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok());
    if age.is_some_and(|a| a < FRESH_FOR) {
        if let Some(index) = parse_file(path.clone()).await {
            return Some(index);
        }
    }
    if let Some(data) = fetch().await {
        let index = tokio::task::spawn_blocking(move || {
            let index = ModelsDevIndex::parse(&data)?;
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, &data).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
            Some(index)
        })
        .await
        .ok()
        .flatten();
        if index.is_some() {
            return index;
        }
    }
    parse_file(config.layout().root.join(CACHE_FILE)).await
}

async fn parse_file(path: std::path::PathBuf) -> Option<ModelsDevIndex> {
    tokio::task::spawn_blocking(move || ModelsDevIndex::parse(&std::fs::read(Path::new(&path)).ok()?)).await.ok().flatten()
}

async fn fetch() -> Option<Vec<u8>> {
    let client = reqwest::Client::builder().timeout(TIMEOUT).user_agent(format!("context-agent/{}", env!("CARGO_PKG_VERSION"))).build().ok()?;
    let response = client.get(API_URL).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.bytes().await.ok().map(|b| b.to_vec())
}

/// One model entry as listed by one provider.
#[derive(Debug, Clone)]
struct Raw {
    id: String,
    name: Option<String>,
    description: Option<String>,
    canonical: Option<String>,
    context_window: Option<u64>,
    max_output: Option<u64>,
    tool_call: bool,
    reasoning: bool,
    effort_levels: Option<Vec<String>>,
    vision: bool,
    open_weights: bool,
    knowledge: Option<String>,
    input_cost: Option<f64>,
    output_cost: Option<f64>,
    cache_read_cost: Option<f64>,
}

impl Raw {
    fn new(key: &str, m: &Map<String, Value>) -> Self {
        let s = |k: &str| m.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        let flag = |k: &str| m.get(k).and_then(Value::as_bool).unwrap_or(false);
        let limit = |k: &str| m.get("limit").and_then(|l| l.get(k)).and_then(Value::as_u64);
        let cost = |k: &str| m.get("cost").and_then(|c| c.get(k)).and_then(Value::as_f64);
        // `reasoning_options: [{"type": "effort", "values": [...]}, {"type": "toggle"}, …]`;
        // a model that does not reason has no effort to set.
        let effort_levels = match m.get("reasoning_options").and_then(Value::as_array) {
            Some(options) => Some(
                options
                    .iter()
                    .filter(|o| o.get("type").and_then(Value::as_str) == Some("effort"))
                    .flat_map(|o| o.get("values").and_then(Value::as_array).into_iter().flatten())
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect(),
            ),
            None if !flag("reasoning") => Some(Vec::new()),
            None => None,
        };
        let images = m.get("modalities").and_then(|v| v.get("input")).and_then(Value::as_array).is_some_and(|i| i.iter().any(|v| v.as_str() == Some("image")));
        Raw {
            id: s("id").unwrap_or_else(|| key.to_string()),
            name: s("name"),
            description: s("description"),
            canonical: s("canonical_model_id"),
            context_window: limit("context"),
            max_output: limit("output"),
            tool_call: flag("tool_call"),
            reasoning: flag("reasoning"),
            effort_levels,
            vision: images || flag("attachment"),
            open_weights: flag("open_weights"),
            knowledge: s("knowledge"),
            input_cost: cost("input"),
            output_cost: cost("output"),
            cache_read_cost: cost("cache_read"),
        }
    }
}

struct Provider {
    name: String,
    api: Option<String>,
    models: HashMap<String, Raw>,
}

/// The parsed api.json, keyed for the lookups `info` tries.
pub struct ModelsDevIndex {
    providers: HashMap<String, Provider>,
    /// Normalized API base URL → provider id.
    providers_by_api: HashMap<String, String>,
    /// Lowercased bare model id → "lab/id" from every `canonical_model_id`.
    canonical_by_bare_id: HashMap<String, String>,
    /// Lowercased model id → (provider, model) of an entry without a
    /// canonical link (a first-party listing), native-SDK providers first.
    first_party_by_id: HashMap<String, (String, String)>,
    /// Lowercased model id → (provider, model) of any listing.
    any_by_id: HashMap<String, (String, String)>,
}

impl ModelsDevIndex {
    pub fn parse(data: &[u8]) -> Option<Self> {
        let root: Map<String, Value> = serde_json::from_slice(data).ok()?;
        let mut providers = HashMap::new();
        let mut providers_by_api = HashMap::new();
        for (provider_id, value) in &root {
            let Some(models) = value.get("models").and_then(Value::as_object) else { continue };
            let models = models.iter().filter_map(|(k, v)| Some((k.clone(), Raw::new(k, v.as_object()?)))).collect();
            let api = value.get("api").and_then(Value::as_str).map(str::to_string);
            if let Some(api) = &api {
                providers_by_api.insert(normalized_url(api), provider_id.clone());
            }
            let name = value.get("name").and_then(Value::as_str).unwrap_or(provider_id).to_string();
            providers.insert(provider_id.clone(), Provider { name, api, models });
        }
        if providers.is_empty() {
            return None;
        }
        let mut index = ModelsDevIndex { providers, providers_by_api, canonical_by_bare_id: HashMap::new(), first_party_by_id: HashMap::new(), any_by_id: HashMap::new() };
        // Deterministic order: native-SDK providers (no `api`) win ties.
        let mut ordered: Vec<&String> = index.providers.keys().collect();
        ordered.sort_by_key(|id| (index.providers[*id].api.is_some(), id.as_str()));
        for provider_id in ordered {
            let provider = &index.providers[provider_id];
            let mut keys: Vec<&String> = provider.models.keys().collect();
            keys.sort();
            for key in keys {
                let raw = &provider.models[key];
                if let Some((lab, bare)) = raw.canonical.as_deref().and_then(|c| c.split_once('/')) {
                    if index.providers.get(lab).is_some_and(|p| p.models.contains_key(bare)) {
                        index.canonical_by_bare_id.entry(bare.to_lowercase()).or_insert_with(|| format!("{lab}/{bare}"));
                    }
                } else {
                    index.first_party_by_id.entry(key.to_lowercase()).or_insert_with(|| (provider_id.clone(), key.clone()));
                }
                index.any_by_id.entry(key.to_lowercase()).or_insert_with(|| (provider_id.clone(), key.clone()));
            }
        }
        Some(index)
    }

    /// 1. the provider whose API base URL is the endpoint's; 2. "lab/id"
    /// ids; 3. a canonical lab entry for the bare id; 4. any first-party
    /// listing; 5. any listing at all.
    pub fn info(&self, model_id: &str, base_url: &str) -> Option<ModelInfo> {
        let id = model_id.trim();
        if id.is_empty() {
            return None;
        }
        if let Some(provider_id) = self.providers_by_api.get(&normalized_url(base_url)) {
            if let Some(raw) = self.lookup(provider_id, id) {
                return Some(self.resolve(raw, provider_id));
            }
        }
        if let Some((lab, bare)) = id.split_once('/') {
            if let Some(raw) = self.lookup(lab, bare) {
                return Some(self.resolve(raw, lab));
            }
            return self.info(bare, "");
        }
        if let Some((lab, bare)) = self.canonical_by_bare_id.get(&id.to_lowercase()).and_then(|c| c.split_once('/')) {
            if let Some(raw) = self.lookup(lab, bare) {
                return Some(self.resolve(raw, lab));
            }
        }
        for table in [&self.first_party_by_id, &self.any_by_id] {
            if let Some((provider_id, key)) = table.get(&id.to_lowercase()) {
                if let Some(raw) = self.providers.get(provider_id).and_then(|p| p.models.get(key)) {
                    return Some(self.resolve(raw, provider_id));
                }
            }
        }
        None
    }

    fn lookup(&self, provider_id: &str, id: &str) -> Option<&Raw> {
        let models = &self.providers.get(provider_id)?.models;
        models.get(id).or_else(|| models.iter().find(|(k, _)| k.eq_ignore_ascii_case(id)).map(|(_, v)| v))
    }

    /// Fills gaps from the canonical lab entry; the lab is the canonical one.
    fn resolve(&self, raw: &Raw, provider_id: &str) -> ModelInfo {
        let mut lab_id = provider_id;
        let mut lab: Option<&Raw> = None;
        if let Some((lab_key, bare)) = raw.canonical.as_deref().and_then(|c| c.split_once('/')) {
            if let Some(entry) = self.lookup(lab_key, bare) {
                lab_id = lab_key;
                lab = Some(entry);
            }
        }
        let name = raw.name.clone().filter(|n| *n != raw.id).or_else(|| lab.and_then(|l| l.name.clone())).map(|n| n.strip_suffix(" (latest)").map(str::to_string).unwrap_or(n)).unwrap_or_else(|| raw.id.clone());
        ModelInfo {
            id: raw.id.clone(),
            name,
            description: raw.description.clone().or_else(|| lab.and_then(|l| l.description.clone())),
            lab_name: self.providers.get(lab_id).map(|p| p.name.clone()).unwrap_or_else(|| lab_id.to_string()),
            context_window: raw.context_window.or(lab.and_then(|l| l.context_window)),
            max_output: raw.max_output.or(lab.and_then(|l| l.max_output)),
            tool_call: raw.tool_call,
            reasoning: raw.reasoning,
            // The endpoint's own listing may accept fewer values than the lab's.
            effort_levels: raw.effort_levels.clone().or_else(|| lab.and_then(|l| l.effort_levels.clone())),
            vision: raw.vision,
            open_weights: raw.open_weights || lab.is_some_and(|l| l.open_weights),
            knowledge: raw.knowledge.clone().or_else(|| lab.and_then(|l| l.knowledge.clone())),
            input_cost: raw.input_cost,
            output_cost: raw.output_cost,
            cache_read_cost: raw.cache_read_cost,
        }
    }
}

impl ctx_core::EffortLookup for ModelsDevIndex {
    fn effort_levels(&self, model: &str, base_url: &str) -> Option<Vec<String>> {
        self.info(model, base_url)?.effort_levels
    }
}

/// Lowercased, scheme and trailing slashes dropped.
fn normalized_url(url: &str) -> String {
    let s = url.trim().to_lowercase();
    let s = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")).unwrap_or(&s);
    s.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const API: &str = r#"{
        "openai": {"name": "OpenAI", "models": {"gpt-4.1": {"id": "gpt-4.1", "name": "GPT-4.1", "description": "Workhorse", "tool_call": true,
            "modalities": {"input": ["text", "image"]}, "limit": {"context": 1047576, "output": 32768}, "knowledge": "2024-04", "cost": {"input": 2, "output": 8}}}},
        "moonshotai": {"name": "Moonshot AI", "models": {"kimi-k2": {"id": "kimi-k2", "name": "Kimi K2", "open_weights": true, "tool_call": true}}},
        "anthropic": {"name": "Anthropic", "models": {"claude-opus-5": {"id": "claude-opus-5", "reasoning": true,
            "reasoning_options": [{"type": "effort", "values": ["low", "medium", "high", "xhigh", "max"]}]}}},
        "reseller": {"name": "Reseller", "api": "https://api.reseller.dev/v1/", "models": {
            "gpt-4.1": {"id": "gpt-4.1", "name": "gpt-4.1", "canonical_model_id": "openai/gpt-4.1", "cost": {"input": 1.5, "output": 6}},
            "opus": {"id": "opus", "reasoning": true, "canonical_model_id": "anthropic/claude-opus-5"},
            "opus-lite": {"id": "opus-lite", "reasoning": true, "canonical_model_id": "anthropic/claude-opus-5", "reasoning_options": [{"type": "toggle"}, {"type": "effort", "values": ["low", "high"]}]},
            "thinker": {"id": "thinker", "reasoning": true, "reasoning_options": [{"type": "budget_tokens", "min": 1024}]},
            "kimi-fast": {"id": "kimi-fast", "name": "Kimi Fast", "canonical_model_id": "moonshotai/kimi-k2"}}}
    }"#;

    #[test]
    fn lookups_follow_the_endpoint_then_the_lab() {
        let index = ModelsDevIndex::parse(API.as_bytes()).unwrap();
        let direct = index.info("gpt-4.1", "https://api.openai.com/v1").unwrap();
        assert_eq!((direct.name.as_str(), direct.lab_name.as_str(), direct.vision), ("GPT-4.1", "OpenAI", true));
        assert_eq!(direct.cost_line().as_deref(), Some("$2 in · $8 out per M"));

        // The endpoint's own listing wins and fills gaps from the canonical lab entry.
        let resold = index.info("GPT-4.1", "HTTPS://api.reseller.dev/v1").unwrap();
        assert_eq!((resold.name.as_str(), resold.description.as_deref(), resold.context_window), ("GPT-4.1", Some("Workhorse"), Some(1047576)));
        assert_eq!(resold.cost_line().as_deref(), Some("$1.50 in · $6 out per M"));

        let alias = index.info("kimi-fast", "").unwrap();
        assert_eq!((alias.lab_name.as_str(), alias.open_weights), ("Moonshot AI", true));
        assert_eq!(index.info("openai/gpt-4.1", "").unwrap().name, "GPT-4.1");
        assert!(index.info("unknown", "").is_none());
    }

    #[test]
    fn effort_levels_come_from_reasoning_options() {
        use ctx_core::EffortLookup;
        let index = ModelsDevIndex::parse(API.as_bytes()).unwrap();
        let levels = |id: &str| index.effort_levels(id, "https://api.reseller.dev/v1");
        let all: Vec<String> = ["low", "medium", "high", "xhigh", "max"].map(String::from).to_vec();
        assert_eq!(levels("claude-opus-5"), Some(all.clone()));
        assert_eq!(levels("opus"), Some(all), "a listing without options takes the lab's");
        assert_eq!(levels("opus-lite"), Some(vec!["low".to_string(), "high".to_string()]), "the endpoint's own options win");
        assert_eq!(levels("thinker"), Some(vec![]), "a token budget is not an effort");
        assert_eq!(levels("gpt-4.1"), Some(vec![]), "a model that does not reason");
        assert_eq!(levels("unknown"), None);
    }

    #[test]
    fn limits_read_like_the_app() {
        assert_eq!(fmt_limit(1_047_576), "1M");
        assert_eq!(fmt_limit(1_500_000), "1.5M");
        assert_eq!(fmt_limit(32_768), "32K");
        assert_eq!(fmt_limit(200_000), "200K");
        assert_eq!(fmt_limit(512), "512");
    }
}
