//! Database error types.

use rusqlite::Error as RusqliteError;
use std::fmt;
use thiserror::Error;

/// Result type for database operations.
pub type DbResult<T> = Result<T, DbError>;

/// Database error type.
#[derive(Error, Debug)]
pub enum DbError {
    #[error("connection error: {0}")]
    Connection(String),

    #[error("migration error: {0}")]
    Migration(String),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] RusqliteError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("config error: {0}")]
    Config(String),
}

impl From<rusqlite::Error> for DbError {
    fn from(err: rusqlite::Error) -> Self {
        DbError::Sqlite(err)
    }
}

impl From<std::io::Error> for DbError {
    fn from(err: std::io::Error) -> Self {
        DbError::Io(err)
    }
}