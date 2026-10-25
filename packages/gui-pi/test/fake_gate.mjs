// A scriptable stand-in for gui-gate (MCP 2026-07-28 over stdio) for extension tests.
//   GUI_GATE_FAKE_CFG  JSON: {tools: [names listed in tools/list], results: {tool: result | [result, ...]},
//                         default: result, echo_env: ["VAR", ...] (values appended to every tools/call text)}
//   GUI_GATE_FAKE_REC  file: one JSON line per request received, plus {"env": {...}} once at start
// A result list is consumed in order; the last one repeats. Unknown tools get the default result
// (like an older gate, which passes unknown names on to the driver).
import { createInterface } from "node:readline";
import { appendFileSync } from "node:fs";

const cfg = JSON.parse(process.env.GUI_GATE_FAKE_CFG || "{}");
const rec = process.env.GUI_GATE_FAKE_REC;
const log = (o) => rec && appendFileSync(rec, JSON.stringify(o) + "\n");
log({ env: process.env, argv: process.argv.slice(2) });
const used = {};
const rl = createInterface({ input: process.stdin });
rl.on("line", (l) => {
  let m;
  try { m = JSON.parse(l); } catch { return; }
  log({ method: m.method, params: m.params });
  const out = (r) => process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: m.id, result: r }) + "\n");
  if (m.method === "server/discover") return out({ supportedVersions: ["2026-07-28"] });
  if (m.method === "tools/list")
    return out({ tools: (cfg.tools || []).map((n) => ({ name: n, description: n, inputSchema: { type: "object", properties: {} } })) });
  if (m.method === "tools/call") {
    const name = m.params?.name;
    if (cfg.crash_on === name) {
      process.stderr.write((cfg.echo_env || []).map((k) => `${k}=${process.env[k] ?? ""}`).join(" ") + "\n");
      process.exit(3);
    }
    let r = cfg.results?.[name] ?? cfg.default ?? { content: [{ type: "text", text: `${name} ok` }], structuredContent: { status: "done" } };
    if (Array.isArray(r)) {
      const i = used[name] = (used[name] ?? -1) + 1;
      r = r[Math.min(i, r.length - 1)];
    }
    r = JSON.parse(JSON.stringify(r));
    if (cfg.echo_env?.length) {
      r.content = r.content || [];
      r.content.push({ type: "text", text: cfg.echo_env.map((k) => `${k}=${process.env[k] ?? ""}`).join(" ") });
    }
    return out({ resultType: "complete", ...r });
  }
});
