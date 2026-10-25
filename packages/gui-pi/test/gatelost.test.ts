// The gate dies: before a call nothing runs ("blocked"); during a call the outcome is unknown and never retried.
import { test } from "node:test";
import assert from "node:assert/strict";
import { McpClient } from "../src/mcp.ts";

// a fake gate: answers server/discover and tools/list, then exits on the first tools/call (after counting it)
const FAKE = `
const rl = require("readline").createInterface({ input: process.stdin });
rl.on("line", (l) => {
  const m = JSON.parse(l);
  const out = (r) => process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: m.id, result: r }) + "\\n");
  if (m.method === "server/discover") out({ supportedVersions: ["2026-07-28"] });
  else if (m.method === "tools/list") out({ tools: [{ name: "click" }] });
  else if (m.method === "tools/call") { process.stderr.write("calls=1\\n"); process.exit(7); }
});`;

test("gate exits mid-call: unknown, reported once, no retry; later calls: not executed", async () => {
  const c = new McpClient(async () => ({ action: "decline" }));
  let exitCode: number | null | undefined;
  c.onExit = (code) => (exitCode = code);
  c.start(process.execPath, ["-e", FAKE], { PATH: process.env.PATH || "" });
  await c.discover();
  assert.equal((await c.listTools())[0].name, "click");
  const r = await c.callTool("click", { x: 1, y: 2 });
  assert.equal(r.structuredContent.status, "unknown");
  assert.match(r.content[0].text, /outcome of this step is unknown/);
  await new Promise((res) => setTimeout(res, 100));
  assert.equal(exitCode, 7);
  assert.equal(c.stderr.join("").match(/calls=1/g)?.length, 1); // exactly one attempt
  const r2 = await c.callTool("click", { x: 1, y: 2 });
  assert.equal(r2.structuredContent.status, "blocked");
  assert.match(r2.content[0].text, /not running; nothing was executed/);
});
