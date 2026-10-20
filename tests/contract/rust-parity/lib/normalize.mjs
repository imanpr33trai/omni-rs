// Volatile-field normalization + secret redaction for contract fixtures.
//
// Hard rule #1 (never commit secrets): anything secret-bearing in a request is
// redacted at record time and never touches disk. Volatile response fields
// (timestamps, request ids, rate-limit headers) fold to placeholders so the
// same fixture replays deterministically against TS and Rust servers.

const REDACTED_REQUEST_HEADERS = new Set([
  "authorization",
  "x-api-key",
  "cookie",
  "x-csrf-token",
  "proxy-authorization",
]);

// Response headers whose value changes per request or encodes session state.
const VOLATILE_RESPONSE_HEADERS = new Set([
  "age",
  "content-length",
  "date",
  "etag",
  "retry-after",
  "set-cookie",
  "vary",
  "x-correlation-id",
  "x-ratelimit-remaining-requests",
  "x-ratelimit-remaining-tokens",
  "x-ratelimit-reset-requests",
  "x-ratelimit-reset-tokens",
  "x-request-id",
]);

// Request bodies for these paths are replaced entirely — they carry credentials.
const REDACTED_REQUEST_PATH_PREFIXES = ["/api/auth/", "/api/oauth/"];

const ISO_DATE_RE = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$/;
const EPOCH_MS_RE = /^\d{13}$/;
const EPOCH_S_RE = /^\d{10}$/;
const VOLATILE_ID_PREFIXES = ["chatcmpl-", "req_", "sess_"];
const VOLATILE_VALUE_KEY_RES = [/^created$/i, /_at$/i, /timestamp/i, /^(x-)?request-id$/i];

function isVolatileKey(key) {
  return VOLATILE_VALUE_KEY_RES.some((re) => re.test(key));
}

function normalizeScalar(key, value) {
  if (typeof value === "string") {
    if (
      (isVolatileKey(key) || ISO_DATE_RE.test(value)) &&
      (ISO_DATE_RE.test(value) || EPOCH_MS_RE.test(value) || EPOCH_S_RE.test(value))
    ) {
      return "<ts>";
    }
    if (VOLATILE_ID_PREFIXES.some((prefix) => value.startsWith(prefix))) {
      return "<id>";
    }
    return value;
  }
  if (typeof value === "number" && isVolatileKey(key)) {
    return "<ts>";
  }
  return value;
}

export function normalizeValue(value, key = "") {
  if (Array.isArray(value)) {
    return value.map((item) => normalizeValue(item, key));
  }
  if (value && typeof value === "object") {
    const out = {};
    for (const [entryKey, entryValue] of Object.entries(value)) {
      out[entryKey] = normalizeValue(entryValue, entryKey);
    }
    return out;
  }
  return normalizeScalar(key, value);
}

export function redactRequestHeaders(headers) {
  const out = {};
  for (const [rawKey, rawValue] of Object.entries(headers)) {
    const key = String(rawKey).toLowerCase();
    const value = Array.isArray(rawValue) ? rawValue.join(", ") : String(rawValue);
    out[key] = REDACTED_REQUEST_HEADERS.has(key) ? "<redacted>" : value;
  }
  delete out.host; // differs per server/port
  delete out["content-length"];
  return out;
}

export function normalizeResponseHeaders(headers) {
  const out = {};
  for (const [rawKey, rawValue] of Object.entries(headers)) {
    const key = String(rawKey).toLowerCase();
    const value = Array.isArray(rawValue) ? rawValue.join(", ") : String(rawValue);
    out[key] = VOLATILE_RESPONSE_HEADERS.has(key) ? "<normalized>" : value;
  }
  delete out.host;
  return out;
}

export function redactRequestBody(requestPath, body) {
  if (body == null) {
    return null;
  }
  if (REDACTED_REQUEST_PATH_PREFIXES.some((prefix) => requestPath.startsWith(prefix))) {
    return "<redacted>";
  }
  return body;
}

// Parses a raw SSE body into a comparable event sequence.
// - Comment-only blocks (keepalives, ":...") are dropped — timing artifacts.
// - Multi-line `data:` payloads join with "\n" per the SSE spec.
export function parseSse(raw) {
  const events = [];
  for (const block of raw.split(/\r?\n\r?\n/)) {
    const lines = block.split(/\r?\n/).filter((line) => line.length > 0);
    if (lines.length === 0 || lines.every((line) => line.startsWith(":"))) {
      continue;
    }
    let event = null;
    const dataLines = [];
    for (const line of lines) {
      if (line.startsWith(":")) continue;
      if (line.startsWith("event:")) event = line.slice("event:".length).trim();
      else if (line.startsWith("data:"))
        dataLines.push(line.slice("data:".length).replace(/^ /, ""));
    }
    if (dataLines.length === 0) continue;
    const data = dataLines.join("\n");
    let parsed = data;
    try {
      parsed = JSON.parse(data);
    } catch {
      // keep the raw string (e.g. "[DONE]")
    }
    events.push({ event, data: parsed });
  }
  return events;
}

export function normalizeSse(raw) {
  return parseSse(raw).map((entry) => ({
    event: entry.event,
    data: typeof entry.data === "string" ? entry.data : normalizeValue(entry.data),
  }));
}
