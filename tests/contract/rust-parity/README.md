# rust-parity — Contract-Test Harness

Golden-fixture parity harness for the Rust backend migration
(plan: `_tasks/superpowers/plans/2026-10-17-rust-backend-migration.md`, ADR-011).

Fixtures are **recorded from the live TS server** (including full SSE chunk
sequences) and **replayed against the Rust server**. Volatile fields
(timestamps, request ids, rate-limit headers) are normalized; secrets are
**redacted at record time and never written to disk** (Hard Rule #1).

## Layout

```
tests/contract/rust-parity/
├── manifest.mjs        # v0 recording manifest (what to record)
├── recorder.mjs        # record fixtures from a --base server
├── replay.mjs          # replay fixtures against a --base server (or --spawn-rust)
├── lib/
│   ├── fixtures.mjs    # fixture persistence (fixtures/<name>.json)
│   ├── mock-upstream.mjs # canned OpenAI-compatible upstream (SSE + JSON)
│   └── normalize.mjs   # volatile-field normalization + secret redaction
└── fixtures/           # committed golden fixtures
```

## Usage

```bash
# 1. mock upstream (deterministic chat responses — no real provider keys)
node tests/contract/rust-parity/lib/mock-upstream.mjs   # port 20190

# 2. sandbox TS server (throwaway DATA_DIR — NEVER the operator's real one)
DATA_DIR=$(mktemp -d) PORT=20191 REQUIRE_API_KEY=false \
  JWT_SECRET=$(openssl rand -base64 48) API_KEY_SECRET=$(openssl rand -hex 32) \
  INITIAL_PASSWORD=fixture-admin npm run dev

# 3. record
CONTRACT_ADMIN_PASSWORD=fixture-admin \
  node tests/contract/rust-parity/recorder.mjs --base http://localhost:20191

# 4. round-trip check (must be 100% PASS against the same TS server)
CONTRACT_ADMIN_PASSWORD=fixture-admin \
  node tests/contract/rust-parity/replay.mjs --base http://localhost:20191

# 5. against the Rust server (healthz parity lands in Phase 1)
node tests/contract/rust-parity/replay.mjs --spawn-rust
```

npm shortcuts: `npm run contract:record`, `npm run contract:replay`, `npm run test:rust`.

## Chat fixtures need a seeded provider

`chat-nonstream` / `chat-stream` route through a provider connection whose
base URL points at the mock upstream. Seed the sandbox with a connection for a
custom-baseUrl provider (id from `CONTRACT_SEED_PROVIDER`, default `custom`)
whose model list includes `mock-model`, then export `CONTRACT_SEEDED=1`.

## Phase-0 expectation

- Replay against the **TS server that recorded the fixtures**: **all PASS**
  (proves normalization determinism — this is the Phase 0 exit criterion).
- Replay against the **Rust stub**: `/healthz` parity is expected to FAIL
  (stub payload) — Phase 1 makes it green fixture by fixture.
