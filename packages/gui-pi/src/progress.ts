// Process state that must survive compaction even when the model never wrote it down with note_fact. Extracted
// automatically, by the program, from tool results and the gate's structured state:
//   - recent failed methods: what was tried and did not work (refused, blocked, driver error, a batch that stopped,
//     a postcondition the gate saw unmet), newest last;
//   - unverified assumptions: steps whose effect nobody has confirmed (outcome unknown until a read follows; an action
//     the gate executed but whose postcondition it could not check or did not report; check_done's verdict);
//   - open sub-goals / operations: plan milestones not done, batch steps that did not run, a step that waits for a
//     person, an unknown outcome not re-read yet, check_done gaps;
//   - temporary constraints: what the gate or the person ruled for the rest of this task (do not repeat a step a
//     person did, read tools cannot write, a person not asked again about X, a platform condition, ...).
// It is saved in the session (pi.appendEntry) after every change, restored on session_start and rendered into the
// deterministic compaction summary next to the task facts. Nothing in it authorises anything.

export const PROGRESS_ENTRY = "gui-pi-progress";
export const MAX_FAILURES = 8;
export const MAX_ITEMS = 12;

export type Item = { text: string; at: string; key?: string };

export type ProgressData = { failures: Item[]; unverified: Item[]; open: Item[]; constraints: Item[] };

function short(v: any, n = 140): string {
  let s = typeof v === "string" ? v : JSON.stringify(v ?? {});
  s = s.replace(/\s+/g, " ").trim();
  return s.length > n ? s.slice(0, n) + "..." : s;
}

/** A one-line description of a call: tool + the arguments that say what it aimed at. */
export function describeCall(tool: string, args: any): string {
  const a = args && typeof args === "object" ? args : {};
  const keys = ["element_token", "element_index", "label", "text", "keys", "key", "command", "path", "x", "y", "menu", "value", "steps"];
  const parts: string[] = [];
  for (const k of keys) if (a[k] !== undefined) parts.push(`${k}=${short(a[k], k === "steps" ? 80 : 60)}`);
  return `${tool}${parts.length ? " " + parts.join(" ") : ""}`;
}

function push(list: Item[], text: string, max: number, key?: string): boolean {
  const t = short(text, 300);
  if (!t) return false;
  const k = key ?? t;
  const i = list.findIndex((x) => (x.key ?? x.text) === k);
  if (i >= 0) {
    if (list[i].text === t) return false;
    list.splice(i, 1);
  }
  list.push({ text: t, at: new Date().toISOString(), key: k });
  while (list.length > max) list.shift();
  return true;
}

function drop(list: Item[], pred: (x: Item) => boolean): boolean {
  const n = list.length;
  for (let i = list.length - 1; i >= 0; i--) if (pred(list[i])) list.splice(i, 1);
  return list.length !== n;
}

/** Postcondition evidence in a gate result (gate v0.10.3 names, read tolerantly): [{what, met, detail}]. */
export type Post = { what: string; met: boolean | null; detail: string };
export function postconditions(sc: any): Post[] {
  const raw = sc?.postconditions ?? sc?.postcondition ?? sc?.evidence ?? sc?.state?.postconditions ?? null;
  if (raw === null || raw === undefined) return [];
  const arr = Array.isArray(raw) ? raw : [raw];
  return arr.filter((x) => x !== null && x !== undefined).map((x: any) => {
    if (typeof x === "string") return { what: short(x, 160), met: null, detail: "" };
    const okv = x.met ?? x.ok ?? x.passed ?? x.satisfied ?? x.verified;
    const met = okv === true ? true : okv === false ? false : null;
    return {
      what: short(x.what ?? x.check ?? x.name ?? x.expect ?? x.condition ?? x.description ?? "postcondition", 160),
      met,
      detail: short(x.detail ?? x.observed ?? x.evidence ?? x.reason ?? "", 160),
    };
  });
}

/** Constraints the gate states in its structured result (gate v0.10.3: `constraints`, read tolerantly). */
export function gateConstraints(sc: any): string[] {
  const raw = sc?.constraints ?? sc?.state?.constraints ?? null;
  if (!raw) return [];
  return (Array.isArray(raw) ? raw : [raw]).map((x: any) => (typeof x === "string" ? x : x?.text ?? x?.rule ?? JSON.stringify(x))).filter(Boolean).map((s: string) => short(s, 200));
}

export type ResultInfo = {
  tool: string;
  args: any;
  /** interpreted status (status.ts) */
  status: string;
  isError: boolean;
  refused: boolean;
  /** whether the step is an action (not a read / plan / report) */
  action: boolean;
  /** the result carries a fresh window observation */
  observed: boolean;
  /** first line of the gate's text (why it refused / what the driver said) */
  why: string;
  sc: any;
  steps?: { ran: number; total: number | null; completed?: number[]; notRun?: number[] } | null;
  human?: string | null;
  choice?: string | null;
  platform?: string | null;
};

export class Progress {
  failures: Item[] = [];
  unverified: Item[] = [];
  open: Item[] = [];
  constraints: Item[] = [];

  /** Update from one gate tool result. Returns true when anything changed. */
  onResult(r: ResultInfo): boolean {
    let ch = false;
    const call = describeCall(r.tool, r.args);
    const sc = r.sc || {};
    // a fresh read settles what was waiting for one
    if (r.tool.endsWith("get_window_state") && !r.isError) {
      ch = drop(this.open, (x) => x.key === "reread") || ch;
      ch = drop(this.unverified, (x) => String(x.key || "").startsWith("unknown:")) || ch;
    }
    if (r.status === "unknown") {
      ch = push(this.unverified, `${call}: outcome unknown - it may or may not have happened`, MAX_ITEMS, `unknown:${call}`) || ch;
      ch = push(this.open, "re-read the window before any action (an earlier outcome is unknown; do not resend it blind)", MAX_ITEMS, "reread") || ch;
    }
    if (r.refused) ch = push(this.failures, `${call}: refused by the gate${r.human ? ` (person: ${r.human})` : ""} - ${r.why}`, MAX_FAILURES) || ch;
    else if (r.choice === "another_way") ch = push(this.failures, `${call}: the person asked for another way (not a refusal)`, MAX_FAILURES) || ch;
    else if (r.status === "blocked" && !r.human) ch = push(this.failures, `${call}: not executed - ${r.why}`, MAX_FAILURES) || ch;
    else if (r.isError && r.action && r.status !== "unknown") ch = push(this.failures, `${call}: error - ${r.why}`, MAX_FAILURES) || ch;
    if (r.status === "partial" && r.steps) {
      const notRun = r.steps.notRun?.length ? `steps ${r.steps.notRun.map((i) => i + 1).join(", ")}` : `steps after step ${r.steps.ran}`;
      ch = push(this.failures, `${call}: batch stopped after ${r.steps.ran}${r.steps.total !== null ? ` of ${r.steps.total}` : ""} steps - ${r.why}`, MAX_FAILURES) || ch;
      ch = push(this.open, `run_actions: ${notRun} did not run (continue from the window, do not resend the steps that ran)`, MAX_ITEMS, `partial:${call}`) || ch;
    }
    if (r.status === "input_required") ch = push(this.open, `${call}: needs a person; not executed`, MAX_ITEMS, `input:${call}`) || ch;
    if (r.status === "done_by_person") {
      ch = push(this.constraints, `a person did "${call}" themselves: never send it again`, MAX_ITEMS, `person:${call}`) || ch;
      ch = drop(this.open, (x) => x.key === `input:${call}`) || ch;
    }
    if (sc?.gate?.tier === "write_args") ch = push(this.constraints, "read tools cannot write files (no destination argument on get_window_state / inspect)", MAX_ITEMS, "write_args") || ch;
    if (r.human === "not_asked_again") ch = push(this.constraints, `the person is not asked again about "${call}" in this task`, MAX_ITEMS, `naa:${call}`) || ch;
    if (r.platform) ch = push(this.constraints, r.platform, MAX_ITEMS, "platform") || ch;
    for (const c of gateConstraints(sc)) ch = push(this.constraints, `gate: ${c}`, MAX_ITEMS) || ch;
    // postconditions: met -> settles the step; unmet -> a failed method; unchecked -> unverified
    const posts = postconditions(sc);
    if (r.action && (r.status === "executed" || r.status === "done")) {
      if (posts.length) {
        for (const p of posts) {
          if (p.met === true) ch = drop(this.unverified, (x) => x.key === `post:${call}`) || ch;
          else if (p.met === false) ch = push(this.failures, `${call}: ran, but "${p.what}" was not met${p.detail ? ` (${p.detail})` : ""}`, MAX_FAILURES) || ch;
          else ch = push(this.unverified, `${call}: ran; "${p.what}" not checked`, MAX_ITEMS, `post:${call}`) || ch;
        }
      } else if (!r.observed && !r.isError) {
        ch = push(this.unverified, `${call}: ran; nothing observed its effect yet`, MAX_ITEMS, `post:${call}`) || ch;
      } else if (r.observed) {
        ch = drop(this.unverified, (x) => x.key === `post:${call}`) || ch;
      }
    }
    if (r.tool === "check_done") {
      const gaps = Array.isArray(sc.gaps) ? sc.gaps : [];
      ch = drop(this.open, (x) => String(x.key || "").startsWith("gap:")) || ch;
      for (const g of gaps.slice(0, 6)) ch = push(this.open, `check_done gap: ${short(typeof g === "string" ? g : g?.text ?? g?.gap ?? g, 200)}`, MAX_ITEMS, `gap:${short(g, 60)}`) || ch;
      if (!gaps.length && !r.isError)
        ch = push(this.unverified, "check_done reported no gap: a model's judgement, not proof from the environment; confirm the result by reading it", MAX_ITEMS, "check_done") || ch;
    }
    return ch;
  }

  /** Plan milestones not done (plan.ts), kept as open sub-goals. */
  setPlan(milestones: { text: string; done: boolean; failed: boolean }[]): boolean {
    const before = JSON.stringify(this.open.filter((x) => String(x.key || "").startsWith("plan:")).map((x) => x.text));
    drop(this.open, (x) => String(x.key || "").startsWith("plan:"));
    milestones.forEach((m, i) => {
      if (!m.done) push(this.open, `plan step ${i + 1}${m.failed ? " (failed - change the approach)" : ""}: ${m.text}`, MAX_ITEMS + milestones.length, `plan:${i}`);
    });
    return JSON.stringify(this.open.filter((x) => String(x.key || "").startsWith("plan:")).map((x) => x.text)) !== before;
  }

  /** Recent failed methods as short lines (the help popup shows them when the gate sends none). */
  recentFailures(n = 4): string[] {
    return this.failures.slice(-n).map((x) => x.text);
  }

  toJSON(): ProgressData {
    return { failures: this.failures, unverified: this.unverified, open: this.open, constraints: this.constraints };
  }

  load(d: Partial<ProgressData> | null | undefined) {
    if (!d || typeof d !== "object") return;
    const items = (v: any) => (Array.isArray(v) ? v.filter((x: any) => x && typeof x.text === "string") : []);
    this.failures = items(d.failures);
    this.unverified = items(d.unverified);
    this.open = items(d.open);
    this.constraints = items(d.constraints);
  }

  restore(branch: any[]) {
    for (let i = (branch || []).length - 1; i >= 0; i--) {
      const e = branch[i];
      if (e?.type === "custom" && e.customType === PROGRESS_ENTRY) {
        this.load(e.data);
        return true;
      }
    }
    return false;
  }

  render(): string {
    const sec = (title: string, l: Item[]) => (l.length ? `${title}\n- ${l.map((x) => x.text).join("\n- ")}` : "");
    const parts = [
      sec("Tried and did not work (do not repeat these as they were):", this.failures),
      sec("Not verified yet (assumptions; confirm before relying on them):", this.unverified),
      sec("Still open (sub-goals / operations not finished):", this.open),
      sec("Constraints for the rest of this task:", this.constraints),
    ].filter(Boolean);
    return parts.length ? "Process state (extracted by the program from tool results and the gate's structured state):\n\n" + parts.join("\n\n") : "";
  }
}
