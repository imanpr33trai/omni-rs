//! Integration tests for the migration runner.
//!
//! These tests verify that the migration pipeline works correctly against
//! a temporary SQLite database.

use db::{Database, DbConfig, MigrationConfig, MigrationState};
use std::path::PathBuf;
use tempfile::TempDir;

/// Create a temporary database for testing
fn create_test_db() -> (Database, TempDir) {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("test.db");
    let config = DbConfig::new(db_path.to_string_lossy().to_string());
    let db = Database::new(config).unwrap();
    (db, _tmp)
}

/// Create a temporary migrations directory with test migration files
fn create_test_migrations_dir() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let migrations_dir = tmp.path().join("migrations");
    std::fs::create_dir_all(&migrations_dir).unwrap();

    // Migration 001: Initial schema
    std::fs::write(
        migrations_dir.join("001_initial_schema.sql"),
        r#"
        CREATE TABLE IF NOT EXISTS test_table (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS _omniroute_migrations (
            version TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        "#,
    )
    .unwrap();

    // Migration 002: Add column
    std::fs::write(
        migrations_dir.join("002_add_column.sql"),
        r#"
        ALTER TABLE test_table ADD COLUMN email TEXT;
        CREATE INDEX IF NOT EXISTS idx_test_table_email ON test_table(email);
        "#,
    )
    .unwrap();

    // Migration 003: Another table
    std::fs::write(
        migrations_dir.join("003_another_table.sql"),
        r#"
        CREATE TABLE IF NOT EXISTS another_table (
            id INTEGER PRIMARY KEY,
            value TEXT
        );
        "#,
    )
    .unwrap();

    (tmp, migrations_dir)
}

#[test]
fn test_scan_migrations() {
    let (_tmp, migrations_dir) = create_test_migrations_dir();
    let migrations = db::scan_migrations(&migrations_dir).unwrap();

    assert_eq!(migrations.len(), 3);
    assert_eq!(migrations[0].version, "001");
    assert_eq!(migrations[0].name, "001_initial_schema");
    assert_eq!(migrations[1].version, "002");
    assert_eq!(migrations[1].name, "002_add_column");
    assert_eq!(migrations[2].version, "003");
    assert_eq!(migrations[2].name, "003_another_table");
}

#[test]
fn test_migration_ordering() {
    let tmp = TempDir::new().unwrap();
    let migrations_dir = tmp.path().join("migrations");
    std::fs::create_dir_all(&migrations_dir).unwrap();

    // Write in non-sequential order
    std::fs::write(migrations_dir.join("010_ten.sql"), "SELECT 1;").unwrap();
    std::fs::write(migrations_dir.join("002_two.sql"), "SELECT 1;").unwrap();
    std::fs::write(migrations_dir.join("001_one.sql"), "SELECT 1;").unwrap();

    let migrations = db::scan_migrations(&migrations_dir).unwrap();
    assert_eq!(migrations.len(), 3);
    assert_eq!(migrations[0].version, "001");
    assert_eq!(migrations[1].version, "002");
    assert_eq!(migrations[2].version, "010");
}

#[test]
fn test_run_migration_pipeline_on_empty_db() {
    let (db, _tmp) = create_test_db();
    let (_migrations_tmp, migrations_dir) = create_test_migrations_dir();

    let config = MigrationConfig {
        migrations_dir: migrations_dir.clone(),
        mass_migration_threshold: 50,
        enable_backup: false,
        backup_dir: None,
    };

    let state = db::run_migrations(&db, &config).unwrap();

    // All 3 migrations should be applied
    assert!(!state.mass_abort);
    assert_eq!(state.applied.len(), 3);
    assert_eq!(state.pending.len(), 3);

    // Verify versions
    let versions: Vec<&str> = state.applied.iter().map(|m| m.version.as_str()).collect();
    assert_eq!(versions, vec!["001", "002", "003"]);
}

#[test]
fn test_run_migration_pipeline_idempotent() {
    let (db, _tmp) = create_test_db();
    let (_migrations_tmp, migrations_dir) = create_test_migrations_dir();

    let config = MigrationConfig {
        migrations_dir: migrations_dir.clone(),
        mass_migration_threshold: 50,
        enable_backup: false,
        backup_dir: None,
    };

    // Run migrations first time
    let state1 = db::run_migrations(&db, &config).unwrap();
    assert_eq!(state1.applied.len(), 3);

    // Run migrations second time - should be idempotent
    let state2 = db::run_migrations(&db, &config).unwrap();
    assert_eq!(state2.applied.len(), 3); // Still 3, no duplicates
    assert_eq!(state2.pending.len(), 0); // No pending migrations
}

#[test]
fn test_get_applied_migrations() {
    let (db, _tmp) = create_test_db();
    let (_migrations_tmp, migrations_dir) = create_test_migrations_dir();

    let config = MigrationConfig {
        migrations_dir: migrations_dir.clone(),
        mass_migration_threshold: 50,
        enable_backup: false,
        backup_dir: None,
    };

    // Run migrations
    db::run_migrations(&db, &config).unwrap();

    // Get applied migrations
    let applied = db::get_applied_migrations(&db).unwrap();
    assert_eq!(applied.len(), 3);
    assert_eq!(applied[0].version, "001");
    assert_eq!(applied[1].version, "002");
    assert_eq!(applied[2].version, "003");
}

#[test]
fn test_ensure_001_auto_seed_new_db() {
    let (db, _tmp) = create_test_db();

    // On a brand new empty database, ensure_001_auto_seed should not fail
    db::ensure_001_auto_seed(&db).unwrap();

    // The migration table should exist
    let applied = db::get_applied_migrations(&db).unwrap();
    // Since no tables exist yet, 001 should not be auto-inserted
    // (it only auto-inserts if tables already exist)
    assert_eq!(applied.len(), 0);
}

#[test]
fn test_ensure_001_auto_seed_existing_tables() {
    let (db, _tmp) = create_test_db();

    // Create a table directly (simulating legacy DB without migration record)
    db.with_conn(|conn| {
        conn.execute_batch("CREATE TABLE legacy_table (id INTEGER);").unwrap();
        Ok(())
    }).unwrap();

    // Now ensure_001_auto_seed should detect existing tables and insert 001
    db::ensure_001_auto_seed(&db).unwrap();

    let applied = db::get_applied_migrations(&db).unwrap();
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].version, "001");
    assert_eq!(applied[0].name, "001_initial_schema");
}

#[test]
fn test_mass_migration_abort() {
    let (db, _tmp) = create_test_db();
    let (_migrations_tmp, migrations_dir) = create_test_migrations_dir();

    // Set threshold very low to trigger abort
    let config = MigrationConfig {
        migrations_dir: migrations_dir.clone(),
        mass_migration_threshold: 2, // Only allow 2, but we have 3
        enable_backup: false,
        backup_dir: None,
    };

    let state = db::run_migrations(&db, &config).unwrap();

    // Should abort due to mass migration detection
    assert!(state.mass_abort);
    assert_eq!(state.pending.len(), 3); // All 3 are pending
}

#[test]
fn test_run_migration_pipeline_full() {
    let (db, _tmp) = create_test_db();
    let (_migrations_tmp, migrations_dir) = create_test_migrations_dir();

    // Use the full pipeline entry point
    let state = db::run_migration_pipeline(&db, Some(migrations_dir)).unwrap();

    assert!(!state.mass_abort);
    assert_eq!(state.applied.len(), 3);
    assert_eq!(state.pending.len(), 3);
}

#[test]
fn test_column_healers() {
    let (db, _tmp) = create_test_db();

    // Should not fail even on empty database
    db::column_healers::ensure_critical_columns(&db).unwrap();
}

#[test]
fn test_migration_state_serialization() {
    let (db, _tmp) = create_test_db();
    let (_migrations_tmp, migrations_dir) = create_test_migrations_dir();

    let config = MigrationConfig {
        migrations_dir: migrations_dir.clone(),
        mass_migration_threshold: 50,
        enable_backup: false,
        backup_dir: None,
    };

    let state = db::run_migrations(&db, &config).unwrap();

    // Test that MigrationState can be serialized (for debugging/logging)
    let json = serde_json::to_string(&state).unwrap();
    assert!(json.contains("001"));
    assert!(json.contains("002"));
    assert!(json.contains("003"));
}