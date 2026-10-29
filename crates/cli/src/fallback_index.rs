//! Keeps the fallback index (`ctx_core::fallback_index`) current. The copy
//! cached in `<data_dir>/fallback-index.json` is installed when the
//! configuration first loads, and a background task fetches the published
//! index from GitHub whenever the cache is older than a few hours, so an
//! edit to `fallback-index.json` on `main` reaches installed apps without a
//! release. A failed fetch keeps whatever is installed.

use ctx_core::fallback_index::{self, FallbackIndex, CACHE_FILE, INDEX_URL};
use ctx_core::Config;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::{Duration, SystemTime};

const FRESH_FOR: Duration = Duration::from_secs(6 * 60 * 60);
const TIMEOUT: Duration = Duration::from_secs(20);

/// Install the cached index and start the refresh task, once per process.
pub fn start(config: &Config) {
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        let root = config.layout().root;
        fallback_index::load_cached(&root);
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(refresh_loop(root));
        }
    });
}

async fn refresh_loop(root: PathBuf) {
    let path = root.join(CACHE_FILE);
    loop {
        let age = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok());
        if age.is_none_or(|a| a >= FRESH_FOR) {
            refresh(&path).await;
        }
        tokio::time::sleep(FRESH_FOR).await;
    }
}

async fn refresh(path: &Path) {
    let url = std::env::var("CTX_FALLBACK_INDEX_URL").ok().filter(|u| !u.trim().is_empty()).unwrap_or_else(|| INDEX_URL.to_string());
    let Some(data) = fetch(&url).await else {
        tracing::debug!(%url, "fallback index: fetch failed");
        return;
    };
    let Some(index) = FallbackIndex::parse(&data) else {
        tracing::warn!(%url, "fallback index: the published index does not parse");
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, &data).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
    if fallback_index::install(index) {
        tracing::info!(updated = %fallback_index::current().updated, "fallback index refreshed");
    }
}

async fn fetch(url: &str) -> Option<Vec<u8>> {
    let client = reqwest::Client::builder().timeout(TIMEOUT).user_agent(format!("context-agent/{}", env!("CARGO_PKG_VERSION"))).build().ok()?;
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.bytes().await.ok().map(|b| b.to_vec())
}
