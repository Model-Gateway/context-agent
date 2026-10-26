//! Retrieval evaluation over the labeled benchmark queries must meet a
//! floor; this guards the ranking against regressions.

use ctx_evaluation::{evaluate_retrieval, spec::load_queries};
use ctx_integration_tests::{test_config, workspace_root};

#[tokio::test]
async fn labeled_queries_recall_floor() {
    let root = workspace_root();
    let queries_path = root.join("benchmarks/retrieval/queries.jsonl");
    if !queries_path.exists() {
        eprintln!("no labeled queries; skipping");
        return;
    }
    let queries = load_queries(&queries_path).unwrap();
    let data = tempfile::tempdir().unwrap();
    let report = evaluate_retrieval(&queries, &root, &test_config(data.path())).await.unwrap();
    eprintln!("{}", report.render());
    assert!(report.queries >= 20);
    assert!(report.mean_recall_at_10 >= 0.75, "recall@10 {:.2} below floor", report.mean_recall_at_10);
    assert!(report.mrr >= 0.5, "MRR {:.3} below floor", report.mrr);
}
