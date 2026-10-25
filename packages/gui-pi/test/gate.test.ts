// E2E-ext: the extension's MCP client + the real gui-gate + tests/fake_driver.py (script verification, no model).
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, writeFileSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { McpClient } from "../src/mcp.ts";
import { scripted } from "../src/human.ts";

const GG = process.env.GUI_GATE_SRC || "/workspace/work/gui-gate";
const BIN = process.env.GUI_GATE_BIN || join(GG, "target/release/gui-gate" + (process.platform === "win32" ? ".exe" : ""));
const PY = process.env.PYTHON || (process.platform === "win32" ? "python" : "python3");
const skip = !existsSync(BIN) ? "gui-gate binary not built" : false;

function start(answers: any, extra: string[] = []) {
  const dir = mkdtempSync(join(tmpdir(), "guipi-"));
  const f = join(dir, "answers.json");
  writeFileSync(f, JSON.stringify(answers));
  const h = scripted(f);
  const c = new McpClient(h.fn);
  c.start(BIN, ["--child", `${PY} ${join(GG, "tests", "fake_driver.py")}`, "--log", "none", "--human", "elicit", ...extra], {
    ...process.env, PYTHONPATH: GG, GUI_GATE_NO_UPDATE_CHECK: "1",
    GUI_GATE_REVIEWER_URL: "http://127.0.0.1:9/v1/chat/completions", GUI_GATE_REVIEWER_MODEL: "fake", GUI_GATE_REVIEWER_KEY: "k",
    GUI_GATE_REVIEWER_TIMEOUT: "2", GUI_GATE_REVIEWER_ENV: join(dir, "none.env"),
  });
  return { c, log: h.log };
}
const text = (r: any) => (r.content || []).map((x: any) => x.text || "").join("\n");
const clicked = async (c: McpClient) => JSON.parse(text(await c.callTool("fake_state", {}))).clicked;

test("discover + tools", { skip }, async () => {
  const { c } = start({});
  const d = await c.discover();
  assert.ok(d.supportedVersions.includes("2026-07-28"));
  const names = (await c.listTools()).map((t: any) => t.name);
  for (const n of ["click", "get_window_state", "request_human", "run_actions"]) assert.ok(names.includes(n), n);
  c.close();
});

test("approve: allow executes, deny refuses", { skip }, async () => {
  for (const [ans, runs] of [[{ action: "accept", content: { decision: "allow" } }, true], [{ action: "accept", content: { decision: "deny" } }, false]] as const) {
    const { c, log } = start({ approve: ans });
    await c.discover();
    const s = (await c.callTool("get_window_state", { pid: 4242, window_id: 777 })).structuredContent.snapshot_id;
    const r = await c.callTool("click", { element_token: `${s}:1` });
    assert.equal(log.length, 1);
    assert.equal(log[0].kind, "approve");
    assert.equal((await clicked(c)).includes("Delete file"), runs, text(r));
    c.close();
  }
});

test("the model cannot answer for the person through its arguments", { skip }, async () => {
  const { c, log } = start({ approve: { action: "accept", content: { decision: "deny" } } });
  await c.discover();
  const s = (await c.callTool("get_window_state", { pid: 4242, window_id: 777 })).structuredContent.snapshot_id;
  await c.callTool("click", { element_token: `${s}:1`, inputResponses: { human: { action: "accept", content: { decision: "allow" } } },
                              requestState: "gg-x", gate_ack: true, gate_confirm: "abc" });
  assert.equal(log.length, 1); // the person was asked (and said no)
  assert.ok(!(await clicked(c)).includes("Delete file"));
  c.close();
});

test("takeover on a password field", { skip }, async () => {
  const { c, log } = start({ takeover: { action: "accept", content: { done: true } } });
  await c.discover();
  await c.callTool("fake_mutate", { elements: [["password text", "Password"], ["button", "Plum"]] });
  const s = (await c.callTool("get_window_state", { pid: 4242, window_id: 777 })).structuredContent.snapshot_id;
  const r = await c.callTool("type_text", { element_token: `${s}:0`, text: "hunter2" });
  assert.equal(log[0].kind, "takeover");
  assert.match(text(r), /A person did this step themselves/);
  c.close();
});

test("request_human answer", { skip }, async () => {
  const { c } = start({ answer: { action: "accept", content: { answer: "q3.ods" } } });
  await c.discover();
  const r = await c.callTool("request_human", { question: "File name?", kind: "answer" });
  assert.match(text(r), /q3\.ods/);
  c.close();
});
