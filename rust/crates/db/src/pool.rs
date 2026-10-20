//! Connection pool for rusqlite connections

use crate::DbConfig;
use crate::error::{DbError, DbResult};
use rusqlite::{Connection, OpenFlags};
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

/// A pooled connection that returns to the pool on drop
pub struct PooledConnection {
    conn: Option<Connection>,
    pool: Arc<InnerPool>,
}

impl PooledConnection {
    /// Get a reference to the underlying connection
    pub fn conn(&self) -> &Connection {
        self.conn
            .as_ref()
            .expect("connection already returned to pool")
    }

    /// Get a mutable reference to the underlying connection
    pub fn conn_mut(&mut self) -> &mut Connection {
        self.conn
            .as_mut()
            .expect("connection already returned to pool")
    }
}

impl Drop for PooledConnection {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.return_connection(conn);
        }
    }
}

impl std::ops::Deref for PooledConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        self.conn()
    }
}

impl std::ops::DerefMut for PooledConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.conn_mut()
    }
}

/// Inner pool state
struct InnerPool {
    config: DbConfig,
    available: Mutex<VecDeque<Connection>>,
    in_use: Mutex<usize>,
    condvar: Condvar,
    total_created: Mutex<usize>,
}

impl InnerPool {
    /// Return a connection to the pool
    fn return_connection(&self, conn: Connection) {
        let mut available = self.available.lock().unwrap();
        available.push_back(conn);
        drop(available);

        let mut in_use = self.in_use.lock().unwrap();
        *in_use = in_use.saturating_sub(1);
        drop(in_use);

        self.condvar.notify_one();
    }
}

/// Thread-safe connection pool
pub struct ConnectionPool {
    inner: Arc<InnerPool>,
}

impl ConnectionPool {
    /// Create a new connection pool
    pub fn new(pool_size: usize, config: DbConfig) -> DbResult<Self> {
        let mut available = VecDeque::new();

        // Pre-create connections
        for _ in 0..pool_size {
            let conn = create_connection(&config)?;
            available.push_back(conn);
        }

        let inner = Arc::new(InnerPool {
            config,
            available: Mutex::new(available),
            in_use: Mutex::new(0),
            condvar: Condvar::new(),
            total_created: Mutex::new(pool_size),
        });

        Ok(Self { inner })
    }

    /// Get a connection from the pool (blocks until available)
    pub fn get(&self) -> DbResult<PooledConnection> {
        let mut available = self.inner.available.lock().unwrap();

        // Wait for a connection to become available
        while available.is_empty() {
            let mut in_use = self.inner.in_use.lock().unwrap();
            if *in_use < self.inner.config.pool_size {
                // We can create a new connection (up to pool_size)
                drop(available);
                drop(in_use);
                let conn = create_connection(&self.inner.config)?;
                let mut in_use = self.inner.in_use.lock().unwrap();
                *in_use += 1;
                return Ok(PooledConnection {
                    conn: Some(conn),
                    pool: self.inner.clone(),
                });
            }
            // Wait for a connection to be returned
            available = self.inner.condvar.wait(available).unwrap();
        }

        let conn = available.pop_front().unwrap();
        drop(available);

        let mut in_use = self.inner.in_use.lock().unwrap();
        *in_use += 1;

        Ok(PooledConnection {
            conn: Some(conn),
            pool: self.inner.clone(),
        })
    }

    /// Return a connection to the pool
    fn return_connection(&self, conn: Connection) {
        let mut available = self.inner.available.lock().unwrap();
        available.push_back(conn);
        drop(available);

        let mut in_use = self.inner.in_use.lock().unwrap();
        *in_use = in_use.saturating_sub(1);
        drop(in_use);

        self.inner.condvar.notify_one();
    }

    /// Get the pool size
    pub fn size(&self) -> usize {
        self.inner.config.pool_size
    }

    /// Get the number of connections currently in use
    pub fn in_use(&self) -> usize {
        *self.inner.in_use.lock().unwrap()
    }

    /// Get the number of available connections
    pub fn available(&self) -> usize {
        self.inner.available.lock().unwrap().len()
    }
}

/// Create a single connection with all pragmas applied
fn create_connection(config: &DbConfig) -> DbResult<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;

    let mut conn = Connection::open_with_flags(&config.path, flags)?;

    // Apply pragmas
    conn.pragma_update(None, "busy_timeout", config.busy_timeout_ms)?;
    if config.enable_wal {
        conn.pragma_update(None, "journal_mode", "WAL")?;
    }
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.pragma_update(None, "cache_size", config.cache_size_kib)?;
    conn.pragma_update(
        None,
        "mmap_size",
        (config.mmap_size_mib as i64) * 1024 * 1024,
    )?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "recursive_triggers", "ON")?;

    Ok(conn)
}

impl InnerPool {
    /// Get the pool configuration
    pub fn config(&self) -> &DbConfig {
        &self.config
    }
}

impl ConnectionPool {
    /// Create a pool with a custom connection factory (for testing)
    #[cfg(test)]
    pub fn new_with_factory<F>(pool_size: usize, config: DbConfig, factory: F) -> DbResult<Self>
    where
        F: Fn() -> DbResult<Connection> + Send + Sync + 'static,
    {
        // For simplicity, we'll use the standard factory in tests
        Self::new(pool_size, config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use tempfile::TempDir;

    #[test]
    fn test_pool_basic() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let config = DbConfig {
            path,
            pool_size: 3,
            ..Default::default()
        };

        let pool = ConnectionPool::new(3, config).unwrap();

        // Get a connection
        let conn = pool.get().unwrap();
        assert_eq!(pool.in_use(), 1);
        assert_eq!(pool.available(), 2);

        // Use the connection
        conn.execute("CREATE TABLE test (id INTEGER)", []).unwrap();

        // Drop it back to pool
        drop(conn);
        assert_eq!(pool.in_use(), 0);
        assert_eq!(pool.available(), 3);
    }

    #[test]
    fn test_pool_exhaustion_and_wait() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let config = DbConfig {
            path,
            pool_size: 2,
            ..Default::default()
        };

        let pool = Arc::new(ConnectionPool::new(2, config).unwrap());
        let pool_clone = pool.clone();

        // Take both connections
        let conn1 = pool.get().unwrap();
        let conn2 = pool.get().unwrap();
        assert_eq!(pool.in_use(), 2);
        assert_eq!(pool.available(), 0);

        // Try to get a third in a thread (should wait)
        let handle = thread::spawn(move || {
            let _conn3 = pool_clone.get().unwrap();
        });

        // Give the thread time to start waiting
        thread::sleep(Duration::from_millis(50));

        // Return one connection
        drop(conn1);

        // Thread should now get the connection
        handle.join().unwrap();

        assert_eq!(pool.in_use(), 1);
    }

    #[test]
    fn test_concurrent_access() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let config = DbConfig {
            path,
            pool_size: 5,
            ..Default::default()
        };

        let pool = Arc::new(ConnectionPool::new(5, config).unwrap());

        // Initialize DB
        {
            let conn = pool.get().unwrap();
            conn.execute(
                "CREATE TABLE counter (id INTEGER PRIMARY KEY, val INTEGER)",
                [],
            )
            .unwrap();
            conn.execute("INSERT INTO counter (val) VALUES (0)", [])
                .unwrap();
        }

        // Spawn multiple threads to increment
        let mut handles = vec![];
        for _ in 0..10 {
            let pool_clone = pool.clone();
            handles.push(thread::spawn(move || {
                let conn = pool_clone.get().unwrap();
                conn.execute("UPDATE counter SET val = val + 1 WHERE id = 1", [])
                    .unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Verify final count
        let conn = pool.get().unwrap();
        let val: i64 = conn
            .query_row("SELECT val FROM counter WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(val, 10);
    }
}
