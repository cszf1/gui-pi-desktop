// Loads the real extension (index.ts) against a mock Pi and the scriptable fake gate (fake_gate.mjs).
import { mkdtempSync, writeFileSync, readFileSync, existsSync, chmodSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import ext from "../index.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
export const WIN = process.platform === "win32";

/** A listen address for a fake shell IPC: a Unix socket in a fresh temp dir, a named pipe on Windows. */
export function ipcPath(tag: string): string {
  if (WIN) return `\\\\.\\pipe\\gpd-test-${tag}-${process.pid}-${Math.random().toString(16).slice(2)}`;
  return join(mkdtempSync(join(tmpdir(), `${tag}-`)), "h.sock");
}

export class MockPi {
  handlers: Record<string, any[]> = {};
  tools = new Map<string, any>();
  entries: { type: string; data: any }[] = [];
  on(ev: string, fn: any) {
    (this.handlers[ev] ||= []).push(fn);
  }
  registerTool(t: any) {
    this.tools.set(t.name, t);
  }
  appendEntry(type: string, data: any) {
    this.entries.push({ type, data });
  }
  async emit(ev: string, e: any = {}, ctx: any = {}) {
    let r: any;
    for (const fn of this.handlers[ev] || []) {
      const x = await fn(e, ctx);
      if (x !== undefined) r = x;
    }
    return r;
  }
  /** A model-issued call: tool_call handlers, then execute. */
  async call(name: string, args: any = {}, id = `c${Math.random().toString(16).slice(2)}`) {
    const blocked = await this.emit("tool_call", { toolName: name, input: args, toolCallId: id });
    if (blocked?.block) return { blocked: true, reason: blocked.reason };
    return this.tools.get(name).execute(id, args);
  }
}

export async function load(cfg: any, env: Record<string, string> = {}) {
  const dir = mkdtempSync(join(tmpdir(), "guipi-h-"));
  const rec = join(dir, "rec.jsonl");
  // Windows cannot run a #! wrapper: the extension starts a gate given as a .mjs with node itself
  let gate = join(HERE, "fake_gate.mjs");
  if (!WIN) {
    gate = join(dir, "gate.sh");
    writeFileSync(gate, `#!/bin/sh\nexec "${process.execPath}" "${join(HERE, "fake_gate.mjs")}" "$@"\n`);
    chmodSync(gate, 0o755);
  }
  const fullEnv: NodeJS.ProcessEnv = {
    PATH: process.env.PATH || "/usr/bin:/bin", HOME: dir, GUI_PI_GATE: gate,
    GUI_GATE_FAKE_CFG: JSON.stringify(cfg), GUI_GATE_FAKE_REC: rec, ...env,
  };
  const pi = new MockPi();
  const api: any = await ext(pi, fullEnv);
  await pi.emit("session_start", {}, { hasUI: false, sessionManager: { getBranch: () => [], getSessionDir: () => dir, getSessionId: () => "t1" } });
  const records = () => (existsSync(rec) ? readFileSync(rec, "utf8").trim().split("\n").filter(Boolean).map((l) => JSON.parse(l)) : []);
  const calls = () => records().filter((r) => r.method === "tools/call").map((r) => r.params);
  return { pi, api, dir, rec, records, calls, env: fullEnv };
}

export const txt = (r: any) => (r?.content || []).filter((c: any) => c.type === "text").map((c: any) => c.text).join("\n");
export const IMG = Buffer.from([0xff, 0xd8, 0xff, 0xe0, 1, 2, 3, 4, 250, 251, 0xff, 0xd9]).toString("base64");
export const read = (pid = 42, win = 7, snap = "s1", image = false) => ({
  content: [{ type: "text", text: `window_id=${win} pid=${pid} snapshot_id=${snap}\n- [0] button "OK"` },
            ...(image ? [{ type: "image", data: IMG, mimeType: "image/jpeg" }] : [])],
  structuredContent: { status: "done", snapshot_id: snap, pid, window_id: win },
});
