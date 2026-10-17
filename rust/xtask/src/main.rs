//! Workspace automation tasks.
//!
//! Phase 0 placeholder — release packaging (single-binary build, migration
//! bundling, sqlite-vec shipping, npm pack layout) arrives in Phase 12.

use std::env;

fn main() {
    match env::args().nth(1).as_deref() {
        Some(other) => println!("unknown task: {other} (xtask is a placeholder in Phase 0)"),
        None => println!("no tasks yet — release packaging arrives in Phase 12"),
    }
}
