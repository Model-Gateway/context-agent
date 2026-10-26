//! Labeled retrieval evaluation: recall@k and MRR (spec §7.9, §15.6).

use crate::spec::RetrievalQuery;
use ctx_agent::RepositoryContext;
use ctx_core::{Config, Result};
use ctx_retrieval::{RetrievalRequest, RetrievalStage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryResult {
    pub repository: String,
    pub query: String,
    pub relevant: Vec<String>,
    pub retrieved: Vec<String>,
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub reciprocal_rank: f64,
    pub confidence: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalEvalReport {
    pub queries: usize,
    pub mean_recall_at_5: f64,
    pub mean_recall_at_10: f64,
    pub mrr: f64,
    pub confidence_calibration: BTreeMap<String, (usize, f64)>,
    pub results: Vec<QueryResult>,
}

pub async fn evaluate_retrieval(queries: &[RetrievalQuery], workspace_root: &Path, config: &Config) -> Result<RetrievalEvalReport> {
    let mut results = Vec::new();
    let mut contexts: BTreeMap<String, std::sync::Arc<RepositoryContext>> = BTreeMap::new();
    for q in queries {
        let ctx = match contexts.get(&q.repository) {
            Some(c) => c.clone(),
            None => {
                let root = workspace_root.join(&q.repository);
                let c = RepositoryContext::open_ephemeral(&root, config.clone())?;
                c.ensure_indexed(None)?;
                contexts.insert(q.repository.clone(), c.clone());
                c
            }
        };
        let started = std::time::Instant::now();
        let mut req = RetrievalRequest::new(q.query.clone(), 8_000);
        req.stage = RetrievalStage::Initial;
        let r = ctx.retrieval.retrieve(req).await?;
        let mut retrieved: Vec<String> = Vec::new();
        for c in &r.candidates {
            if !retrieved.contains(&c.path) {
                retrieved.push(c.path.clone());
            }
        }
        let recall = |k: usize| -> f64 {
            if q.relevant_files.is_empty() {
                return 1.0;
            }
            let top: Vec<&String> = retrieved.iter().take(k).collect();
            q.relevant_files.iter().filter(|f| top.contains(f)).count() as f64 / q.relevant_files.len() as f64
        };
        let rr = retrieved.iter().position(|p| q.relevant_files.contains(p)).map(|i| 1.0 / (i as f64 + 1.0)).unwrap_or(0.0);
        let (r5, r10) = (recall(5), recall(10));
        let top10: Vec<String> = retrieved.iter().take(10).cloned().collect();
        results.push(QueryResult { repository: q.repository.clone(), query: q.query.clone(), relevant: q.relevant_files.clone(), retrieved: top10, recall_at_5: r5, recall_at_10: r10, reciprocal_rank: rr, confidence: r.confidence.as_str().into(), duration_ms: started.elapsed().as_millis() as u64 });
    }
    let n = results.len().max(1) as f64;
    let mut calibration: BTreeMap<String, (usize, f64)> = BTreeMap::new();
    for r in &results {
        let e = calibration.entry(r.confidence.clone()).or_insert((0, 0.0));
        e.0 += 1;
        e.1 += r.recall_at_5;
    }
    for v in calibration.values_mut() {
        if v.0 > 0 {
            v.1 /= v.0 as f64;
        }
    }
    Ok(RetrievalEvalReport {
        queries: results.len(),
        mean_recall_at_5: results.iter().map(|r| r.recall_at_5).sum::<f64>() / n,
        mean_recall_at_10: results.iter().map(|r| r.recall_at_10).sum::<f64>() / n,
        mrr: results.iter().map(|r| r.reciprocal_rank).sum::<f64>() / n,
        confidence_calibration: calibration,
        results,
    })
}

impl RetrievalEvalReport {
    pub fn render(&self) -> String {
        let mut s = format!("Retrieval evaluation over {} queries: recall@5 {:.1}%, recall@10 {:.1}%, MRR {:.3}\n", self.queries, self.mean_recall_at_5 * 100.0, self.mean_recall_at_10 * 100.0, self.mrr);
        s.push_str("Confidence calibration (label: queries, mean recall@5):\n");
        for (k, (n, r)) in &self.confidence_calibration {
            s.push_str(&format!("  {k}: {n} queries, {:.1}%\n", r * 100.0));
        }
        for r in &self.results {
            s.push_str(&format!("{} {:>5.0}% [{}] {} → {}\n", if r.recall_at_5 >= 1.0 { "ok  " } else { "MISS" }, r.recall_at_5 * 100.0, r.confidence, r.query, r.retrieved.iter().take(3).cloned().collect::<Vec<_>>().join(", ")));
        }
        s
    }
}
