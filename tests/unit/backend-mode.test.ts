/**
 * Backend-mode flag tests (Phase 0.5 — pass-through, zero behavior change).
 *
 * Guards:
 * - default is `"ts"` when the env var is unset/empty/whitespace
 * - valid values parse case-insensitively (env vars are conventionally upper)
 * - invalid values fall back to `"ts"` silently (lenient env posture)
 * - `isRustBackendActive()` is false unless mode is `rust`/`hybrid`
 */

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  BACKEND_MODE_ENV,
  DEFAULT_BACKEND_MODE,
  getBackendMode,
  isRustBackendActive,
  parseBackendMode,
} from "@/shared/utils/backendMode";

const ORIGINAL = process.env[BACKEND_MODE_ENV];

function withMode(value: string | undefined, fn: () => void): void {
  if (value === undefined) delete process.env[BACKEND_MODE_ENV];
  else process.env[BACKEND_MODE_ENV] = value;
  try {
    fn();
  } finally {
    if (ORIGINAL === undefined) delete process.env[BACKEND_MODE_ENV];
    else process.env[BACKEND_MODE_ENV] = ORIGINAL;
  }
}

test("default backend mode is ts", () => {
  withMode(undefined, () => {
    assert.equal(getBackendMode(), "ts");
    assert.equal(DEFAULT_BACKEND_MODE, "ts");
  });
});

test("empty or whitespace values fall back to ts", () => {
  for (const empty of ["", "   "]) {
    withMode(empty, () => assert.equal(parseBackendMode(empty), "ts"));
  }
});

test("valid modes parse and are case-insensitive", () => {
  withMode("rust", () => assert.equal(getBackendMode(), "rust"));
  withMode("RUST", () => assert.equal(getBackendMode(), "rust"));
  withMode("Hybrid", () => assert.equal(getBackendMode(), "hybrid"));
  withMode(" TS ", () => assert.equal(getBackendMode(), "ts"));
});

test("invalid values fall back to ts silently", () => {
  for (const bad of ["node", "golang", "rusty", "0", "true"]) {
    withMode(bad, () => assert.equal(parseBackendMode(bad), "ts"));
  }
});

test("null and undefined parse to ts", () => {
  assert.equal(parseBackendMode(undefined), "ts");
  assert.equal(parseBackendMode(null), "ts");
});

test("isRustBackendActive only for rust or hybrid", () => {
  withMode("ts", () => assert.equal(isRustBackendActive(), false));
  withMode("rust", () => assert.equal(isRustBackendActive(), true));
  withMode("hybrid", () => assert.equal(isRustBackendActive(), true));
  withMode("bogus", () => assert.equal(isRustBackendActive(), false));
});
