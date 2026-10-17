//! services: memory, skills sandbox, embedded services, webhooks, log export.
//!
//! Scaffolded in Phase 0 — implementation lands in Phase 11 per the
//! migration plan (`_tasks/superpowers/plans/2026-10-17-rust-backend-migration.md`).

/// Marker test: keeps `cargo test --workspace` exercising this crate from day one.
#[test]
fn crate_scaffolded() {
    assert_eq!(env!("CARGO_PKG_NAME"), "services");
}
