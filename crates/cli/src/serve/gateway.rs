//! ModelGateway account calls for the desktop app (`gateway/*` methods):
//! the device-code sign-in handshake, the account summary, the usage rows
//! and sign-out. The approved device key is kept here, in the credential
//! store (`CredentialStore::MODELGATEWAY`), never handed to the app or an
//! environment variable: `model.credential = "modelgateway"` makes the
//! provider use it. The HTTP side is `crate::gateway` (shared with the TUI's
//! `/signin`); these methods hand the `data` object of each reply back
//! (docs/desktop-protocol.md "ModelGateway").

use super::*;
use crate::gateway::{self, GatewayError};

#[derive(Deserialize)]
pub(super) struct GatewayDeviceStartParams {
    pub(super) base_url: String,
    #[serde(default)]
    pub(super) device_name: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct GatewayDevicePollParams {
    pub(super) base_url: String,
    pub(super) device_code: String,
}

/// Calls that authenticate with the stored credential; `base_url` may be
/// omitted (the stored origin is used).
#[derive(Deserialize, Default)]
pub(super) struct GatewayKeyParams {
    #[serde(default)]
    pub(super) base_url: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct GatewayUsageParams {
    #[serde(default)]
    pub(super) base_url: Option<String>,
    #[serde(default)]
    pub(super) start_timestamp: Option<i64>,
    #[serde(default)]
    pub(super) end_timestamp: Option<i64>,
}

/// `gateway/billing/*` and `gateway/key/update` writes: the authenticated
/// call's `base_url` plus the body relayed to the gateway as is (`{amount}`,
/// `{plan_id, method}`, `{billing_preference}`, the key settings).
#[derive(Deserialize)]
pub(super) struct GatewayBillingParams {
    #[serde(default)]
    pub(super) base_url: Option<String>,
    #[serde(flatten)]
    pub(super) body: serde_json::Map<String, Value>,
}

/// `gateway/report`: one assistant message of a run, by its task and step.
#[derive(Deserialize)]
pub(super) struct GatewayReportParams {
    pub(super) project_id: String,
    pub(super) task_id: String,
    pub(super) step: u32,
    pub(super) reason: String,
    #[serde(default)]
    pub(super) comment: Option<String>,
    #[serde(default)]
    pub(super) base_url: Option<String>,
}

/// `gateway/status`.
#[derive(Serialize)]
pub(super) struct GatewayStatus {
    pub(super) signed_in: bool,
    pub(super) origin: Option<String>,
    pub(super) account: Option<String>,
}

impl From<GatewayError> for RpcError {
    fn from(e: GatewayError) -> Self {
        let (message, data) = match e {
            GatewayError::Transport(message) => (message, json!({"kind": "transport"})),
            GatewayError::Unauthorized { message, code } => (message, json!({"kind": "unauthorized", "code": code})),
            GatewayError::Gateway { message, code } => (message, json!({"kind": "gateway", "code": code})),
            GatewayError::SignedOut => (GatewayError::SignedOut.to_string(), json!({"kind": "signed_out"})),
            GatewayError::Local(e) => return RpcError::from(e),
        };
        let mut err = RpcError::new(BACKEND_ERROR, message);
        err.data = Some(data);
        err
    }
}

/// The gateway origin without a trailing slash or `/v1`; `-32602` when it is
/// not an absolute http(s) URL.
fn gateway_origin(base_url: &str) -> RpcResult<String> {
    gateway::origin(base_url).ok_or_else(|| RpcError::invalid_params(format!("base_url must be an absolute http(s) URL, got `{base_url}`")))
}

impl Server {
    pub(super) fn layout(&self) -> ctx_core::paths::DataLayout {
        self.config.read().layout()
    }

    /// `-32602` for a `base_url` that is given but not an http(s) URL.
    fn check_base_url(base_url: Option<&str>) -> RpcResult<()> {
        if let Some(base) = base_url.map(str::trim).filter(|b| !b.is_empty()) {
            gateway_origin(base)?;
        }
        Ok(())
    }

    /// `gateway/status`: whether a device key is stored, and for whom.
    pub(super) async fn gateway_status(&self) -> RpcResult<Value> {
        let status = match gateway::stored(&self.layout())? {
            Some(credential) => GatewayStatus { signed_in: true, origin: credential.origin, account: credential.account },
            None => GatewayStatus { signed_in: false, origin: None, account: None },
        };
        to_value(&status)
    }

    /// `gateway/device/start`: opens a device authorization; the result is
    /// the gateway's `{device_code, user_code, verification_uri,
    /// verification_uri_complete, expires_in, interval}`.
    pub(super) async fn gateway_device_start(&self, params: GatewayDeviceStartParams) -> RpcResult<Value> {
        let origin = gateway_origin(&params.base_url)?;
        Ok(gateway::device_start(&self.layout(), &origin, params.device_name).await?)
    }

    /// `gateway/device/poll`: `{status: "pending"|"approved"|"denied"|"expired",
    /// interval?, account?}`. On approval the gateway's one-time key is
    /// written to the credential store and never returned.
    pub(super) async fn gateway_device_poll(&self, params: GatewayDevicePollParams) -> RpcResult<Value> {
        let origin = gateway_origin(&params.base_url)?;
        Ok(gateway::device_poll(&self.layout(), &origin, &params.device_code).await?)
    }

    /// `gateway/account`: the account summary behind the stored key.
    pub(super) async fn gateway_account(&self, params: GatewayKeyParams) -> RpcResult<Value> {
        Self::check_base_url(params.base_url.as_deref())?;
        Ok(gateway::account(&self.layout(), params.base_url.as_deref()).await?)
    }

    /// `gateway/usage`: hourly usage rows per model and project.
    pub(super) async fn gateway_usage(&self, params: GatewayUsageParams) -> RpcResult<Value> {
        Self::check_base_url(params.base_url.as_deref())?;
        Ok(gateway::usage(&self.layout(), params.base_url.as_deref(), params.start_timestamp, params.end_timestamp).await?)
    }

    /// `gateway/billing`: the balance top-up options, the plans on sale and
    /// the billing preference (`GET /api/device/billing`).
    pub(super) async fn gateway_billing(&self, params: GatewayKeyParams) -> RpcResult<Value> {
        Self::check_base_url(params.base_url.as_deref())?;
        Ok(gateway::billing(&self.layout(), params.base_url.as_deref()).await?)
    }

    /// `gateway/billing/{quote,topup,subscribe}`: a price quote, a checkout
    /// link for adding funds, or buying a plan (by checkout link, or from
    /// the balance). Checkout links are opened in the browser by the app.
    pub(super) async fn gateway_billing_post(&self, action: &str, params: GatewayBillingParams) -> RpcResult<Value> {
        Self::check_base_url(params.base_url.as_deref())?;
        Ok(gateway::billing_post(&self.layout(), params.base_url.as_deref(), action, Value::Object(params.body)).await?)
    }

    /// `gateway/billing/preference`: whether requests draw on the plan or
    /// the balance first.
    pub(super) async fn gateway_billing_preference(&self, params: GatewayBillingParams) -> RpcResult<Value> {
        Self::check_base_url(params.base_url.as_deref())?;
        Ok(gateway::billing_preference(&self.layout(), params.base_url.as_deref(), Value::Object(params.body)).await?)
    }

    /// `gateway/key`: the device key's price limits and spending cap
    /// (`GET /api/device/key`).
    pub(super) async fn gateway_key(&self, params: GatewayKeyParams) -> RpcResult<Value> {
        Self::check_base_url(params.base_url.as_deref())?;
        Ok(gateway::key_settings(&self.layout(), params.base_url.as_deref()).await?)
    }

    /// `gateway/key/update`: changes them (`PUT /api/device/key`).
    pub(super) async fn gateway_key_update(&self, params: GatewayBillingParams) -> RpcResult<Value> {
        Self::check_base_url(params.base_url.as_deref())?;
        Ok(gateway::update_key_settings(&self.layout(), params.base_url.as_deref(), Value::Object(params.body)).await?)
    }

    /// `gateway/context_credit/claim`: claims the welcome Context credit
    /// (`POST /api/device/context-credit/claim`).
    pub(super) async fn gateway_claim_context_credit(&self, params: GatewayKeyParams) -> RpcResult<Value> {
        Self::check_base_url(params.base_url.as_deref())?;
        Ok(gateway::claim_context_credit(&self.layout(), params.base_url.as_deref()).await?)
    }

    /// `gateway/device/identify`: attaches this machine's identity to the
    /// stored key (`POST /api/device/identify`); the reply is the account's
    /// `device` block. `gateway/account` does this by itself once.
    pub(super) async fn gateway_device_identify(&self, params: GatewayKeyParams) -> RpcResult<Value> {
        Self::check_base_url(params.base_url.as_deref())?;
        Ok(gateway::identify(&self.layout(), params.base_url.as_deref()).await?)
    }

    /// `gateway/referral`: the Invite card (`GET /api/device/referral`).
    pub(super) async fn gateway_referral(&self, params: GatewayKeyParams) -> RpcResult<Value> {
        Self::check_base_url(params.base_url.as_deref())?;
        Ok(gateway::referral(&self.layout(), params.base_url.as_deref()).await?)
    }

    /// `gateway/referral/update`: `{split_referee_cents}` (`PUT /api/device/referral`).
    pub(super) async fn gateway_referral_update(&self, params: GatewayBillingParams) -> RpcResult<Value> {
        Self::check_base_url(params.base_url.as_deref())?;
        Ok(gateway::update_referral(&self.layout(), params.base_url.as_deref(), Value::Object(params.body)).await?)
    }

    /// `gateway/referral/attach`: `{code}` (`POST /api/device/referral/attach`).
    pub(super) async fn gateway_referral_attach(&self, params: GatewayBillingParams) -> RpcResult<Value> {
        Self::check_base_url(params.base_url.as_deref())?;
        Ok(gateway::attach_referral(&self.layout(), params.base_url.as_deref(), Value::Object(params.body)).await?)
    }

    /// `gateway/report`: reports the response of a run's step to the gateway
    /// that served it. The step's last model request that carried a gateway
    /// request id is the one reported; a step that did not go through the
    /// gateway is `-32602`.
    pub(super) async fn gateway_report(&self, params: GatewayReportParams) -> RpcResult<Value> {
        Self::check_base_url(params.base_url.as_deref())?;
        let project = self.project(&params.project_id)?;
        let (task_id, step) = (params.task_id.clone(), params.step);
        let request_id = blocking(move || {
            let requests = project.ctx.stores.sessions.model_requests_for_task(&task_id)?;
            Ok(requests.into_iter().rev().filter(|r| r.step == step).find_map(|r| r.gateway_request_id.filter(|id| !id.is_empty())))
        })
        .await?
        .ok_or_else(|| RpcError::invalid_params("this response did not come through ModelGateway"))?;
        let body = json!({"request_id": request_id, "reason": params.reason, "comment": params.comment.unwrap_or_default()});
        Ok(gateway::report(&self.layout(), params.base_url.as_deref(), body).await?)
    }

    /// `gateway/signout`: revokes the device's key on the gateway and forgets
    /// it locally. The local credential goes even when the gateway cannot be
    /// reached; `{ "revoked": false }` then says the key may still exist there.
    pub(super) async fn gateway_signout(&self, params: GatewayKeyParams) -> RpcResult<Value> {
        Self::check_base_url(params.base_url.as_deref())?;
        let revoked = gateway::sign_out(&self.layout(), params.base_url.as_deref()).await?;
        Ok(json!({"revoked": revoked}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_errors_are_invalid_params() {
        assert_eq!(gateway_origin("https://www.modelgateway.co/v1/").unwrap(), "https://www.modelgateway.co");
        assert_eq!(gateway_origin("modelgateway.co").unwrap_err().code, INVALID_PARAMS);
    }
}
