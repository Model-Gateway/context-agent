//! `spend/summary` and `pricing/lookup`: what the agent spent this period
//! against the `[spend]` limits, and the price each model is charged at
//! (docs/desktop-protocol.md "Spending").

use super::*;
use ctx_core::pricing::{price_for, PriceSource};
use ctx_core::{ModelPrice, SpendPeriod};
use ctx_storage::SpendLedger;

#[derive(Deserialize)]
pub(super) struct PricingLookupParams {
    pub(super) models: Vec<String>,
}

#[derive(Serialize)]
struct PriceEntry {
    model: String,
    /// Absent when no price is known: requests then have no cost.
    price: Option<ModelPrice>,
    source: Option<PriceSource>,
}

#[derive(Serialize)]
struct ModelSpendInfo {
    model: String,
    cost_usd: f64,
    requests: u64,
    unpriced_requests: u64,
    limit_usd: Option<f64>,
}

#[derive(Serialize)]
struct SpendSummary {
    period: SpendPeriod,
    /// When the current period began (RFC 3339).
    since: String,
    total_usd: f64,
    max_usd: Option<f64>,
    /// Spend per model this period, plus every model with a limit and no spend yet.
    models: Vec<ModelSpendInfo>,
}

impl Server {
    pub(super) async fn spend_summary(&self) -> RpcResult<Value> {
        let config = self.config.read().clone();
        let summary = blocking(move || {
            let spend = &config.spend;
            let since = spend.period.start(chrono::Local::now());
            let ledger = SpendLedger::open(&config.layout())?;
            let mut models: Vec<ModelSpendInfo> = ledger
                .by_model(since)?
                .into_iter()
                .map(|m| ModelSpendInfo { limit_usd: spend.model_limit(&m.model).map(|(_, l)| l), model: m.model, cost_usd: m.cost_usd, requests: m.requests, unpriced_requests: m.unpriced_requests })
                .collect();
            for (model, limit) in &spend.models {
                if !models.iter().any(|m| spend.model_limit(&m.model).is_some_and(|(id, _)| &id == model)) {
                    models.push(ModelSpendInfo { model: model.clone(), cost_usd: 0.0, requests: 0, unpriced_requests: 0, limit_usd: Some(*limit) });
                }
            }
            Ok(SpendSummary { period: spend.period, since: since.to_rfc3339(), total_usd: ledger.spent_since(since, None)?, max_usd: spend.max_usd, models })
        })
        .await?;
        to_value(&summary)
    }

    pub(super) async fn pricing_lookup(&self, params: PricingLookupParams) -> RpcResult<Value> {
        let config = self.config.read().clone();
        let prices: Vec<PriceEntry> = params
            .models
            .into_iter()
            .map(|model| {
                let found = price_for(&config, &model);
                PriceEntry { price: found.map(|(p, _)| p), source: found.map(|(_, s)| s), model }
            })
            .collect();
        Ok(json!({ "prices": to_value(&prices)? }))
    }
}
