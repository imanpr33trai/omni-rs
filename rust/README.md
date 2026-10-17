# OmniRoute — Rust Backend

Rust reimplementation of the OmniRoute backend (everything except the Next.js
frontend). Phase 0 scaffold.

- **Migration plan**: `_tasks/superpowers/plans/2026-10-17-rust-backend-migration.md`
- **Decision records**: `_tasks/superpowers/specs/2026-10-17-rust-migration-adrs.md`
- **Contract harness**: `tests/contract/rust-parity/` (golden fixtures recorded
  from the TS server, replayed against this server)

## Workspace layout

| Crate        | Purpose                                                 | Phase |
| ------------ | ------------------------------------------------------- | ----- |
| `server`     | axum binary: router, `/healthz` stub, graceful shutdown | 0     |
| `cli`        | `omniroute` CLI commands                                | 12    |
| `config`     | env layering, feature flags, runtime settings           | 1     |
| `error`      | error-body construction + sanitization                  | 1     |
| `auth`       | JWT, API keys, authz pipeline, routeGuard tiers         | 1     |
| `db`         | rusqlite, migration runner, domain modules, encryption  | 2     |
| `providers`  | provider registry, executors                            | 3     |
| `oauth`      | OAuth flows (PKCE, device, refresh)                     | 3     |
| `translate`  | 8-format translators, SSE parsers                       | 4     |
| `chat`       | chatCore pipeline, streaming, caches                    | 5     |
| `resilience` | breaker, cooldowns, lockout, rate limits                | 6     |
| `routing`    | combo strategies, fusion, autoCombo                     | 7     |
| `policy`     | policy engine, cost rules, budgets                      | 8     |
| `protocols`  | MCP (rmcp), A2A, ACP                                    | 10    |
| `guardrails` | PII (opt-in), injection, vision guards                  | 11    |
| `services`   | memory, skills, embedded services, webhooks             | 11    |
| `edge`       | tunnels, MITM, TLS                                      | 12    |
| `xtask`      | release/packaging automation                            | 12    |

## Commands

```bash
cargo build                 # build all crates
cargo test                  # run tests
cargo fmt --all             # format
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p server         # run the server stub (PORT env, default 20128)
```

## Conventions

- Crate names are simple and unprefixed (ADR-006) — private workspace members,
  never published. The released binary artifact name is decided in Phase 12.
- `publish = false` everywhere; add new deps through `[workspace.dependencies]`.
- CI: `.github/workflows/rust-ci.yml` (fmt + clippy `-D warnings` + test; cargo-deny
  licenses strict, advisories advisory during phase 0).
