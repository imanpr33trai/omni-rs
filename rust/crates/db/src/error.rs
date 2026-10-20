//! Database error types

use rusqlite::Error as RusqliteError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("connection failed: {0}")]
    Connection(#[from] RusqliteError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("migration error: {0}")]
    Migration(String),

    #[error("pool exhausted: no connections available")]
    PoolExhausted,

    #[error("database locked: {0}")]
    Locked(String),
}

pub type DbResult<T> = Result<T, DbError>;