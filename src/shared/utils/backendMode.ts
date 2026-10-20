/**
 * Backend mode switch — the strangler-fig control for the Rust migration.
 *
 * `OMNIROUTE_BACKEND_MODE` selects which server serves a request family:
 * - `ts`     (default): the existing Next.js/Node backend handles everything.
 * - `rust`:  the Rust server (`rust/crates/server`) handles the families it
 *   has reached parity on (contract fixtures in `tests/contract/rust-parity/`).
 * - `hybrid`: per-family override map routes migrated families to Rust and
 *   the rest to TS.
 *
 * Phase 0 ships this as a pure pass-through: it parses and validates the env
 * var but NOTHING consumes it yet, so behavior is unchanged. The routing hook
 * lands in Phase 1 with the axum skeleton
 * (`_tasks/superpowers/plans/2026-10-17-rust-backend-migration.md` §5.1).
 */

export const BACKEND_MODE_ENV = "OMNIROUTE_BACKEND_MODE";

export type BackendMode = "ts" | "rust" | "hybrid";

export const BACKEND_MODES: readonly BackendMode[] = ["ts", "rust", "hybrid"] as const;

export const DEFAULT_BACKEND_MODE: BackendMode = "ts";

/**
 * Parses a raw env value into a `BackendMode`. Invalid or missing values fall
 * back to `"ts"` silently — mirroring the lenient env posture used by the
 * other tunables (see `open-sse/config/constants.ts` `envInt()`).
 */
export function parseBackendMode(raw: string | undefined | null): BackendMode {
  if (!raw) return DEFAULT_BACKEND_MODE;
  const value = raw.trim().toLowerCase();
  return (BACKEND_MODES as readonly string[]).includes(value)
    ? (value as BackendMode)
    : DEFAULT_BACKEND_MODE;
}

/** Reads the current mode from the environment (read-only; no caching). */
export function getBackendMode(): BackendMode {
  return parseBackendMode(process.env[BACKEND_MODE_ENV]);
}

/**
 * True when the mode is `rust` or `hybrid` — the signal the Phase 1 routing
 * hook will check before forwarding a request family to the Rust server.
 */
export function isRustBackendActive(): boolean {
  const mode = getBackendMode();
  return mode === "rust" || mode === "hybrid";
}
