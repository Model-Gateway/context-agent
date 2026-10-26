//! Approval handlers connect the policy engine to a user interface.

use async_trait::async_trait;
use ctx_core::{ApprovalDecision, ApprovalRequest};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

#[async_trait]
pub trait ApprovalHandler: Send + Sync {
    async fn request(&self, request: ApprovalRequest) -> ApprovalDecision;
}

/// Approve everything (headless `--approval auto`).
pub struct AutoApprove;

#[async_trait]
impl ApprovalHandler for AutoApprove {
    async fn request(&self, _request: ApprovalRequest) -> ApprovalDecision {
        ApprovalDecision::Approve
    }
}

/// Deny everything that needs approval (headless `--approval deny`).
pub struct DenyAll;

#[async_trait]
impl ApprovalHandler for DenyAll {
    async fn request(&self, _request: ApprovalRequest) -> ApprovalDecision {
        ApprovalDecision::Deny
    }
}

/// Forwards requests over a channel to an interactive UI and waits for the
/// answer. Dropping the receiver denies.
pub struct ChannelApprovals {
    tx: mpsc::Sender<(ApprovalRequest, oneshot::Sender<ApprovalDecision>)>,
}

impl ChannelApprovals {
    pub fn new() -> (Self, mpsc::Receiver<(ApprovalRequest, oneshot::Sender<ApprovalDecision>)>) {
        let (tx, rx) = mpsc::channel(8);
        (Self { tx }, rx)
    }
}

#[async_trait]
impl ApprovalHandler for ChannelApprovals {
    async fn request(&self, request: ApprovalRequest) -> ApprovalDecision {
        let (tx, rx) = oneshot::channel();
        if self.tx.send((request, tx)).await.is_err() {
            return ApprovalDecision::Deny;
        }
        rx.await.unwrap_or(ApprovalDecision::Deny)
    }
}

/// Wraps another handler and remembers `ApproveForSession` answers keyed by
/// tool, summary and the full action details (command text or patch), so
/// only truly identical actions skip the prompt.
pub struct RememberingApprovals {
    inner: Arc<dyn ApprovalHandler>,
    remembered: Arc<Mutex<HashSet<String>>>,
}

impl RememberingApprovals {
    pub fn new(inner: Arc<dyn ApprovalHandler>) -> Self {
        Self { inner, remembered: Arc::default() }
    }

    /// A handler that asks `inner` and shares this one's remembered
    /// answers (one per run, remembering for all of them).
    pub fn sharing(&self, inner: Arc<dyn ApprovalHandler>) -> Self {
        Self { inner, remembered: self.remembered.clone() }
    }

    fn key(request: &ApprovalRequest) -> String {
        format!("{}::{}::{}", request.tool, request.summary, ctx_core::content_hash(request.details.as_bytes()))
    }
}

#[async_trait]
impl ApprovalHandler for RememberingApprovals {
    async fn request(&self, request: ApprovalRequest) -> ApprovalDecision {
        let key = Self::key(&request);
        if self.remembered.lock().contains(&key) {
            return ApprovalDecision::Approve;
        }
        let decision = self.inner.request(request).await;
        if decision == ApprovalDecision::ApproveForSession {
            self.remembered.lock().insert(key);
        }
        decision
    }
}
