// Item 5 (real token usage in _meta gui_pi/usage) and item 7 (Stop: gate_reset, no action after Stop).
import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:net";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { load, txt } from "./harness.ts";
import { UsageBook } from "../src/usage.ts";

test("usage: the assistant message's real usage goes with its first tool call, zeros with the others", async () => {
  const h = await load({ tools: ["click", "type_text"] });
  await h.pi.emit("message_end", { message: { role: "assistant", usage: { input: 1200, output: 85, cacheRead: 900, cacheWrite: 0 },
    content: [{ type: "text", text: "two steps" }, { type: "toolCall", id: "c1", name: "click", arguments: {} }, { type: "toolCall", id: "c2", name: "type_text", arguments: {} }] } });
  await h.pi.call("click", { x: 1, y: 2 }, "c1");
  await h.pi.call("type_text", { text: "a" }, "c2");
  await h.pi.call("click", { x: 3, y: 4 }, "c3"); // no usage known for it
  const calls = h.records().filter((r) => r.method === "tools/call");
  const meta = (i: number) => calls[i].params._meta;
  assert.deepEqual(meta(0)["gui_pi/usage"], { input: 1200, output: 85, cache_read: 900 });
  assert.deepEqual(meta(1)["gui_pi/usage"], { input: 0, output: 0, cache_read: 0, same_message: true });
  assert.equal(meta(2)["gui_pi/usage"], undefined);
  // usage is in _meta only, never in the model's arguments; the protocol keys are still there
  assert.deepEqual(calls[0].params.arguments, { x: 1, y: 2 });
  assert.equal(meta(0)["io.modelcontextprotocol/protocolVersion"], "2026-07-28");
  // user / tool messages are ignored
  const b = new UsageBook();
  b.onAssistant({ role: "user", usage: { input: 5 }, content: [{ type: "toolCall", id: "x" }] });
  assert.equal(b.metaFor("x"), undefined);
  h.api.mcp.close();
});

/** The shell's side of the stop channel: answers gate_env with {}, keeps watch_stop open, sends stop on demand. */
function fakeShell() {
  const dir = mkdtempSync(join(tmpdir(), "stop-"));
  const path = join(dir, "h.sock");
  let watcher: any = null;
  const acks: any[] = [];
  const socks: any[] = [];
  const srv = createServer((s) => {
    socks.push(s);
    let buf = "";
    s.on("data", (d) => {
      buf += String(d);
      let nl;
      while ((nl = buf.indexOf("\n")) >= 0) {
        const m = JSON.parse(buf.slice(0, nl));
        buf = buf.slice(nl + 1);
        if (m.op === "gate_env") s.end(JSON.stringify({ env: {} }) + "\n");
        else if (m.op === "watch_stop" && m.token === "tok") watcher = s;
        else if (m.ack) acks.push(m);
      }
    });
  });
  srv.listen(path);
  return {
    path, acks, srv,
    close() {
      for (const s of socks) s.destroy();
      srv.close();
    },
    async stop() {
      for (let i = 0; i < 50 && !watcher; i++) await new Promise((r) => setTimeout(r, 20));
      watcher.write(JSON.stringify({ stop: true }) + "\n");
      for (let i = 0; i < 100 && !acks.length; i++) await new Promise((r) => setTimeout(r, 20));
    },
  };
}

for (const withReset of [true, false]) {
  test(`stop (${withReset ? "gate with gate_reset" : "current gate, no gate_reset"}): reset if available, gate killed, no action after Stop`, async () => {
    const shell = fakeShell();
    const h = await load({ tools: ["get_window_state", "click", ...(withReset ? ["gate_reset"] : [])],
      results: withReset ? { gate_reset: { content: [{ type: "text", text: "reset" }], structuredContent: { reset: true } } } : {} },
      { GUI_PI_HUMAN_IPC: shell.path, GUI_PI_HUMAN_TOKEN: "tok" });
    await h.pi.call("click", { x: 1, y: 1 });
    await shell.stop();
    assert.equal(shell.acks.length, 1);
    assert.equal(shell.acks[0].reset, withReset);
    const before = h.calls().length;
    const names = h.calls().map((c: any) => c.name);
    assert.equal(names.at(-1), "gate_reset", "gate_reset is tried last (listed, or once for a gate that may have it)");
    assert.equal(h.calls().at(-1)._meta?.["io.github.cszf1/gui-gate/client"], true, "B9: gate_reset carries the client marker");
    // after Stop: the model's calls are held, a direct execute does not reach the gate either
    const blocked = await h.pi.call("click", { x: 2, y: 2 });
    assert.equal(blocked.blocked, true);
    assert.match(blocked.reason, /Stopped by the user/);
    const direct = await h.pi.tools.get("click").execute("z", { x: 3, y: 3 });
    assert.match(txt(direct), /pressed Stop; nothing was executed/);
    assert.equal(await h.pi.emit("turn_end", { toolResults: [{}], entries: [] }), undefined);
    await new Promise((r) => setTimeout(r, 200));
    assert.equal(h.calls().length, before, "nothing reached the gate after Stop");
    assert.ok(h.api.mcp.child.killed || h.api.mcp.child.exitCode !== null || h.api.mcp.child.signalCode !== null);
    shell.close();
  });
}
