// Item 11: fake keys end to end on the extension side. Keys from the shell's keychain-mock reach only the child
// that needs them: the gate's keys only the gate's environment (handed over once through the person IPC), Pi's
// model key never the gate; and no key reaches the model (tool output, details), the session or a log line.
import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:net";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { load } from "./harness.ts";

const MAIN = "sk-fake-pi-MAIN-0000111122";
const REV = "sk-fake-gate-REVIEWER-123456";
const PLAN = "sk-fake-gate-PLANNER-654321";
const TOKEN = "tok-secret-person-ipc-abcdef";

function shellIpc() {
  const path = join(mkdtempSync(join(tmpdir(), "leak-")), "h.sock");
  const socks: any[] = [];
  const srv = createServer((s) => {
    socks.push(s);
    s.on("data", (d) => {
      const m = JSON.parse(String(d).split("\n")[0]);
      if (m.op === "gate_env" && m.token === TOKEN)
        s.end(JSON.stringify({ env: { GUI_GATE_REVIEWER_KEY: REV, GUI_GATE_ROLE_PLANNER_KEY: PLAN, GUI_GATE_REVIEWER_URL: "https://r.example.com",
                                      GUI_PI_MAIN_KEY: "must-not-pass", GUI_GATE_ALLOW_CONFIG: "1" } }) + "\n");
    });
  });
  srv.listen(path);
  return { path, close: () => { socks.forEach((s) => s.destroy()); srv.close(); } };
}

test("fake keys: only in the intended child env, never in model-facing output, session entries or logs", async () => {
  const ipc = shellIpc();
  const logs: string[] = [];
  const orig = console.error;
  console.error = (...a: any[]) => logs.push(a.join(" "));
  try {
    const h = await load({
      tools: ["get_window_state", "click", "run_command"],
      echo_env: ["GUI_GATE_REVIEWER_KEY", "GUI_GATE_ROLE_PLANNER_KEY", "GUI_PI_MAIN_KEY"],
      crash_on: "run_command",
      results: { click: { content: [{ type: "text", text: `page text says ${MAIN} and ${REV}` }],
                          structuredContent: { status: "executed", gate: { note: `echo ${PLAN}` } } } },
    }, { GUI_PI_MAIN_KEY: MAIN, GUI_PI_HUMAN_IPC: ipc.path, GUI_PI_HUMAN_TOKEN: TOKEN });
    // the gate child: its own keys yes; Pi's key, the person token, config switches no
    const genv = h.records()[0].env;
    assert.equal(genv.GUI_GATE_REVIEWER_KEY, REV);
    assert.equal(genv.GUI_GATE_ROLE_PLANNER_KEY, PLAN);
    const { GUI_GATE_FAKE_CFG: _cfg, ...realEnv } = genv; // the test's own script config carries the literals
    const gj = JSON.stringify(realEnv);
    for (const bad of [MAIN, TOKEN, "must-not-pass", "GUI_GATE_ALLOW_CONFIG"]) assert.ok(!gj.includes(bad), `${bad} in the gate env`);
    // Pi's own process environment is the shell's business; the extension never copies gate keys into it
    assert.ok(!JSON.stringify(process.env).includes(REV));

    await h.pi.emit("message_end", { message: { role: "assistant", usage: { input: 1, output: 1, cacheRead: 0 }, content: [{ type: "toolCall", id: "c1" }] } });
    const r = await h.pi.call("click", { x: 1, y: 1 }, "c1");
    const out = JSON.stringify(r);
    for (const k of [MAIN, REV, PLAN, TOKEN]) assert.ok(!out.includes(k), `${k} reached the model`);
    assert.match(out, /\*\*\*/);
    await h.pi.call("note_fact", { fact: `key is ${MAIN}`, source: "page" });
    // the gate dies printing keys on stderr: the user-facing / log line is scrubbed
    await h.pi.call("run_command", { command: "printenv" });
    await new Promise((res) => setTimeout(res, 200));
    const compact = await h.pi.emit("session_before_compact", { preparation: {}, branchEntries: [] });
    const all = JSON.stringify({ entries: h.pi.entries, logs, compact });
    for (const k of [MAIN, REV, PLAN, TOKEN]) assert.ok(!all.includes(k), `${k} in the session or a log`);
    assert.ok(logs.some((l) => l.includes("gui-gate exited")), "the exit was reported");
  } finally {
    console.error = orig;
    ipc.close();
  }
});
