// v0 golden-fixture recording manifest (Phase 0.4).
//
// - needsManagementAuth: the recorder logs into the sandbox with
//   CONTRACT_ADMIN_PASSWORD and attaches a bearer token (never persisted).
// - needsSeed: requires the sandbox provider connection created by the seed
//   step (see README) pointing at the mock upstream; export CONTRACT_SEEDED=1
//   once seeded, otherwise these entries are skipped.

export const RECORDINGS = [
  { name: "healthz", method: "GET", path: "/healthz" },
  { name: "auth-status", method: "GET", path: "/api/auth/status" },
  { name: "models", method: "GET", path: "/api/v1/models" },
  {
    name: "chat-nonstream",
    method: "POST",
    path: "/api/v1/chat/completions",
    body: {
      model: "mock-model",
      messages: [{ role: "user", content: "Say hello" }],
      stream: false,
    },
    needsSeed: true,
  },
  {
    name: "chat-stream",
    method: "POST",
    path: "/api/v1/chat/completions",
    body: {
      model: "mock-model",
      messages: [{ role: "user", content: "Say hello" }],
      stream: true,
      stream_options: { include_usage: true },
    },
    sse: true,
    needsSeed: true,
  },
  {
    name: "settings-cache-config-get",
    method: "GET",
    path: "/api/settings/cache-config",
    needsManagementAuth: true,
  },
  {
    name: "settings-cache-config-patch",
    method: "PATCH",
    path: "/api/settings/cache-config",
    body: {},
    needsManagementAuth: true,
  },
];

export const ADMIN_PASSWORD_ENV = "CONTRACT_ADMIN_PASSWORD";
export const SEED_PROVIDER_ENV = "CONTRACT_SEED_PROVIDER";
