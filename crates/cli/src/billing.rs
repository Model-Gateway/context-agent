//! What model requests cost (`ctx_core::pricing`): installs models.dev's
//! prices as the catalogue, and the ModelGateway biller that replaces the
//! estimates of gateway-routed requests with what the gateway billed
//! (`POST /api/device/costs`, by `X-Oneapi-Request-Id`).

use crate::models_dev::{self, ModelsDevIndex};
use ctx_core::pricing::{BilledCosts, Biller, ModelPrice, PriceLookup};
use ctx_core::{Config, CredentialStore, GatewayAccess};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

/// The gateway accepts at most this many ids per call.
const COSTS_BATCH: usize = 200;

/// Installs the gateway biller now and the models.dev prices (and each
/// model's reasoning-effort values) once loaded (the disk cache when fresh,
/// else a fetch in the background).
pub fn install(config: &Config) {
    ctx_core::pricing::install_biller(Arc::new(GatewayBiller));
    let config = config.clone();
    tokio::spawn(async move {
        if let Some(index) = models_dev::load(&config).await {
            let index = Arc::new(index);
            ctx_core::pricing::install_price_lookup(Arc::new(ModelsDevPrices(index.clone())));
            ctx_core::install_effort_lookup(index);
        }
    });
}

struct ModelsDevPrices(Arc<ModelsDevIndex>);

impl PriceLookup for ModelsDevPrices {
    fn price(&self, model: &str, base_url: &str) -> Option<ModelPrice> {
        let info = self.0.info(model, base_url)?;
        Some(ModelPrice { input: info.input_cost?, output: info.output_cost?, cache_read: info.cache_read_cost })
    }
}

/// Bills requests sent with the stored ModelGateway credential.
struct GatewayBiller;

impl Biller for GatewayBiller {
    fn billed<'a>(&'a self, config: &'a Config, request_ids: Vec<String>) -> Pin<Box<dyn Future<Output = BilledCosts> + Send + 'a>> {
        Box::pin(async move {
            let mut costs = BilledCosts::new();
            if config.model.credential.as_deref() != Some(CredentialStore::MODELGATEWAY) || request_ids.is_empty() {
                return costs;
            }
            let Some(access) = GatewayAccess::load(config, "") else { return costs };
            let Ok(client) = reqwest::Client::builder().timeout(Duration::from_secs(8)).user_agent(format!("context-agent/{}", env!("CARGO_PKG_VERSION"))).build() else { return costs };
            for batch in request_ids.chunks(COSTS_BATCH) {
                let request = client.post(format!("{}/api/device/costs", access.origin)).bearer_auth(&access.api_key).json(&json!({"request_ids": batch}));
                let Ok(response) = request.send().await else { break };
                let Ok(body) = response.json::<Value>().await else { break };
                costs.extend(parse_costs(&body));
            }
            costs
        })
    }
}

/// `{success, data: {quota_per_unit, costs: [{request_id, quota}]}}` → USD by id.
fn parse_costs(body: &Value) -> BilledCosts {
    let mut costs = BilledCosts::new();
    if body.get("success").and_then(Value::as_bool) != Some(true) {
        return costs;
    }
    let data = &body["data"];
    let per_unit = data.get("quota_per_unit").and_then(Value::as_f64).filter(|u| *u > 0.0).unwrap_or(500_000.0);
    for row in data.get("costs").and_then(Value::as_array).into_iter().flatten() {
        if let (Some(id), Some(quota)) = (row.get("request_id").and_then(Value::as_str), row.get("quota").and_then(Value::as_f64)) {
            costs.insert(id.to_string(), quota / per_unit);
        }
    }
    costs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_converts_to_dollars() {
        let body = json!({"success": true, "data": {"quota_per_unit": 500000, "costs": [{"request_id": "a", "quota": 250000}, {"request_id": "b"}]}});
        let costs = parse_costs(&body);
        assert_eq!(costs.len(), 1);
        assert!((costs["a"] - 0.5).abs() < 1e-9);
        assert!(parse_costs(&json!({"success": false, "message": "no"})).is_empty());
    }
}
