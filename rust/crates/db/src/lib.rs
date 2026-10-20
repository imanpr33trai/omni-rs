//! OmniRoute database layer — Phase 2.1: rusqlite core.
//!
//! Ports the TS `src/lib/db/` layer with exact parity:
//! - rusqlite with `bundled` + `fts5` + `load_extension`
//! - WAL mode pragmas (busy_timeout=2000, journal_mode=WAL, synchronous=NORMAL, etc.)
//! - DATA_DIR resolution (~/.omniroute/storage.sqlite)
//! - Connection pool with configurable size
//! - Migration runner integration point

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;

pub mod error;
pub mod pool;

use config::AppConfig;
use error::DbError;
use pool::ConnectionPool;

/// Result type for database operations
pub type DbResult<T> = Result<T, DbError>;

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbConfig {
    pub path: PathBuf,
    pub pool_size: usize,
    pub busy_timeout_ms: u32,
    pub cache_size_kib: i32,
    pub mmap_size_mib: i32,
    pub enable_wal: bool,
    pub enable_fts5: bool,
}

impl Default for DbConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("~/.omniroute/storage.sqlite"),
            pool_size: 10,
            busy_timeout_ms: 2000,
            cache_size_kib: -32768, // 32 MiB (negative = KiB)
            mmap_size_mib: 256,
            enable_wal: true,
            enable_fts5: true,
        }
    }
}

impl DbConfig {
    /// Build config from AppConfig
    pub fn from_app_config(app_config: &AppConfig) -> DbResult<Self> {
        let data_dir = app_config
            .storage
            .data_dir
            .as_deref()
            .or(app_config.storage.omniroute_data_dir.as_deref())
            .unwrap_or("~/.omniroute");

        let expanded = shellexpand::tilde(data_dir).to_string();
        let path = PathBuf::from(expanded).join("storage.sqlite");

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        Ok(Self {
            path,
            pool_size: 10,
            busy_timeout_ms: 2000,
            cache_size_kib: -32768,
            mmap_size_mib: 256,
            enable_wal: true,
            enable_fts5: true,
        })
    }
}

/// Open a single connection with all pragmas applied
fn open_connection(config: &DbConfig) -> DbResult<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;

    let mut conn = Connection::open_with_flags(&config.path, flags)?;

    // Apply pragmas (ported from TS: src/lib/db/core.ts)
    apply_pragmas(&mut conn, config)?;

    Ok(conn)
}

/// Apply all database pragmas — exact parity with TS `src/lib/db/core.ts`
fn apply_pragmas(conn: &mut Connection, config: &DbConfig) -> DbResult<()> {
    // Busy timeout (ms)
    conn.pragma_update(None, "busy_timeout", config.busy_timeout_ms)?;

    // WAL mode
    if config.enable_wal {
        conn.pragma_update(None, "journal_mode", "WAL")?;
    }

    // Synchronous = NORMAL (TS default)
    conn.pragma_update(None, "synchronous", "NORMAL")?;

    // Temp store = MEMORY
    conn.pragma_update(None, "temp_store", "MEMORY")?;

    // Cache size (negative = KiB)
    conn.pragma_update(None, "cache_size", config.cache_size_kib)?;

    // mmap_size (bytes)
    conn.pragma_update(
        None,
        "mmap_size",
        (config.mmap_size_mib as i64) * 1024 * 1024,
    )?;

    // Page size (optional, default 4096)
    // conn.pragma_update(None, "page_size", 4096)?;

    // Foreign keys ON
    conn.pragma_update(None, "foreign_keys", "ON")?;

    // Recursive triggers ON (for FTS5 triggers)
    conn.pragma_update(None, "recursive_triggers", "ON")?;

    Ok(())
}

/// Main database handle — wraps a connection pool
pub struct Database {
    pool: Arc<ConnectionPool>,
    config: DbConfig,
}

impl Database {
    /// Open a new database with the given config
    pub fn new(config: DbConfig) -> DbResult<Self> {
        // Test connection first
        let _test_conn = open_connection(&config)?;

        let pool = ConnectionPool::new(config.pool_size, config.clone())?;
        Ok(Self {
            pool: Arc::new(pool),
            config,
        })
    }

    /// Open from AppConfig (loads env, DATA_DIR, etc.)
    pub fn from_app_config(app_config: &AppConfig) -> DbResult<Self> {
        let config = DbConfig::from_app_config(app_config)?;
        Self::new(config)
    }

    /// Get a connection from the pool
    pub fn conn(&self) -> DbResult<pool::PooledConnection> {
        self.pool.get()
    }

    /// Execute a closure with a connection
    pub fn with_conn<F, T>(&self, f: F) -> DbResult<T>
    where
        F: FnOnce(&Connection) -> DbResult<T>,
    {
        let conn = self.conn()?;
        f(&conn)
    }

    /// Execute a transaction
    pub fn transaction<F, T>(&self, f: F) -> DbResult<T>
    where
        F: FnOnce(&Connection) -> DbResult<T>,
    {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    }

    /// Get the database path
    pub fn path(&self) -> &Path {
        &self.config.path
    }

    /// Get the pool size
    pub fn pool_size(&self) -> usize {
        self.pool.size()
    }

    /// Health check — verify DB is accessible
    pub fn health_check(&self) -> DbResult<ComponentHealth> {
        self.with_conn(|conn| {
            conn.query_row("SELECT 1", [], |_| Ok(()))?;
            Ok(())
        })?;
        Ok(ComponentHealth {
            status: "ok".to_string(),
            details: Some(serde_json::json!({
                "path": self.config.path.to_string_lossy(),
                "pool_size": self.config.pool_size,
            })),
        })
    }

    /// Run VACUUM INTO for backup (TS: `managedBackup.ts`)
    pub fn vacuum_into(&self, backup_path: &Path) -> DbResult<()> {
        self.with_conn(|conn| {
            conn.execute(&format!("VACUUM INTO '{}'", backup_path.display()), [])?;
            Ok(())
        })
    }

    /// WAL checkpoint (TS: `walMaintenance.ts` — TRUNCATE only at shutdown)
    pub fn wal_checkpoint_truncate(&self) -> DbResult<()> {
        self.with_conn(|conn| {
            conn.execute("PRAGMA wal_checkpoint(TRUNCATE)", [])?;
            Ok(())
        })
    }

    /// Get the underlying pool for advanced operations
    pub fn pool(&self) -> Arc<ConnectionPool> {
        self.pool.clone()
    }
}

/// Health component status (mirrors auth::HealthResponse::ComponentHealth)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    pub status: String, // "ok" | "degraded" | "down"
    pub details: Option<serde_json::Value>,
}

impl Database {
    /// Initialize the database schema (run migrations)
    /// This is a placeholder — full migration runner in Phase 2.2
    pub fn init_schema(&self) -> DbResult<()> {
        self.with_conn(|conn| {
            // Create migration tracking table (TS: `_omniroute_migrations`)
            conn.execute(
                "CREATE TABLE IF NOT EXISTS _omniroute_migrations (
                    version TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                )",
                [],
            )?;
            Ok(())
        })
    }
}

/// Convenience: open default database from environment
pub fn open_default() -> DbResult<Database> {
    let config = DbConfig::default();
    // Expand tilde in path
    let expanded = shellexpand::tilde(&config.path.to_string_lossy()).to_string();
    let mut config = config;
    config.path = PathBuf::from(expanded);
    Database::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_open_and_pragmas() -> DbResult<()> {
        let tmp = TempDir::new()?;
        let path = tmp.path().join("test.db");
        let config = DbConfig {
            path: path.clone(),
            pool_size: 2,
            ..Default::default()
        };

        let db = Database::new(config)?;
        let health = db.health_check()?;
        assert_eq!(health.status, "ok");
        assert_eq!(db.pool_size(), 2);
        Ok(())
    }

    #[test]
    fn test_pool_reuse() -> DbResult<()> {
        let tmp = TempDir::new()?;
        let path = tmp.path().join("test.db");
        let config = DbConfig {
            path,
            pool_size: 2,
            ..Default::default()
        };

        let db = Database::new(config)?;

        // Get connection, use it, return it
        {
            let conn = db.conn()?;
            conn.execute("CREATE TABLE test (id INTEGER PRIMARY KEY)", [])?;
            conn.execute("INSERT INTO test (id) VALUES (1)", [])?;
        }

        // Get another connection, verify data persists
        {
            let conn = db.conn()?;
            let count: i64 = conn.query_row("SELECT COUNT(*) FROM test", [], |r| r.get(0))?;
            assert_eq!(count, 1);
        }
        Ok(())
    }

    #[test]
    fn test_transaction() -> DbResult<()> {
        let tmp = TempDir::new()?;
        let path = tmp.path().join("test.db");
        let config = DbConfig {
            path,
            pool_size: 2,
            ..Default::default()
        };

        let db = Database::new(config)?;

        db.transaction(|conn| {
            conn.execute(
                "CREATE TABLE txn_test (id INTEGER PRIMARY KEY, val TEXT)",
                [],
            )?;
            conn.execute("INSERT INTO txn_test (val) VALUES ('hello')", [])?;
            Ok(())
        })?;

        let count: i64 = db.with_conn(|conn| {
            conn.query_row("SELECT COUNT(*) FROM txn_test", [], |r| r.get(0))
                .map_err(DbError::Connection)
        })?;
        assert_eq!(count, 1);
        Ok(())
    }

    #[test]
    fn test_vacuum_into() -> DbResult<()> {
        let tmp = TempDir::new()?;
        let path = tmp.path().join("test.db");
        let backup = tmp.path().join("backup.db");
        let config = DbConfig {
            path,
            pool_size: 1,
            ..Default::default()
        };

        let db = Database::new(config)?;
        db.with_conn(|conn| {
            conn.execute("CREATE TABLE data (id INTEGER PRIMARY KEY)", [])?;
            conn.execute("INSERT INTO data (id) VALUES (1)", [])?;
            conn.execute("INSERT INTO data (id) VALUES (2)", [])?;
            Ok(())
        })?;

        db.vacuum_into(&backup)?;
        assert!(backup.exists());

        // Verify backup is readable
        let conn = Connection::open(&backup)?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM data", [], |r| r.get(0))?;
        assert_eq!(count, 2);
        Ok(())
    }
}
