// Records golden fixtures from a live server (normally the TS sandbox).
//
//   node tests/contract/rust-parity/recorder.mjs --base http://localhost:20191
//     [--out tests/contract/rust-parity] [--only name1,name2]
//
// Secrets are redacted before anything is written to disk (lib/normalize.mjs).
// Management-auth entries need CONTRACT_ADMIN_PASSWORD matching the sandbox's
// INITIAL_PASSWORD.

import process from "node:process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { ADMIN_PASSWORD_ENV, RECORDINGS } from "./manifest.mjs";
import {
  normalizeResponseHeaders,
  normalizeSse,
  normalizeValue,
  redactRequestBody,
  redactRequestHeaders,
} from "./lib/normalize.mjs";
import { saveFixture } from "./lib/fixtures.mjs";

function parseArgs(argv) {
  const here = path.dirname(fileURLToPath(import.meta.url));
  const args = { base: null, out: here, only: null };
  for (let i = 2; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--base") args.base = argv[i + 1];
    else if (arg === "--out") args.out = argv[i + 1];
    else if (arg === "--only") args.only = argv[i + 1]?.split(",");
    else throw new Error(`unknown argument: ${arg}`);
  }
  if (!args.base) throw new Error("--base <url> is required (e.g. http://localhost:20191)");
  return args;
}

async function login(base) {
  const password = process.env[ADMIN_PASSWORD_ENV];
  if (!password) {
    throw new Error(`${ADMIN_PASSWORD_ENV} must be set for management-auth recordings`);
  }
  const res = await fetch(new URL("/api/auth/login", base), {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ password }),
  });
  if (!res.ok) throw new Error(`sandbox login failed: HTTP ${res.status}`);
  const body = await res.json();
  const token = body?.token ?? body?.sessionToken ?? body?.data?.token;
  if (!token) {
    throw new Error(
      `sandbox login returned no token (keys: ${JSON.stringify(Object.keys(body ?? {}))})`
    );
  }
  return token;
}

async function recordOne(base, spec, token) {
  const headers = { accept: spec.sse ? "text/event-stream" : "application/json" };
  if (spec.body !== undefined) headers["content-type"] = "application/json";
  if (spec.needsManagementAuth && token) headers.authorization = `Bearer ${token}`;

  const res = await fetch(new URL(spec.path, base), {
    method: spec.method,
    headers,
    body: spec.body === undefined ? undefined : JSON.stringify(spec.body),
  });

  const contentType = res.headers.get("content-type") || "";
  let responsePart;
  if (contentType.includes("text/event-stream")) {
    const raw = await res.text();
    responsePart = { sse: normalizeSse(raw) };
  } else {
    const text = await res.text();
    let parsed = text;
    try {
      parsed = JSON.parse(text);
    } catch {
      // keep raw text
    }
    responsePart = { body: typeof parsed === "string" ? parsed : normalizeValue(parsed) };
  }

  return {
    meta: {
      name: spec.name,
      method: spec.method,
      path: spec.path,
      needsManagementAuth: spec.needsManagementAuth === true,
      recordedAt: "<ts>",
      recordedFrom: new URL(base).host,
    },
    request: {
      method: spec.method,
      path: spec.path,
      headers: redactRequestHeaders(headers),
      body: redactRequestBody(spec.path, spec.body ?? null),
    },
    response: {
      status: res.status,
      headers: normalizeResponseHeaders(Object.fromEntries(res.headers.entries())),
      ...responsePart,
    },
  };
}

const args = parseArgs(process.argv);
const specs = args.only ? RECORDINGS.filter((spec) => args.only.includes(spec.name)) : RECORDINGS;

let token = null;
if (specs.some((spec) => spec.needsManagementAuth)) {
  token = await login(args.base);
}

let failures = 0;
for (const spec of specs) {
  if (spec.needsSeed && process.env.CONTRACT_SEEDED !== "1") {
    console.log(`SKIP ${spec.name} (needs seeded mock provider — see README)`);
    continue;
  }
  try {
    const fixture = await recordOne(args.base, spec, token);
    const file = await saveFixture(args.out, fixture);
    console.log(`RECORDED ${spec.name} → ${file} (status ${fixture.response.status})`);
  } catch (err) {
    failures += 1;
    console.error(`FAILED ${spec.name}: ${err.message}`);
  }
}
process.exitCode = failures > 0 ? 1 : 0;
