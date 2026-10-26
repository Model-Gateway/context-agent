//! Bounded retry with exponential backoff.

use ctx_core::{CancellationToken, CtxError, ModelConfig, ProviderError, Result};
use std::future::Future;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self { max_retries: 3, base_delay: Duration::from_millis(500), max_delay: Duration::from_secs(20) }
    }
}

impl RetryPolicy {
    /// The policy a model's configuration asks for.
    pub fn from_model(config: &ModelConfig) -> Self {
        let base = Duration::from_millis(config.retry_base_delay_ms.max(1));
        Self { max_retries: config.max_retries, base_delay: base, max_delay: Duration::from_millis(config.retry_max_delay_ms).max(base) }
    }

    pub fn delay_for(&self, attempt: u32, hint: Option<Duration>) -> Duration {
        if let Some(h) = hint {
            return h.min(self.max_delay);
        }
        let exp = self.base_delay.as_millis() as u64 * 2u64.saturating_pow(attempt);
        let jitter = (exp / 4).max(1);
        let jittered = exp + (fastrand(attempt) % jitter);
        Duration::from_millis(jittered).min(self.max_delay)
    }

    /// Run `op` until it succeeds, a non-retryable error occurs, retries are
    /// exhausted, or cancellation is requested.
    pub async fn run<T, F, Fut>(&self, cancel: &CancellationToken, mut op: F) -> Result<T>
    where
        F: FnMut(u32) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let mut attempt = 0u32;
        loop {
            if cancel.is_cancelled() {
                return Err(CtxError::Cancelled);
            }
            match op(attempt).await {
                Ok(v) => return Ok(v),
                Err(CtxError::Provider(err)) if err.is_retryable() && attempt < self.max_retries => {
                    let hint = match &err {
                        ProviderError::RateLimited { retry_after } => *retry_after,
                        _ => None,
                    };
                    let delay = self.delay_for(attempt, hint);
                    tracing::warn!(attempt, ?delay, error = %err, "retrying provider request");
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(CtxError::Cancelled),
                        _ = tokio::time::sleep(delay) => {}
                    }
                    attempt += 1;
                }
                Err(CtxError::Provider(err)) if err.is_retryable() => {
                    return Err(ProviderError::RetriesExhausted { attempts: attempt + 1, last: err.to_string() }.into());
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Tiny deterministic pseudo-random helper (no extra dependency needed).
fn fastrand(seed: u32) -> u64 {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0) as u64;
    let mut x = now ^ (seed as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn retries_then_succeeds() {
        let policy = RetryPolicy { max_retries: 3, base_delay: Duration::from_millis(1), max_delay: Duration::from_millis(2) };
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let out: Result<u32> = policy
            .run(&CancellationToken::new(), move |attempt| {
                let c = c.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    if attempt < 2 {
                        Err(ProviderError::Timeout.into())
                    } else {
                        Ok(attempt)
                    }
                }
            })
            .await;
        assert_eq!(out.unwrap(), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn does_not_retry_non_retryable() {
        let policy = RetryPolicy { max_retries: 3, base_delay: Duration::from_millis(1), max_delay: Duration::from_millis(2) };
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let out: Result<()> = policy
            .run(&CancellationToken::new(), move |_| {
                let c = c.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Err(ProviderError::ContextLength("too long".into()).into())
                }
            })
            .await;
        assert!(matches!(out, Err(CtxError::Provider(ProviderError::ContextLength(_)))));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn exhausts_retries() {
        let policy = RetryPolicy { max_retries: 1, base_delay: Duration::from_millis(1), max_delay: Duration::from_millis(2) };
        let out: Result<()> = policy.run(&CancellationToken::new(), |_| async { Err(ProviderError::Timeout.into()) }).await;
        assert!(matches!(out, Err(CtxError::Provider(ProviderError::RetriesExhausted { attempts: 2, .. }))));
    }
}
