//! auth: dashboard JWT, API keys, authz pipeline, routeGuard tiers.
//!
//! Scaffolded in Phase 0 — implementation lands in Phase 1 per the
//! migration plan (`_tasks/superpowers/plans/2026-10-17-rust-backend-migration.md`).

/// Marker test: keeps `cargo test --workspace` exercising this crate from day one.
#[test]
fn crate_scaffolded() {
    assert_eq!(env!("CARGO_PKG_NAME"), "auth");
}
