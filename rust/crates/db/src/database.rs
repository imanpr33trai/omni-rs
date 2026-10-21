//! Database connection pool and core operations.
//!
//! Provides a thread-safe wrapper around `rusqlite::Connection` using `parking_lot::Mutex`.

use crate::{DbConfig, DbError, DbResult};
use parking_lot::Mutex;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::sync::Arc;

/// Database connection wrapper with thread-safe access.
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
    config: DbConfig,
}

impl Database {
    /// Create a new database connection.
    pub fn new(config: DbConfig) -> DbResult<Self> {
        let mut conn = Connection::open_with_flags(
            &config.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_URI,
        )
        .map_err(DbError::from)?;

        // Enable WAL mode for better concurrency
        conn.execute_batch("PRAGMA journal_mode = WAL;")?;
        // Enable foreign keys
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        // Busy timeout for contention
        conn.execute_batch("PRAGMA busy_timeout = 5000;")?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            config,
        })
    }

    /// Get a connection from the pool and run a closure.
    pub fn with_conn<F, R>(&self, f: F) -> DbResult<R>
    where
        F: FnOnce(&mut Connection) -> DbResult<R>,
    {
        let mut conn = self.conn.lock();
        f(&mut conn)
    }

    /// Run a closure within an IMMEDIATE transaction (writer lock).
    pub fn with_tx<F, R>(&self, f: F) -> DbResult<R>
    where
        F: FnOnce(&mut Connection) -> DbResult<R>,
    {
        self.with_conn(|conn| {
            let tx = conn.transaction().map_err(DbError::from)?;
            let result = f(tx)?;
            tx.commit().map_err(DbError::from)?;
            Ok(result)
        })
    }

    /// Get the database configuration.
    pub fn config(&self) -> &DbConfig {
        &self.config
    }

    /// Initialize the schema (create _omniroute_migrations table).
    pub fn init_schema(&self) -> DbResult<()> {
        self.with_conn(|conn| {
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS _omniroute_migrations (
                    version TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                "#,
            )?;
            Ok(())
        })
    }

    /// Perform VACUUM INTO for pre-migration backup.
    pub fn vacuum_into<P: AsRef<Path>>(source: P, dest: P) -> DbResult<()> {
        let mut conn = Connection::open_with_flags(
            source,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(DbError::from)?;

        let dest_str = dest.as_ref().to_string_lossy();
        conn.execute_batch(&format!("VACUUM INTO '{}';", dest_str))
            .map_err(DbError::from)?;
        Ok(())
    }
}

/// Database configuration.
#[derive(Debug, Clone)]
pub struct DbConfig {
    pub path: String,
    pub max_connections: u32, // Kept for compatibility, not used with single connection
}

impl Default for DbConfig {
    fn default() -> Self {
        Self {
            path: ":memory:".to_string(),
            max_connections: 10,
        }
    }
}

impl DbConfig {
    pub fn new<P: Into<String>>(path: P) -> Self {
        Self {
            path: path.into(),
            max_connections: 10,
        }
    }
}