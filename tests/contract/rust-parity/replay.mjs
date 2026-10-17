// Replays recorded golden fixtures against a server and reports parity.
//
//   node tests/contract/rust-parity/replay.mjs --base http://localhost:20191
//     [--fixtures tests/contract/rust-parity] [--only name1,name2]
//   node tests/contract/rust-parity/replay.mjs --spawn-rust [--port 20192]
//
// Phase-0 expectation: replay against the SAME TS server that recorded the
// fixtures must be 100% PASS (proves normalization determinism). Against the
// Rust stub, /healthz parity is NOT expected yet — Phase 1 turns it green.

import { spawn } from "node:child_process";
import { mkdtemp } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { ADMIN_PASSWORD_ENV } from "./manifest.mjs";
import { listFixtures, loadFixture } from "./lib/fixtures.mjs";
import { normalizeSse, normalizeValue } from "./lib/normalize.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
// tests/contract/rust-parity → three levels up to the repo root.
const REPO_ROOT = path.resolve(HERE, "..", "..", "..");
const RUST_ROOT = path.join(REPO_ROOT, "rust");

function parseArgs(argv) {
  const args = { base: null, fixtures: HERE, only: null, spawnRust: false, port: 20192 };
  for (let i = 2; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--base") args.base = argv[++i];
    else if (arg === "--fixtures") args.fixtures = argv[++i];
    else if (arg === "--only") args.only = argv[++i]?.split(",");
    else if (arg === "--spawn-rust") args.spawnRust = true;
    else if (arg === "--port") args.port = Number(argv[++i]);
    else throw new Error(`unknown argument: ${arg}`);
  }
  if (!args.spawnRust && !args.base) {
    throw new Error("--base <url> or --spawn-rust is required");
  }
  return args;
}

async function waitForHealth(base, timeoutMs = 20000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const res = await fetch(new URL("/healthz", base));
      if (res.ok) return;
    } catch {
      // not up yet
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`server at ${base} never became healthy`);
}

async function login(base) {
  const password = process.env[ADMIN_PASSWORD_ENV];
  if (!password) {
    throw new Error(`${ADMIN_PASSWORD_ENV} must be set to replay management-auth fixtures`);
  }
  const res = await fetch(new URL("/api/auth/login", base), {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ password }),
  });
  if (!res.ok) throw new Error(`login failed: HTTP ${res.status}`);
  const body = await res.json();
  const token = body?.token ?? body?.sessionToken ?? body?.data?.token;
  if (!token) throw new Error("login returned no token");
  return token;
}

function summarize(value) {
  return JSON.stringify(value).slice(0, 200);
}

async function replayOne(base, fixture, token) {
  const { request: requestSpec, response: want } = fixture;
  const headers = { ...requestSpec.headers };
  if (fixture.meta.needsManagementAuth) {
    if (!token) throw new Error("fixture needs management auth but no token available");
    headers.authorization = `Bearer ${token}`;
  }

  const res = await fetch(new URL(requestSpec.path, base), {
    method: requestSpec.method,
    headers,
    body:
      requestSpec.body === null || requestSpec.body === undefined
        ? undefined
        : JSON.stringify(requestSpec.body),
  });

  const problems = [];
  if (res.status !== want.status) {
    problems.push(`status ${res.status} ≠ ${want.status}`);
  }

  const contentType = res.headers.get("content-type") || "";
  const wantContentType = want.headers["content-type"] || "";
  if (contentType.split(";")[0].trim() !== wantContentType.split(";")[0].trim()) {
    problems.push(`content-type ${contentType || "(none)"} ≠ ${wantContentType || "(none)"}`);
  }

  if (contentType.includes("text/event-stream")) {
    const raw = await res.text();
    const actual = normalizeSse(raw);
    const expected = want.sse ?? [];
    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
      problems.push(`sse sequence differs (${actual.length} vs ${expected.length} events)`);
      const max = Math.max(actual.length, expected.length);
      for (let i = 0; i < max; i += 1) {
        if (JSON.stringify(actual[i]) !== JSON.stringify(expected[i])) {
          problems.push(
            `  event[${i}]: expected ${summarize(expected[i])} / got ${summarize(actual[i])}`
          );
          break;
        }
      }
    }
  } else {
    const text = await res.text();
    let parsed = text;
    try {
      parsed = JSON.parse(text);
    } catch {
      // keep raw text
    }
    const actual = typeof parsed === "string" ? parsed : normalizeValue(parsed);
    if (JSON.stringify(actual) !== JSON.stringify(want.body)) {
      problems.push(`body differs: expected ${summarize(want.body)} / got ${summarize(actual)}`);
    }
  }
  return problems;
}

async function main() {
  const args = parseArgs(process.argv);

  let base = args.base;
  let child = null;
  if (args.spawnRust) {
    await new Promise((resolve, reject) => {
      const build = spawn("cargo", ["build", "-p", "server"], {
        cwd: RUST_ROOT,
        stdio: "inherit",
      });
      build.on("error", reject);
      build.on("exit", (code) =>
        code === 0 ? resolve() : reject(new Error("cargo build failed"))
      );
    });
    const dataDir = await mkdtemp(path.join(os.tmpdir(), "omniroute-rust-replay-"));
    base = `http://127.0.0.1:${args.port}`;
    child = spawn(path.join(RUST_ROOT, "target", "debug", "server"), [], {
      env: { ...process.env, PORT: String(args.port), DATA_DIR: dataDir },
      stdio: ["ignore", "ignore", "ignore"],
    });
    await waitForHealth(base);
  }

  try {
    const names = args.only ? args.only : await listFixtures(args.fixtures);
    let token = null;
    const needsAuth = [];
    for (const name of names) {
      const fixture = await loadFixture(args.fixtures, name);
      if (fixture.meta.needsManagementAuth) needsAuth.push(name);
    }
    if (needsAuth.length > 0) {
      token = await login(base);
    }

    let passed = 0;
    let failed = 0;
    for (const name of names) {
      const fixture = await loadFixture(args.fixtures, name);
      try {
        const problems = await replayOne(base, fixture, token);
        if (problems.length === 0) {
          passed += 1;
          console.log(`PASS ${name}`);
        } else {
          failed += 1;
          console.log(`FAIL ${name}`);
          for (const problem of problems) console.log(`  ${problem}`);
        }
      } catch (err) {
        failed += 1;
        console.log(`FAIL ${name}`);
        console.log(`  ${err.message}`);
      }
    }
    console.log(`\n${passed} passed, ${failed} failed (base ${base})`);
    if (failed > 0) process.exitCode = 1;
  } finally {
    if (child) {
      child.kill("SIGTERM");
      await new Promise((resolve) => {
        child.on("exit", resolve);
      });
    }
  }
}

main().catch((err) => {
  console.error(err.message);
  process.exitCode = 1;
});
