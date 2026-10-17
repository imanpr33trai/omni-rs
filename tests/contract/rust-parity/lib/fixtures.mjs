// Fixture persistence for the rust-parity contract harness.
// Fixtures are plain JSON committed under tests/contract/rust-parity/fixtures/.

import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

const FIXTURE_NAME_RE = /^[a-z0-9-]+$/;

export function fixturesDir(baseDir) {
  return path.resolve(baseDir, "fixtures");
}

export async function saveFixture(baseDir, fixture) {
  const name = fixture?.meta?.name;
  if (!FIXTURE_NAME_RE.test(name ?? "")) {
    throw new Error(`invalid fixture name: ${name}`);
  }
  const dir = fixturesDir(baseDir);
  await mkdir(dir, { recursive: true });
  const file = path.join(dir, `${name}.json`);
  await writeFile(file, `${JSON.stringify(fixture, null, 2)}\n`, "utf8");
  return file;
}

export async function listFixtures(baseDir) {
  const dir = fixturesDir(baseDir);
  let entries;
  try {
    entries = await readdir(dir);
  } catch {
    throw new Error(`fixtures directory not found: ${dir} — run the recorder first`);
  }
  return entries
    .filter((name) => name.endsWith(".json"))
    .map((name) => name.slice(0, -".json".length))
    .sort();
}

export async function loadFixture(baseDir, name) {
  const file = path.join(fixturesDir(baseDir), `${name}.json`);
  const raw = await readFile(file, "utf8");
  return JSON.parse(raw);
}
