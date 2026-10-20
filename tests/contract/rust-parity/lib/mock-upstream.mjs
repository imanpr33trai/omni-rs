// Canned OpenAI-compatible upstream for deterministic chat fixtures.
// The sandbox TS server routes to this mock, so recorded SSE sequences are
// fully reproducible and need zero real provider keys.
//
//   node tests/contract/rust-parity/lib/mock-upstream.mjs   # port 20190 default

import http from "node:http";
import process from "node:process";

const PORT = Number(process.env.MOCK_UPSTREAM_PORT || 20190);
const COMPLETION_TEXT = "Hello from mock";
const FIXED_EPOCH = 1700000000; // deterministic: no wall-clock in fixtures
const MOCK_ID = "chatcmpl-mock-0001";
const USAGE = { prompt_tokens: 10, completion_tokens: 6, total_tokens: 16 };

function json(res, status, body) {
  const payload = JSON.stringify(body);
  res.writeHead(status, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(payload),
  });
  res.end(payload);
}

function sseWrite(res, obj) {
  res.write(`data: ${JSON.stringify(obj)}\n\n`);
}

const server = http.createServer((req, res) => {
  const chunks = [];
  req.on("data", (chunk) => chunks.push(chunk));
  req.on("end", () => {
    const raw = Buffer.concat(chunks).toString("utf8");

    if (req.method === "GET" && req.url === "/v1/models") {
      json(res, 200, {
        object: "list",
        data: [{ id: "mock-model", object: "model", owned_by: "mock" }],
      });
      return;
    }

    if (req.method === "POST" && req.url === "/v1/chat/completions") {
      let body = {};
      try {
        body = JSON.parse(raw);
      } catch {
        json(res, 400, { error: { message: "mock upstream: invalid JSON" } });
        return;
      }
      const model = typeof body.model === "string" ? body.model : "mock-model";

      if (body.stream === true) {
        res.writeHead(200, {
          "content-type": "text/event-stream",
          "cache-control": "no-cache",
          connection: "keep-alive",
        });
        const base = { id: MOCK_ID, object: "chat.completion.chunk", model, created: FIXED_EPOCH };
        sseWrite(res, {
          ...base,
          choices: [{ index: 0, delta: { role: "assistant", content: "" }, finish_reason: null }],
        });
        for (const part of COMPLETION_TEXT.split(" ")) {
          sseWrite(res, {
            ...base,
            choices: [{ index: 0, delta: { content: `${part} ` }, finish_reason: null }],
          });
        }
        sseWrite(res, { ...base, choices: [{ index: 0, delta: {}, finish_reason: "stop" }] });
        if (body.stream_options?.include_usage === true) {
          sseWrite(res, { ...base, choices: [], usage: USAGE });
        }
        res.write("data: [DONE]\n\n");
        res.end();
        return;
      }

      json(res, 200, {
        id: MOCK_ID,
        object: "chat.completion",
        created: FIXED_EPOCH,
        model,
        choices: [
          {
            index: 0,
            message: { role: "assistant", content: COMPLETION_TEXT },
            finish_reason: "stop",
          },
        ],
        usage: USAGE,
      });
      return;
    }

    json(res, 404, { error: { message: `mock upstream: no route ${req.method} ${req.url}` } });
  });
});

server.listen(PORT, "127.0.0.1", () => {
  console.log(`mock upstream listening on http://127.0.0.1:${PORT}`);
});
