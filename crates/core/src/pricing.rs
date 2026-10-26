//! What a model request costs, and the spending limits checked before one.
//!
//! A request is priced by the model that served it, in this order:
//! 1. `[pricing.models."<id>"]`: the user's own price for that model;
//! 2. the installed [`PriceLookup`] (the frontends install models.dev's
//!    catalogue, resolved against the endpoint's base URL);
//! 3. the flat `[pricing]` prices, the fallback for unlisted models.
//!
//! Requests sent through ModelGateway are first priced the same way and
//! then replaced by what the gateway billed: the installed [`Biller`] looks
//! the requests up by their `X-Oneapi-Request-Id`.

use crate::config::Config;
use crate::model::TokenUsage;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

/// `ProviderMetadata::extra` key holding the gateway's id for a request.
pub const GATEWAY_REQUEST_ID: &str = "gateway_request_id";

/// USD per million tokens. Cached input falls back to the input price.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
}

impl ModelPrice {
    pub fn cost(&self, usage: &TokenUsage) -> f64 {
        let cached = usage.cached_prompt_tokens.unwrap_or(0).min(usage.prompt_tokens);
        let uncached = usage.prompt_tokens - cached;
        (uncached as f64 * self.input + cached as f64 * self.cache_read.unwrap_or(self.input) + usage.completion_tokens as f64 * self.output) / 1e6
    }
}

/// A catalogue of model prices (models.dev, a gateway's price list).
pub trait PriceLookup: Send + Sync {
    /// The price of `model` as served from `base_url`.
    fn price(&self, model: &str, base_url: &str) -> Option<ModelPrice>;
}

/// Gateway request id → what the gateway billed for it (USD). Ids it has no
/// record of yet are left out.
pub type BilledCosts = HashMap<String, f64>;

/// Looks up what a gateway billed for requests sent with `config`'s model
/// endpoint; an empty map when that endpoint is not a billing gateway.
pub trait Biller: Send + Sync {
    fn billed<'a>(&'a self, config: &'a Config, request_ids: Vec<String>) -> Pin<Box<dyn Future<Output = BilledCosts> + Send + 'a>>;
}

static LOOKUP: RwLock<Option<Arc<dyn PriceLookup>>> = RwLock::new(None);
static BILLER: RwLock<Option<Arc<dyn Biller>>> = RwLock::new(None);

/// Installs (or replaces) the process-wide price catalogue.
pub fn install_price_lookup(lookup: Arc<dyn PriceLookup>) {
    *LOOKUP.write().unwrap_or_else(|e| e.into_inner()) = Some(lookup);
}

pub fn install_biller(biller: Arc<dyn Biller>) {
    *BILLER.write().unwrap_or_else(|e| e.into_inner()) = Some(biller);
}

pub fn biller() -> Option<Arc<dyn Biller>> {
    BILLER.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Where a model's price came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceSource {
    /// `[pricing.models]`.
    Custom,
    /// The installed catalogue.
    Catalog,
    /// The flat `[pricing]` prices.
    Default,
}

/// The price `config` puts on `model`, and where it came from.
pub fn price_for(config: &Config, model: &str) -> Option<(ModelPrice, PriceSource)> {
    let pricing = config.pricing.as_ref();
    if let Some(price) = pricing.and_then(|p| lookup_model(&p.models, model)) {
        return Some((price, PriceSource::Custom));
    }
    let lookup = LOOKUP.read().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(price) = lookup.and_then(|l| l.price(model, &config.model.base_url)) {
        return Some((price, PriceSource::Catalog));
    }
    pricing.and_then(|p| p.flat()).map(|price| (price, PriceSource::Default))
}

/// The cost of one request `model` served with `usage`; `None` without a price.
pub fn request_cost(config: &Config, model: &str, usage: &TokenUsage) -> Option<f64> {
    price_for(config, model).map(|(price, _)| price.cost(usage))
}

/// Exact id first, then case-insensitively, then the id without its
/// `vendor/` prefix (`openai/gpt-4.1` → `gpt-4.1`).
fn lookup_model(models: &BTreeMap<String, ModelPrice>, model: &str) -> Option<ModelPrice> {
    let model = model.trim();
    if model.is_empty() || models.is_empty() {
        return None;
    }
    let find = |id: &str| models.get(id).or_else(|| models.iter().find(|(k, _)| k.eq_ignore_ascii_case(id)).map(|(_, v)| v)).copied();
    find(model).or_else(|| model.rsplit_once('/').and_then(|(_, bare)| find(bare)))
}

// ----- spending limits --------------------------------------------------------

/// The window spending limits count over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpendPeriod {
    Day,
    Week,
    #[default]
    Month,
}

impl SpendPeriod {
    /// "today", "this week", "this month".
    pub fn phrase(&self) -> &'static str {
        match self {
            SpendPeriod::Day => "today",
            SpendPeriod::Week => "this week",
            SpendPeriod::Month => "this month",
        }
    }

    /// When the current period began: local midnight, Monday, or the 1st.
    pub fn start(&self, now: chrono::DateTime<chrono::Local>) -> chrono::DateTime<chrono::Utc> {
        use chrono::{Datelike, TimeZone};
        let date = now.date_naive();
        let first = match self {
            SpendPeriod::Day => date,
            SpendPeriod::Week => date - chrono::Duration::days(date.weekday().num_days_from_monday() as i64),
            SpendPeriod::Month => date.with_day(1).unwrap_or(date),
        };
        let midnight = first.and_hms_opt(0, 0, 0).unwrap_or_default();
        chrono::Local.from_local_datetime(&midnight).earliest().map(|t| t.with_timezone(&chrono::Utc)).unwrap_or_else(|| now.with_timezone(&chrono::Utc))
    }
}

/// How far below the model's best ModelGateway discount (percentage
/// points) a fallback provider may be before the user is asked.
pub const DISCOUNT_DROP_POINTS: u32 = 10;

/// `[spend]`: caps on what the agent may spend per period, checked before
/// every model request against the recorded request costs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpendConfig {
    pub period: SpendPeriod,
    /// Every model together (USD).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_usd: Option<f64>,
    /// Model id → cap for that model alone (USD).
    pub models: BTreeMap<String, f64>,
    /// Ask before ModelGateway falls back to a provider discounted at least
    /// [`DISCOUNT_DROP_POINTS`] below the model's best offer; false
    /// ("always continue") lets the gateway fall back without asking.
    pub confirm_discount_drop: bool,
}

impl Default for SpendConfig {
    fn default() -> Self {
        Self { period: SpendPeriod::default(), max_usd: None, models: BTreeMap::new(), confirm_discount_drop: true }
    }
}

impl SpendConfig {
    /// The cap on `model` alone (same lookup as model prices).
    pub fn model_limit(&self, model: &str) -> Option<(String, f64)> {
        let model = model.trim();
        let find = |id: &str| self.models.get_key_value(id).or_else(|| self.models.iter().find(|(k, _)| k.eq_ignore_ascii_case(id))).map(|(k, v)| (k.clone(), *v));
        find(model).or_else(|| model.rsplit_once('/').and_then(|(_, bare)| find(bare)))
    }

    pub fn is_empty(&self) -> bool {
        self.max_usd.is_none() && self.models.is_empty()
    }

    /// Why a request to `model` may not be sent, given what was spent this
    /// period in total and on `model`; `None` while under every cap.
    pub fn exceeded(&self, model: &str, spent_total: f64, spent_model: f64) -> Option<String> {
        let money = |v: f64| format!("${v:.2}");
        if let Some((id, limit)) = self.model_limit(model) {
            if spent_model >= limit {
                return Some(format!("Spending limit reached: {} of {} on {id} {}. Raise or remove the limit in Settings → Spending.", money(spent_model), money(limit), self.period.phrase()));
            }
        }
        if let Some(limit) = self.max_usd {
            if spent_total >= limit {
                return Some(format!("Spending limit reached: {} of {} on all models {}. Raise or remove the limit in Settings → Spending.", money(spent_total), money(limit), self.period.phrase()));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PricingConfig;
    use crate::model::UsageSource;

    fn usage(prompt: u64, cached: u64, completion: u64) -> TokenUsage {
        TokenUsage { prompt_tokens: prompt, completion_tokens: completion, cached_prompt_tokens: Some(cached), reasoning_tokens: None, source: UsageSource::Reported }
    }

    #[test]
    fn cached_input_uses_the_cache_price_or_the_input_price() {
        let price = ModelPrice { input: 10.0, output: 30.0, cache_read: Some(1.0) };
        assert!((price.cost(&usage(200_000, 100_000, 100_000)) - 4.1).abs() < 1e-9);
        let uncached = ModelPrice { cache_read: None, ..price };
        assert!((uncached.cost(&usage(200_000, 100_000, 0)) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn custom_prices_win_and_the_flat_price_is_the_fallback() {
        let mut config = Config::default();
        config.pricing = Some(PricingConfig { input_per_million: Some(1.0), cached_input_per_million: None, output_per_million: Some(2.0), models: BTreeMap::from([("gpt-4.1".to_string(), ModelPrice { input: 2.0, output: 8.0, cache_read: Some(0.5) })]) });
        assert_eq!(price_for(&config, "openai/GPT-4.1").map(|p| p.1), Some(PriceSource::Custom));
        let (flat, source) = price_for(&config, "some-unlisted-model-xyz").unwrap();
        assert_eq!((flat.input, flat.output, source), (1.0, 2.0, PriceSource::Default));
    }

    #[test]
    fn limits_name_the_model_cap_first() {
        let spend = SpendConfig { period: SpendPeriod::Month, max_usd: Some(10.0), models: BTreeMap::from([("gpt-4.1".to_string(), 2.0)]), ..SpendConfig::default() };
        assert!(spend.exceeded("gpt-4.1", 1.0, 1.0).is_none());
        assert!(spend.exceeded("openai/gpt-4.1", 1.0, 2.0).unwrap().contains("on gpt-4.1 this month"));
        assert!(spend.exceeded("other", 10.5, 0.0).unwrap().contains("on all models"));
    }

    #[test]
    fn periods_start_at_local_boundaries() {
        use chrono::TimeZone;
        let now = chrono::Local.with_ymd_and_hms(2026, 10, 23, 15, 30, 0).unwrap();
        let local = |t: chrono::DateTime<chrono::Utc>| t.with_timezone(&chrono::Local).naive_local().to_string();
        assert_eq!(local(SpendPeriod::Day.start(now)), "2026-10-23 00:00:00");
        assert_eq!(local(SpendPeriod::Week.start(now)), "2026-10-19 00:00:00");
        assert_eq!(local(SpendPeriod::Month.start(now)), "2026-10-01 00:00:00");
    }
}
