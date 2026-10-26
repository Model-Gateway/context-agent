//! Tracing initialization.

use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone)]
pub enum TracingTarget {
    Stderr,
    File(PathBuf),
    Off,
}

/// Initialize the global tracing subscriber. Safe to call once per process;
/// subsequent calls are ignored.
pub fn init_tracing(level: &str, target: TracingTarget) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    match target {
        TracingTarget::Off => {}
        TracingTarget::Stderr => {
            let _ = tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).with_target(false).try_init();
        }
        TracingTarget::File(path) => {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                let _ = tracing_subscriber::fmt()
                    .with_env_filter(filter)
                    .with_writer(std::sync::Mutex::new(file))
                    .with_ansi(false)
                    .try_init();
            }
        }
    }
}
