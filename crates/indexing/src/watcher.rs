//! Filesystem watching with debounce and periodic reconciliation.

use crate::indexer::{IndexReport, Indexer};
use ctx_core::{CtxError, Result};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

pub struct IndexWatcher {
    _debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
    _worker: std::thread::JoinHandle<()>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

impl IndexWatcher {
    /// Start watching the indexer's root. `on_update` runs after each batch
    /// of changes is indexed. A reconciliation pass runs every
    /// `reconcile_every` to recover from missed events.
    pub fn start(indexer: Arc<Indexer>, reconcile_every: Duration, on_update: impl Fn(IndexReport) + Send + 'static) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<Vec<PathBuf>>();
        let mut debouncer = new_debouncer(Duration::from_millis(400), None, move |result: DebounceEventResult| match result {
            Ok(events) => {
                let mut paths: Vec<PathBuf> = events.into_iter().flat_map(|e| e.event.paths.clone()).collect();
                paths.sort();
                paths.dedup();
                if !paths.is_empty() {
                    let _ = tx.send(paths);
                }
            }
            Err(errors) => {
                for e in errors {
                    tracing::warn!(error = %e, "file watcher error");
                }
            }
        })
        .map_err(|e| CtxError::other(format!("failed to create file watcher: {e}")))?;
        debouncer.watch(indexer.root(), RecursiveMode::Recursive).map_err(|e| CtxError::other(format!("failed to watch {}: {e}", indexer.root().display())))?;

        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_flag = stop.clone();
        let worker = std::thread::Builder::new()
            .name("ctx-index-watcher".into())
            .spawn(move || {
                let mut last_reconcile = std::time::Instant::now();
                loop {
                    if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    match rx.recv_timeout(Duration::from_secs(1)) {
                        Ok(paths) => {
                            let relevant: Vec<PathBuf> = paths.into_iter().filter(|p| !p.components().any(|c| c.as_os_str() == ".git")).collect();
                            if relevant.is_empty() {
                                continue;
                            }
                            match indexer.update_paths(&relevant) {
                                Ok(report) => {
                                    if report.files_indexed > 0 || report.files_removed > 0 {
                                        on_update(report);
                                    }
                                }
                                Err(e) => tracing::warn!(error = %e, "incremental index failed"),
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                    if last_reconcile.elapsed() >= reconcile_every {
                        last_reconcile = std::time::Instant::now();
                        match indexer.reconcile() {
                            Ok(report) if report.files_indexed > 0 || report.files_removed > 0 => on_update(report),
                            Ok(_) => {}
                            Err(e) => tracing::warn!(error = %e, "reconcile failed"),
                        }
                    }
                }
            })
            .map_err(|e| CtxError::other(e.to_string()))?;
        Ok(Self { _debouncer: debouncer, _worker: worker, stop })
    }

    pub fn stop(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Drop for IndexWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}
