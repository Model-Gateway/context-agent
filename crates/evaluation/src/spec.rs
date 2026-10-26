//! Benchmark task and retrieval query definitions.

use ctx_core::{CtxError, Result, TaskCategory};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    pub prompt: String,
    #[serde(default)]
    pub expected_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkTask {
    pub id: String,
    pub repository: String,
    pub category: TaskCategory,
    #[serde(default)]
    pub languages: Vec<String>,
    pub prompt: String,
    #[serde(default)]
    pub validation: Vec<String>,
    #[serde(default)]
    pub expected_files: Vec<String>,
    #[serde(default)]
    pub requires_tools: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub turns: Vec<Turn>,
    /// Set by the loader: the file this task came from.
    #[serde(skip)]
    pub source: PathBuf,
}

fn default_timeout() -> u64 {
    600
}

impl BenchmarkTask {
    /// All prompts in order (initial + follow-up turns).
    pub fn prompts(&self) -> Vec<(String, Vec<String>)> {
        let mut v = vec![(self.prompt.clone(), self.expected_files.clone())];
        v.extend(self.turns.iter().map(|t| (t.prompt.clone(), t.expected_files.clone())));
        v
    }

    pub fn tools_available(&self) -> Vec<(String, bool)> {
        self.requires_tools.iter().map(|t| (t.clone(), which(t))).collect()
    }
}

/// Whether `executable` is on `PATH` (`PATHEXT` on Windows; `cargo` also
/// in `~/.cargo/bin`).
pub fn which(executable: &str) -> bool {
    ctx_core::platform::find_executable(executable).is_some()
}

/// Load every `*.toml` task under a directory (or a single file).
pub fn load_tasks(path: &Path) -> Result<Vec<BenchmarkTask>> {
    let mut files: Vec<PathBuf> = if path.is_dir() {
        std::fs::read_dir(path)?.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "toml").unwrap_or(false)).collect()
    } else {
        vec![path.to_path_buf()]
    };
    files.sort();
    let mut tasks = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file)?;
        let mut task: BenchmarkTask = toml::from_str(&text).map_err(|e| CtxError::Config(format!("{}: {e}", file.display())))?;
        task.source = file;
        tasks.push(task);
    }
    Ok(tasks)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalQuery {
    pub repository: String,
    pub query: String,
    pub relevant_files: Vec<String>,
}

pub fn load_queries(path: &Path) -> Result<Vec<RetrievalQuery>> {
    let text = std::fs::read_to_string(path)?;
    text.lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str(l).map_err(CtxError::from)).collect()
}
