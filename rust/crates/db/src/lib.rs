//! db: rusqlite core, migration runner, domain modules, field encryption.
//!
//! Scaffolded in Phase 0 — implementation lands in Phase 2 per the
//! migration plan (`_tasks/superpowers/plans/2026-10-17-rust-backend-migration.md`).

pub mod database;
mod error;
pub mod migration;

pub use database::{Database, DbConfig};
pub use error::{DbError, DbResult};
pub use migration::{
    MigrationConfig,
    MigrationState,
    MigrationFile,
    MigrationRecord,
    run_migration_pipeline,
    run_migrations,
    scan_migrations,
    ensure_001_auto_seed,
    column_healers,
};

/// Marker test: keeps `cargo test --workspace` exercising this crate from day one.
#[test]
fn crate_scaffolded() {
    assert_eq!(env!("CARGO_PKG_NAME"), "db");
}
