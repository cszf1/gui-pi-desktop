// Ask-a-person bridge: an MRTR elicitation from gui-gate -> the person -> an ElicitResult.
// Order:
//   1. the desktop shell's human IPC (GUI_PI_HUMAN_IPC + GUI_PI_HUMAN_TOKEN): native popup windows
//   2. GUI_PI_HUMAN_SCRIPT (tests only): a JSON file of scripted answers per kind
//   3. Pi's own UI (ctx.ui.select / input with a timeout) when Pi runs interactively
//   4. nobody: decline (the gate refuses)
// Whatever happens, a missing / late / malformed answer is a refusal, never an approval.
import { connect } from "node:net";
import { readFileSync } from "node:fs";
import { GG, type ElicitParams, type ElicitResult } from "./mcp.ts";

export type Kind = "approve" | "answer" | "takeover" | "ask_help";
export const KINDS: Kind[] = ["approve", "answer", "takeover", "ask_help"];

export function kindOf(p: ElicitParams): Kind {
  const k = p?._meta?.[GG]?.kind;
  return KINDS.includes(k) ? k : "approve";
}

export function timeoutOf(p: ElicitParams, dflt = 120): number {
  const t = Number(p?._meta?.[GG]?.timeout_s);
  return Number.isFinite(t) && t > 0 ? t : dflt;
}

const DECLINE: ElicitResult = { action: "decline" };
/** Why a person's answer is a refusal; carried to the gate in the ElicitResult's _meta (the gate records it). */
export const REASONS = ["declined", "cancelled", "timed_out", "closed", "unavailable"];

export function refusal(action: "decline" | "cancel", reason?: string): ElicitResult {
  const r: ElicitResult = { action };
  if (reason && REASONS.includes(reason)) r._meta = { [GG]: { reason } };
  return r;
}

/** Validate an answer against the kind's form; anything off is a decline. */
export function sanitize(kind: Kind, r: any): ElicitResult {
  if (!r || typeof r !== "object") return DECLINE;
  if (r.action !== "accept") return refusal(r.action === "cancel" ? "cancel" : "decline", typeof r.reason === "string" ? r.reason : undefined);
  const c = r.content || {};
  switch (kind) {
    case "approve":
      return c.decision === "allow" || c.decision === "deny" ? { action: "accept", content: { decision: c.decision } } : DECLINE;
    case "answer":
      return typeof c.answer === "string" && c.answer.trim() ? { action: "accept", content: { answer: c.answer.slice(0, 2000) } } : DECLINE;
    case "takeover":
      return typeof c.done === "boolean" ? { action: "accept", content: { done: c.done } } : DECLINE;
    case "ask_help":
      if (c.choice !== "did_it" && c.choice !== "try_other") return DECLINE;
      return { action: "accept", content: { choice: c.choice, note: typeof c.note === "string" ? c.note.slice(0, 300) : "" } };
  }
}

/** One request over the shell's IPC socket (Unix socket or Windows named pipe). */
export function askIpc(path: string, token: string, p: ElicitParams, signal?: AbortSignal): Promise<ElicitResult> {
  const kind = kindOf(p);
  const timeout = timeoutOf(p);
  const id = Math.random().toString(16).slice(2);
  return new Promise((resolve) => {
    let done = false;
    let buf = "";
    const finish = (r: ElicitResult) => {
      if (done) return;
      done = true;
      clearTimeout(t);
      sock.destroy();
      resolve(r);
    };
    const t = setTimeout(() => finish(refusal("decline", "timed_out")), (timeout + 2) * 1000);
    signal?.addEventListener("abort", () => finish(refusal("cancel", "cancelled")), { once: true });
    // "tcp:127.0.0.1:PORT" (Windows) or a Unix socket path
    const tcp = path.match(/^tcp:([^:]+):(\d+)$/);
    const sock = tcp ? connect({ host: tcp[1], port: Number(tcp[2]) }) : connect(path);
    sock.on("error", () => finish(refusal("decline", "unavailable")));
    sock.on("connect", () => {
      sock.write(JSON.stringify({ token, id, kind, message: p.message, schema: p.requestedSchema, timeout_s: timeout,
                                  deadline_ms: p._meta?.[GG]?.deadline_ms, meta: p._meta?.[GG] || {} }) + "\n");
    });
    sock.on("data", (d) => {
      buf += String(d);
      const nl = buf.indexOf("\n");
      if (nl < 0) return;
      try {
        const r = JSON.parse(buf.slice(0, nl));
        finish(r.id === id ? sanitize(kind, r) : DECLINE);
      } catch {
        finish(DECLINE);
      }
    });
    sock.on("close", () => finish(refusal("cancel", "cancelled"))); // the shell went away mid-question
  });
}

/** Tests only: {"approve": {...ElicitResult}, "takeover": ..., "default": ...}; answers are consumed in order if arrays. */
export function scripted(file: string) {
  const rules = JSON.parse(readFileSync(file, "utf8"));
  const log: any[] = [];
  const fn = async (p: ElicitParams): Promise<ElicitResult> => {
    const kind = kindOf(p);
    let a = rules[kind] ?? rules.default ?? DECLINE;
    if (Array.isArray(a)) a = a.length ? a.shift() : DECLINE;
    log.push({ kind, message: p.message, answer: a });
    if (a === "silent") {
      await new Promise((r) => setTimeout(r, (timeoutOf(p) + 6) * 1000));
      return { action: "accept", content: { decision: "allow" } }; // too late: the gate must treat it as a timeout
    }
    return sanitize(kind, a);
  };
  return { fn, log };
}

/** Pi's own dialogs (TUI / RPC) with the gate's timeout. */
export async function askPiUi(ui: any, p: ElicitParams): Promise<ElicitResult> {
  const kind = kindOf(p);
  const timeout = timeoutOf(p) * 1000;
  const title = p._meta?.[GG]?.title || "gui-gate: a person is needed";
  if (kind === "answer") {
    const v = await ui.input(`${title}\n${p.message}`, "", { timeout });
    return typeof v === "string" && v.trim() ? { action: "accept", content: { answer: v } } : DECLINE;
  }
  const opts: Record<Kind, [string, string]> = {
    approve: ["Allow", "Deny"],
    takeover: ["I did it, continue", "No"],
    ask_help: ["I did this step myself, continue", "Let the agent try another way"],
    answer: ["", ""],
  };
  const [yes, no] = opts[kind];
  const v = await ui.select(`${title}\n${p.message}`, [yes, no], { timeout });
  if (v === undefined) return DECLINE;
  const pos = v === yes;
  if (kind === "approve") return { action: "accept", content: { decision: pos ? "allow" : "deny" } };
  if (kind === "takeover") return { action: "accept", content: { done: pos } };
  return { action: "accept", content: { choice: pos ? "did_it" : "try_other", note: "" } };
}

export function makeAskHuman(env: NodeJS.ProcessEnv, getUi: () => any) {
  const ipc = env.GUI_PI_HUMAN_IPC;
  const token = env.GUI_PI_HUMAN_TOKEN || "";
  const script = env.GUI_PI_HUMAN_SCRIPT ? scripted(env.GUI_PI_HUMAN_SCRIPT) : null;
  const ask = async (p: ElicitParams, signal?: AbortSignal): Promise<ElicitResult> => {
    if (ipc) return askIpc(ipc, token, p, signal);
    if (script) return script.fn(p);
    const ui = getUi();
    if (ui) {
      try {
        return await askPiUi(ui, p);
      } catch {
        return refusal("decline", "unavailable");
      }
    }
    return refusal("decline", "unavailable");
  };
  return { ask, log: script?.log || [] };
}
