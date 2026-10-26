//! Connection wrapper with migrations.

use ctx_core::{CtxError, Result};
use parking_lot::Mutex;
use rusqlite::{Connection, OpenFlags, Transaction};
use std::path::{Path, PathBuf};

pub fn map_err(e: rusqlite::Error) -> CtxError {
    CtxError::Storage(e.to_string())
}

/// A single-writer SQLite connection guarded by a mutex. Reads and writes
/// serialize through the mutex; SQLite's WAL mode keeps readers from other
/// processes unblocked.
pub struct Database {
    path: Option<PathBuf>,
    conn: Mutex<Connection>,
}

pub struct Migration {
    pub version: i64,
    pub sql: &'static str,
}

impl Database {
    pub fn open(path: &Path, migrations: &[Migration]) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(map_err)?;
        conn.pragma_update(None, "journal_mode", "WAL").map_err(map_err)?;
        conn.pragma_update(None, "synchronous", "NORMAL").map_err(map_err)?;
        conn.pragma_update(None, "foreign_keys", "ON").map_err(map_err)?;
        conn.busy_timeout(std::time::Duration::from_secs(10)).map_err(map_err)?;
        let db = Self { path: Some(path.to_path_buf()), conn: Mutex::new(conn) };
        db.migrate(migrations)?;
        Ok(db)
    }

    pub fn in_memory(migrations: &[Migration]) -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(map_err)?;
        conn.pragma_update(None, "foreign_keys", "ON").map_err(map_err)?;
        let db = Self { path: None, conn: Mutex::new(conn) };
        db.migrate(migrations)?;
        Ok(db)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn migrate(&self, migrations: &[Migration]) -> Result<()> {
        self.with_tx(|tx| {
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);",
            )?;
            let current: i64 = tx
                .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |r| r.get(0))?;
            for m in migrations {
                if m.version > current {
                    tx.execute_batch(m.sql)?;
                    tx.execute(
                        "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                        rusqlite::params![m.version, chrono::Utc::now().to_rfc3339()],
                    )?;
                }
            }
            Ok(())
        })
    }

    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let conn = self.conn.lock();
        f(&conn).map_err(map_err)
    }

    pub fn with_tx<T>(&self, f: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<T>) -> Result<T> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction().map_err(map_err)?;
        let out = f(&tx).map_err(map_err)?;
        tx.commit().map_err(map_err)?;
        Ok(out)
    }

    /// Run `VACUUM` and `wal_checkpoint`; used by index repair.
    pub fn compact(&self) -> Result<()> {
        self.with(|c| {
            c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
            Ok(())
        })
    }
}

/// Serialize a value to JSON text for storage, mapping errors.
pub fn to_json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(CtxError::from)
}

pub fn from_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T> {
    serde_json::from_str(text).map_err(CtxError::from)
}

pub fn now_text() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn parse_ts(text: &str) -> ctx_core::Timestamp {
    chrono::DateTime::parse_from_rfc3339(text)
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now())
}
