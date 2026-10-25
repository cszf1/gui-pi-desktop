import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:net";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { askIpc, sanitize } from "../src/human.ts";

const P = (kind: string, timeout = 5) => ({ message: "m", requestedSchema: {}, _meta: { "io.github.cszf1/gui-gate": { kind, timeout_s: timeout } } });

test("sanitize: only well-formed accepts pass", () => {
  assert.deepEqual(sanitize("approve", { action: "accept", content: { decision: "allow" } }), { action: "accept", content: { decision: "allow" } });
  assert.equal(sanitize("approve", { action: "accept", content: { decision: "yes" } }).action, "decline");
  assert.equal(sanitize("takeover", { action: "accept", content: { done: "true" } }).action, "decline");
  assert.equal(sanitize("answer", { action: "accept", content: { answer: "  " } }).action, "decline");
  assert.equal(sanitize("ask_help", { action: "accept", content: { choice: "did_it" } }).content!.choice, "did_it");
  assert.equal(sanitize("approve", null).action, "decline");
});

function shell(reply: (req: any) => any) {
  const path = join(mkdtempSync(join(tmpdir(), "ipc-")), "h.sock");
  const seen: any[] = [];
  const srv = createServer((sock) => {
    let buf = "";
    sock.on("data", (d) => {
      buf += d;
      const i = buf.indexOf("\n");
      if (i < 0) return;
      const req = JSON.parse(buf.slice(0, i));
      seen.push(req);
      const r = reply(req);
      if (r) sock.write(JSON.stringify({ id: req.id, ...r }) + "\n");
    });
  });
  return new Promise<{ path: string; seen: any[]; close: () => void }>((res) => srv.listen(path, () => res({ path, seen, close: () => srv.close() })));
}

test("ipc round trip carries the token and the kind", async () => {
  const s = await shell((req) => (req.token === "tok" ? { action: "accept", content: { done: true } } : { action: "decline" }));
  const r = await askIpc(s.path, "tok", P("takeover") as any);
  assert.deepEqual(r, { action: "accept", content: { done: true } });
  assert.equal(s.seen[0].kind, "takeover");
  s.close();
});

test("no answer before the deadline is a decline", async () => {
  const s = await shell(() => null);
  const t0 = Date.now();
  const r = await askIpc(s.path, "tok", P("approve", 1) as any);
  assert.equal(r.action, "decline");
  assert.ok(Date.now() - t0 < 6000);
  s.close();
});

test("a dead socket is a decline", async () => {
  const r = await askIpc("/nonexistent/sock", "t", P("approve") as any);
  assert.equal(r.action, "decline");
});

const reason = (r: any) => r?._meta?.["io.github.cszf1/gui-gate"]?.reason;

test("refusal reasons travel to the gate: timeout, unavailable, window closed", async () => {
  const s = await shell(() => null);
  assert.equal(reason(await askIpc(s.path, "tok", P("approve", 1) as any)), "timed_out");
  s.close();
  assert.equal(reason(await askIpc("/nonexistent/sock", "t", P("approve") as any)), "unavailable");
  const c = await shell(() => ({ action: "cancel", reason: "closed" }));
  const r = await askIpc(c.path, "tok", P("approve") as any);
  assert.equal(r.action, "cancel");
  assert.equal(reason(r), "closed");
  c.close();
  assert.equal(reason(sanitize("approve", { action: "decline", reason: "made-up" })), undefined);
  // B8: a question the shell never showed (it expired in the queue) says so; the gate records it as no answer
  assert.equal(reason(sanitize("help" as any, { action: "decline", reason: "expired" })), "expired");
});

test("coordinates pass through unchanged (the client never rescales x/y)", async () => {
  const { cleanArgs } = await import("../src/mcp.ts");
  assert.deepEqual(cleanArgs({ x: 1234.5, y: 77, pid: 1, window_id: 2, gate_confirm: "c", inputResponses: {} }), { x: 1234.5, y: 77, pid: 1, window_id: 2 });
});

test("the four strict result types: clarify / help are the answer / ask_help forms", async () => {
  const { kindOf, sanitize } = await import("../src/human.ts");
  const p = (kind: string) => ({ message: "m", requestedSchema: {}, _meta: { "io.github.cszf1/gui-gate": { kind } } });
  assert.equal(kindOf(p("clarify")), "answer");
  assert.equal(kindOf(p("help")), "ask_help");
  assert.equal(kindOf(p("rm -rf")), "approve");
  // a clarify answer cannot pass as an approval, a takeover "done" carries nothing else
  assert.deepEqual(sanitize("answer", { action: "accept", content: { answer: "q3.ods", decision: "allow" } }), { action: "accept", content: { answer: "q3.ods" } });
  assert.deepEqual(sanitize("takeover", { action: "accept", content: { done: true, decision: "allow" } }), { action: "accept", content: { done: true } });
  assert.equal(sanitize("approve", { action: "accept", content: { answer: "yes" } }).action, "decline");
  assert.deepEqual(sanitize("ask_help", { action: "accept", content: { choice: "try_other", note: "use the File menu" } }).content,
                   { choice: "try_other", note: "use the File menu" });
});
