// MCP 2026-07-28 client for gui-gate over stdio.
// - every request carries `_meta` (protocolVersion, clientCapabilities with form elicitation, clientInfo)
// - person requests come back as Multi Round-Trip Requests: a tools/call answers `resultType: "input_required"`
//   with `inputRequests` (elicitation/create, mode form) and an opaque `requestState`; we ask the person through
//   `askHuman` and retry the same call with a NEW id, `inputResponses` and the exact `requestState`.
// - the model only ever controls `arguments`; keys the gate reads from the params top level are stripped from them.
import { spawn, type ChildProcess } from "node:child_process";
import { createInterface } from "node:readline";

export const PROTOCOL = "2026-07-28";
const PV = "io.modelcontextprotocol/protocolVersion";
const CC = "io.modelcontextprotocol/clientCapabilities";
const CI = "io.modelcontextprotocol/clientInfo";
export const GG = "io.github.cszf1/gui-gate";
const MAX_ROUNDS = 3;
/** keys the model must never be able to set: they would let it answer for the person (the last three are legacy
 *  gate keys, stripped in case an old gate still reads them) */
export const FORBIDDEN_ARGS = ["inputResponses", "requestState", "_meta", "gate_confirm", "gate_ack", "gate_task"];
/** Per-call `_meta` key with the model's real token usage for the turn that produced the call (gate >= 0.10.1 reads it). */
export const USAGE_META = "gui_pi/usage";
/** Gate tools only the client calls (never registered with Pi, never offered to the model). */
export const CLIENT_ONLY_TOOLS = new Set(["gate_next_step", "gate_reset"]);

export type ElicitParams = {
  mode?: string;
  message: string;
  requestedSchema: any;
  _meta?: Record<string, any>;
};
export type ElicitResult = { action: "accept" | "decline" | "cancel"; content?: Record<string, any>; _meta?: Record<string, any> };
export type AskHuman = (p: ElicitParams, signal?: AbortSignal) => Promise<ElicitResult>;

export type ToolResult = {
  content: any[];
  isError?: boolean;
  structuredContent?: any;
  resultType?: string;
  _meta?: any;
};

export function cleanArgs(args: any): Record<string, any> {
  const out: Record<string, any> = {};
  if (!args || typeof args !== "object" || Array.isArray(args)) return out;
  for (const [k, v] of Object.entries(args)) if (!FORBIDDEN_ARGS.includes(k)) out[k] = v;
  return out;
}

/** Line-delimited JSON-RPC over a child's stdio. */
export class McpClient {
  child: ChildProcess | null = null;
  nextId = 1;
  pending = new Map<number, { resolve: (m: any) => void; reject: (e: Error) => void }>();
  askHuman: AskHuman;
  clientVersion: string;
  stderr: string[] = [];
  onExit: ((code: number | null) => void) | null = null;
  closed = false;
  /** set by Stop: nothing more is sent to the gate, whatever asks */
  stopped = false;
  /** we closed the gate's stdin ourselves (session end): its exit is expected */
  closing = false;

  constructor(askHuman: AskHuman, clientVersion = "0.1.0") {
    this.askHuman = askHuman;
    this.clientVersion = clientVersion;
  }

  start(cmd: string, args: string[], env: NodeJS.ProcessEnv) {
    const child = spawn(cmd, args, { stdio: ["pipe", "pipe", "pipe"], env });
    this.child = child;
    const rl = createInterface({ input: child.stdout! });
    rl.on("line", (line) => this.onLine(line));
    child.stderr!.on("data", (d) => {
      this.stderr.push(String(d));
      if (this.stderr.length > 50) this.stderr.shift();
    });
    child.on("exit", (code) => {
      this.closed = true;
      for (const p of this.pending.values()) p.reject(new Error(`gui-gate exited (${code}): ${this.stderr.join("").slice(-500)}`));
      this.pending.clear();
      this.onExit?.(code);
    });
  }

  onLine(line: string) {
    let m: any;
    try {
      m = JSON.parse(line);
    } catch {
      return;
    }
    if (m.method && m.id !== undefined) {
      // a legacy server-to-client request: a 2026-07-28 server must not send one; refuse it
      this.write({ jsonrpc: "2.0", id: m.id, error: { code: -32601, message: "use MRTR (2026-07-28)" } });
      return;
    }
    if (m.id === undefined) return; // notifications
    const p = this.pending.get(m.id);
    if (p) {
      this.pending.delete(m.id);
      p.resolve(m);
    }
  }

  write(obj: any) {
    if (!this.child || this.closed) throw new Error("gui-gate is not running");
    this.child.stdin!.write(JSON.stringify(obj) + "\n");
  }

  meta() {
    return { [PV]: PROTOCOL, [CC]: { elicitation: { form: {} } }, [CI]: { name: "gui-pi", version: this.clientVersion } };
  }

  request(method: string, params: Record<string, any> = {}, timeoutMs = 600_000, extraMeta?: Record<string, any>): Promise<any> {
    if (this.stopped) return Promise.reject(new Error("stopped by the user"));
    const id = this.nextId++;
    const msg = { jsonrpc: "2.0", id, method, params: { ...params, _meta: { ...(extraMeta || {}), ...this.meta() } } };
    return new Promise((resolve, reject) => {
      const t = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`${method} timed out after ${timeoutMs} ms`));
      }, timeoutMs);
      this.pending.set(id, {
        resolve: (m) => {
          clearTimeout(t);
          resolve(m);
        },
        reject: (e) => {
          clearTimeout(t);
          reject(e);
        },
      });
      try {
        this.write(msg);
      } catch (e: any) {
        clearTimeout(t);
        this.pending.delete(id);
        reject(e);
      }
    });
  }

  async discover() {
    const m = await this.request("server/discover", {}, 60_000);
    if (m.error) throw new Error(`server/discover: ${m.error.message}`);
    if (!(m.result?.supportedVersions || []).includes(PROTOCOL)) throw new Error(`gui-gate does not speak MCP ${PROTOCOL}`);
    return m.result;
  }

  async listTools(): Promise<any[]> {
    const m = await this.request("tools/list", {}, 60_000);
    if (m.error) throw new Error(`tools/list: ${m.error.message}`);
    return m.result?.tools || [];
  }

  /** tools/call with MRTR person rounds. `extraMeta` (e.g. USAGE_META) goes into the request's `_meta`, never into
   *  `arguments`; the protocol keys always win over it. */
  async callTool(name: string, rawArgs: any, signal?: AbortSignal, extraMeta?: Record<string, any>, timeoutMs = 600_000): Promise<ToolResult> {
    const args = cleanArgs(rawArgs);
    if (this.stopped)
      return { content: [{ type: "text", text: "[gui-pi] the user pressed Stop; nothing was executed." }],
               isError: true, structuredContent: { status: "blocked", stopped: true } };
    if (this.closed || !this.child)
      return { content: [{ type: "text", text: "[gui-pi] gui-gate is not running; nothing was executed. The task cannot continue." }],
               isError: true, structuredContent: { status: "blocked", gate_lost: true } };
    let params: Record<string, any> = { name, arguments: args };
    for (let round = 0; round <= MAX_ROUNDS; round++) {
      let m: any;
      try {
        m = await this.request("tools/call", params, timeoutMs, extraMeta);
      } catch (e: any) {
        if (this.stopped)
          return { content: [{ type: "text", text: "[gui-pi] the user pressed Stop while this step was in flight; its outcome is unknown." }],
                   isError: true, structuredContent: { status: "unknown", stopped: true } };
        // the gate died or stopped answering while the step was in flight: it may or may not have run. Never retried.
        return { content: [{ type: "text", text: `[gui-pi] no result from gui-gate (${String(e?.message || e).slice(0, 300)}). ` +
                 "The outcome of this step is unknown." }], isError: true, structuredContent: { status: "unknown", gate_lost: true } };
      }
      if (m.error) return { content: [{ type: "text", text: `[gui-pi] gui-gate error ${m.error.code}: ${m.error.message}` }], isError: true };
      const r = m.result || {};
      if (r.resultType !== "input_required") return r;
      const responses: Record<string, ElicitResult> = {};
      for (const [k, req] of Object.entries<any>(r.inputRequests || {})) {
        if (req?.method !== "elicitation/create" || (req.params?.mode && req.params.mode !== "form")) {
          responses[k] = { action: "decline" };
          continue;
        }
        let ans: ElicitResult;
        try {
          ans = await this.askHuman(req.params, signal);
        } catch {
          ans = { action: "cancel" };
        }
        responses[k] = ans && ["accept", "decline", "cancel"].includes(ans.action) ? ans : { action: "cancel" };
      }
      params = { name, arguments: args, inputResponses: responses };
      if (r.requestState !== undefined) params.requestState = r.requestState;
    }
    return { content: [{ type: "text", text: "[gui-pi] too many ask-a-person rounds; nothing more was done" }], isError: true };
  }

  close() {
    this.closing = true;
    try {
      this.child?.stdin?.end();
    } catch {}
  }

  /** Stop: refuse every later request and kill the gate now (it takes its driver with it). */
  kill() {
    this.stopped = true;
    for (const p of this.pending.values()) p.reject(new Error("stopped by the user"));
    this.pending.clear();
    try {
      this.child?.kill("SIGKILL");
    } catch {}
  }
}
