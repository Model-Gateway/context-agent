//! The spend ledger (`<data_dir>/spend.sqlite`): one row per answered model
//! request from every repository, so spending limits (`[spend]`) count all
//! of them. Session stores keep their own per-request costs for task totals.

use crate::db::{Database, Migration};
use crate::records::ModelSpend;
use ctx_core::paths::DataLayout;
use ctx_core::{Result, Timestamp};
use rusqlite::params;

const SPEND_MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: r#"
CREATE TABLE IF NOT EXISTS spend (
    id INTEGER PRIMARY KEY,
    model TEXT NOT NULL,
    cost_usd REAL,
    gateway_request_id TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS spend_created ON spend(created_at, model);
CREATE INDEX IF NOT EXISTS spend_gateway ON spend(gateway_request_id);
"#,
}];

pub struct SpendLedger {
    db: Database,
}

impl SpendLedger {
    pub fn open(layout: &DataLayout) -> Result<Self> {
        Ok(Self { db: Database::open(&layout.root.join("spend.sqlite"), SPEND_MIGRATIONS)? })
    }

    pub fn in_memory() -> Result<Self> {
        Ok(Self { db: Database::in_memory(SPEND_MIGRATIONS)? })
    }

    /// One answered request; `cost_usd` is `None` without a known price.
    pub fn record(&self, model: &str, cost_usd: Option<f64>, gateway_request_id: Option<&str>, at: Timestamp) -> Result<()> {
        self.db.with(|c| c.execute("INSERT INTO spend (model, cost_usd, gateway_request_id, created_at) VALUES (?1, ?2, ?3, ?4)", params![model, cost_usd, gateway_request_id, at.to_rfc3339()]).map(|_| ()))
    }

    /// What the gateway billed replaces the estimate.
    pub fn set_billed_cost(&self, gateway_request_id: &str, cost_usd: f64) -> Result<usize> {
        self.db.with(|c| c.execute("UPDATE spend SET cost_usd = ?2 WHERE gateway_request_id = ?1", params![gateway_request_id, cost_usd]))
    }

    /// USD spent since `since` (all models, or one).
    pub fn spent_since(&self, since: Timestamp, model: Option<&str>) -> Result<f64> {
        self.db.with(|c| match model {
            Some(m) => c.query_row("SELECT COALESCE(SUM(cost_usd),0) FROM spend WHERE created_at >= ?1 AND model = ?2", params![since.to_rfc3339(), m], |r| r.get(0)),
            None => c.query_row("SELECT COALESCE(SUM(cost_usd),0) FROM spend WHERE created_at >= ?1", params![since.to_rfc3339()], |r| r.get(0)),
        })
    }

    /// Spend per model since `since`, biggest first.
    pub fn by_model(&self, since: Timestamp) -> Result<Vec<ModelSpend>> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT model, COALESCE(SUM(cost_usd),0), COUNT(*), COUNT(*) - COUNT(cost_usd) FROM spend WHERE created_at >= ?1 GROUP BY model ORDER BY 2 DESC, model ASC")?;
            let rows = stmt.query_map(params![since.to_rfc3339()], |r| Ok(ModelSpend { model: r.get(0)?, cost_usd: r.get(1)?, requests: r.get::<_, i64>(2)? as u64, unpriced_requests: r.get::<_, i64>(3)? as u64 }))?;
            rows.collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_per_period_and_model_and_takes_billed_costs() {
        let ledger = SpendLedger::in_memory().unwrap();
        let now = ctx_core::now();
        ledger.record("a", Some(0.25), Some("gw-1"), now).unwrap();
        ledger.record("b", None, None, now).unwrap();
        ledger.record("a", Some(1.0), None, now - chrono::Duration::days(40)).unwrap();
        let since = now - chrono::Duration::hours(1);
        assert_eq!(ledger.set_billed_cost("gw-1", 0.5).unwrap(), 1);
        assert!((ledger.spent_since(since, None).unwrap() - 0.5).abs() < 1e-9);
        assert!((ledger.spent_since(since, Some("b")).unwrap()).abs() < 1e-9);
        let rows = ledger.by_model(since).unwrap();
        assert_eq!(rows.iter().map(|r| (r.model.as_str(), r.requests, r.unpriced_requests)).collect::<Vec<_>>(), vec![("a", 1, 0), ("b", 1, 1)]);
    }
}
