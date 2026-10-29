//! The fallback index: which models stand in for a failing one, chosen by
//! closeness in intelligence. It lives in `fallback-index.json` at the root
//! of the public repository so it can be edited without a release; a copy
//! is compiled in, the CLI keeps a fresher one cached in
//! `<data_dir>/fallback-index.json`, and [`current`] is whichever of the two
//! was updated last.
//!
//! Only a ModelGateway model with no `model.fallbacks` of its own gets
//! automatic fallbacks (see [`crate::ModelConfig::chain`]): the index names
//! gateway model IDs, which another endpoint would not serve.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

/// Where installed apps fetch the index from.
pub const INDEX_URL: &str = "https://raw.githubusercontent.com/Model-Gateway/context-agent/main/fallback-index.json";
/// The cached copy's file name under the data directory.
pub const CACHE_FILE: &str = "fallback-index.json";

const EMBEDDED: &str = include_str!("../../../fallback-index.json");

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FallbackIndex {
    pub version: u32,
    /// `YYYY-MM-DD`; the later of the embedded and cached copies wins.
    pub updated: String,
    pub max_fallbacks: usize,
    pub max_score_drop: f64,
    pub max_score_rise: f64,
    pub models: BTreeMap<String, IndexedModel>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IndexedModel {
    pub score: f64,
    pub vendor: String,
    /// Replaces the score-based choice for this model.
    #[serde(default)]
    pub fallbacks: Option<Vec<String>>,
    /// False keeps this model from being picked as another's fallback.
    #[serde(default = "yes")]
    pub as_fallback: bool,
    /// Rules the model out as a fallback for a configured window larger than
    /// this.
    #[serde(default)]
    pub context_window: Option<u32>,
}

fn yes() -> bool {
    true
}

impl FallbackIndex {
    /// The parsed index, or `None` for anything this version cannot use.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let index: FallbackIndex = serde_json::from_slice(data).ok()?;
        (index.version == 1 && !index.models.is_empty()).then_some(index)
    }

    pub fn embedded() -> Self {
        Self::parse(EMBEDDED.as_bytes()).expect("the embedded fallback-index.json parses")
    }

    /// The models to try, in order, when `model` fails while configured with
    /// `context_window`: the closest scores first (higher on a tie), at most
    /// one per vendor, `model`'s own vendor last. Empty for a model the index
    /// does not list.
    pub fn fallbacks_for(&self, model: &str, context_window: u32) -> Vec<String> {
        let Some(entry) = self.models.get(model) else { return Vec::new() };
        if let Some(list) = &entry.fallbacks {
            return list.iter().filter(|name| name.as_str() != model).take(self.max_fallbacks).cloned().collect();
        }
        let mut candidates: Vec<(&String, &IndexedModel)> = self
            .models
            .iter()
            .filter(|(name, m)| {
                name.as_str() != model
                    && m.as_fallback
                    && m.score >= entry.score - self.max_score_drop
                    && m.score <= entry.score + self.max_score_rise
                    && m.context_window.is_none_or(|window| window >= context_window)
            })
            .collect();
        candidates.sort_by(|(a_name, a), (b_name, b)| {
            let (a_gap, b_gap) = ((a.score - entry.score).abs(), (b.score - entry.score).abs());
            a_gap.total_cmp(&b_gap).then(b.score.total_cmp(&a.score)).then(a_name.cmp(b_name))
        });
        let mut vendors: Vec<&str> = Vec::new();
        let mut others = Vec::new();
        let mut same_vendor = None;
        for (name, m) in candidates {
            if m.vendor == entry.vendor {
                same_vendor.get_or_insert(name.clone());
            } else if !vendors.contains(&m.vendor.as_str()) {
                vendors.push(&m.vendor);
                others.push(name.clone());
            }
        }
        others.extend(same_vendor);
        others.truncate(self.max_fallbacks);
        others
    }
}

fn slot() -> &'static RwLock<Arc<FallbackIndex>> {
    static CURRENT: OnceLock<RwLock<Arc<FallbackIndex>>> = OnceLock::new();
    CURRENT.get_or_init(|| RwLock::new(Arc::new(FallbackIndex::embedded())))
}

/// The index in use: the embedded copy until a newer one is installed.
pub fn current() -> Arc<FallbackIndex> {
    slot().read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Use `index` from now on when it is at least as recent as the current
/// one; true when it was installed.
pub fn install(index: FallbackIndex) -> bool {
    let mut current = slot().write().unwrap_or_else(|e| e.into_inner());
    if index.updated < current.updated {
        return false;
    }
    *current = Arc::new(index);
    true
}

/// Install the cached copy under `data_root`, if there is a usable one.
pub fn load_cached(data_root: &Path) -> bool {
    std::fs::read(data_root.join(CACHE_FILE)).ok().and_then(|data| FallbackIndex::parse(&data)).is_some_and(install)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(models: &[(&str, f64, &str)]) -> FallbackIndex {
        FallbackIndex {
            version: 1,
            updated: "2026-01-01".into(),
            max_fallbacks: 3,
            max_score_drop: 8.0,
            max_score_rise: 6.0,
            models: models.iter().map(|(n, s, v)| (n.to_string(), IndexedModel { score: *s, vendor: v.to_string(), fallbacks: None, as_fallback: true, context_window: None })).collect(),
        }
    }

    #[test]
    fn embedded_index_parses_and_covers_kimi() {
        let index = FallbackIndex::embedded();
        assert_eq!(index.fallbacks_for("kimi-k3.1-pro", 200_000).len(), 3);
    }

    #[test]
    fn closest_scores_first_one_per_vendor_own_vendor_last() {
        let index = index(&[("primary", 50.0, "a"), ("same", 50.0, "a"), ("b1", 52.0, "b"), ("b2", 49.0, "b"), ("c", 47.0, "c"), ("d", 46.0, "d")]);
        // b2 (gap 1) beats b1 (gap 2) for vendor b; the equally close `same`
        // waits behind every other vendor, here past the limit.
        assert_eq!(index.fallbacks_for("primary", 100_000), vec!["b2", "c", "d"]);
        let index = FallbackIndex { max_fallbacks: 4, ..index };
        assert_eq!(index.fallbacks_for("primary", 100_000), vec!["b2", "c", "d", "same"]);
    }

    #[test]
    fn window_score_range_and_flags_rule_models_out() {
        let mut index = index(&[("primary", 50.0, "a"), ("far_below", 41.0, "b"), ("far_above", 57.0, "c"), ("small", 50.0, "d"), ("hidden", 50.0, "e"), ("ok", 44.0, "f")]);
        index.models.get_mut("small").unwrap().context_window = Some(128_000);
        index.models.get_mut("hidden").unwrap().as_fallback = false;
        assert_eq!(index.fallbacks_for("primary", 200_000), vec!["ok"]);
        assert_eq!(index.fallbacks_for("primary", 100_000), vec!["small", "ok"]);
        assert!(index.fallbacks_for("unlisted", 100_000).is_empty());
    }

    #[test]
    fn an_explicit_list_replaces_the_scores() {
        let mut index = index(&[("primary", 50.0, "a"), ("near", 50.0, "b")]);
        index.models.get_mut("primary").unwrap().fallbacks = Some(vec!["primary".into(), "x".into(), "y".into()]);
        assert_eq!(index.fallbacks_for("primary", 100_000), vec!["x", "y"]);
    }
}
