// Task facts: what must survive compaction word for word, kept by the program (never re-summarised by a model).
//   - every message the user wrote (the first request AND each later correction), verbatim, with a version and its
//     source; this record is never shortened (only its rendering for display may be, and then it says so)
//   - facts with their kind and source: an agent note (note_fact; not verified by anything), tool evidence (a gate
//     result that observed it, e.g. a met postcondition) or a check result (check_done: a model's judgement that no
//     gap is left - NOT proof from the environment that the task succeeded)
//   - what the person answered (and every refusal outcome)
//   - effects the gate refused (do not retry them)
//   - files the run produced (run_command's changed_files, the gate's files fields)
// The store is saved in the session with pi.appendEntry after every change and restored from the branch on
// session_start, so a compaction (which only changes what the model sees) never loses it.

export const ENTRY_TYPE = "gui-pi-facts";
const CAP = 40;

export type FactKind = "agent_note" | "tool_evidence" | "check";
export type Fact = { text: string; source: string; confirmed: boolean; at: string; kind?: FactKind };

/** Where a user message reached the extension. */
export type UserSource =
  | "initial"       // GUI_PI_TASK: the request the user typed to start the task
  | "task_file"     // the shell's trusted task file (gui-pi-task/1), read only
  | "input"         // Pi's input event (a prompt / steer / follow-up sent over RPC)
  | "session"       // a user message found in the session (message_end, or the compaction fallback)
  | string;

export type UserMsg = { v: number; source: UserSource; text: string; at: string; task_v?: number };

export type FactsData = {
  /** the first user message (kept for older readers; the authority is `messages`) */
  request: string;
  messages: UserMsg[];
  facts: Fact[];
  answers: string[];
  refused: string[];
  files: string[];
};

/** Rendering budget of the user messages in a compaction summary (the stored record is never cut). */
export const RENDER_TOTAL = 24_000;
export const RENDER_EACH = 2_000;

function pushCapped<T>(arr: T[], v: T, same: (a: T) => boolean): boolean {
  if (arr.some(same)) return false;
  arr.push(v);
  if (arr.length > CAP) arr.shift();
  return true;
}

const norm = (s: string) => String(s || "").replace(/\r\n/g, "\n").trim();

/** For display only (timeline, summaries): never use the result as the record. */
export function shorten(text: string, max: number): string {
  const t = String(text || "");
  return t.length <= max ? t : t.slice(0, max) + ` ... [shortened for display: ${t.length - max} more characters are kept in the task record]`;
}

/** The shell's trusted task file (gui-pi-task/1): "--- user <i> <time> initial request|later message" blocks. */
export function parseTaskFile(body: string): { v: number; text: string; at: string }[] {
  const out: { v: number; text: string; at: string }[] = [];
  const lines = String(body || "").replace(/\r\n/g, "\n").split("\n");
  if (!/^gui-pi-task\/1\b/.test(lines[0] || "")) return out;
  let cur: { v: number; text: string[]; at: string } | null = null;
  for (const l of lines.slice(1)) {
    const m = l.match(/^--- user (\d+) (\S+)(?: .*)?$/);
    if (m) {
      if (cur) out.push({ v: cur.v, text: cur.text.join("\n"), at: cur.at });
      cur = { v: Number(m[1]), text: [], at: m[2] };
      continue;
    }
    if (cur) cur.text.push(l);
  }
  if (cur) out.push({ v: cur.v, text: cur.text.join("\n"), at: cur.at });
  return out.map((x) => ({ ...x, text: x.text.replace(/\n+$/, "") })).filter((x) => norm(x.text));
}

export class TaskFacts {
  messages: UserMsg[] = [];
  facts: Fact[] = [];
  answers: string[] = [];
  refused: string[] = [];
  files: string[] = [];

  /** The first thing the user asked (the authority is the whole message list). */
  get request(): string {
    return this.messages[0]?.text ?? "";
  }

  /** Record one user message, verbatim and in full. The same text twice (the env and the session both carry the
   *  first request) is one message. Returns true when it is new. */
  addUserMessage(text: string, source: UserSource, at = new Date().toISOString(), taskV?: number): boolean {
    const t = String(text ?? "");
    if (!norm(t)) return false;
    const n = norm(t);
    const ex = this.messages.find((m) => norm(m.text) === n);
    if (ex) {
      if (taskV !== undefined && ex.task_v === undefined) ex.task_v = taskV;
      return false;
    }
    const m: UserMsg = { v: this.messages.length + 1, source, text: t, at };
    if (taskV !== undefined) m.task_v = taskV;
    this.messages.push(m);
    return true;
  }

  /** Older callers: the first request (only when nothing is recorded yet; later messages use addUserMessage). */
  setRequest(r: string): boolean {
    if (!r || this.messages.length) return false;
    return this.addUserMessage(r, "initial");
  }

  /** Every user message of the shell's task file that is not recorded yet. */
  syncTaskFile(body: string): boolean {
    let changed = false;
    for (const b of parseTaskFile(body)) changed = this.addUserMessage(b.text, "task_file", b.at, b.v) || changed;
    return changed;
  }

  addFact(text: string, source: string, confirmed: boolean, at = new Date().toISOString(), kind?: FactKind): boolean {
    const t = String(text || "").trim().slice(0, 500);
    if (!t) return false;
    const k: FactKind = kind ?? (confirmed ? "tool_evidence" : "agent_note");
    const existing = this.facts.find((f) => f.text === t);
    if (existing) {
      if (confirmed && !existing.confirmed) {
        existing.confirmed = true;
        existing.kind = k;
        existing.source = String(source || "").slice(0, 200);
        return true;
      }
      return false;
    }
    return pushCapped(this.facts, { text: t, source: String(source || "?").slice(0, 200), confirmed, at, kind: k }, () => false);
  }

  addAnswer(a: string): boolean {
    const t = String(a || "").trim().slice(0, 300);
    return !!t && pushCapped(this.answers, t, (x) => x === t);
  }

  addRefused(e: string): boolean {
    const t = String(e || "").trim().slice(0, 200);
    return !!t && pushCapped(this.refused, t, (x) => x === t);
  }

  addFile(f: string): boolean {
    const t = String(f || "").trim().slice(0, 300);
    return !!t && pushCapped(this.files, t, (x) => x === t);
  }

  toJSON(): FactsData {
    return { request: this.request, messages: this.messages, facts: this.facts, answers: this.answers, refused: this.refused, files: this.files };
  }

  load(d: Partial<FactsData> | null | undefined) {
    if (!d || typeof d !== "object") return;
    this.messages = Array.isArray(d.messages)
      ? d.messages.filter((m: any) => m && typeof m.text === "string").map((m: any, i: number) => ({ ...m, v: Number(m.v) || i + 1, source: String(m.source || "session"), at: String(m.at || "") }))
      : [];
    // 0.3.1 and older stored only the first request (capped at 4000 characters)
    if (!this.messages.length && typeof d.request === "string" && d.request) this.messages = [{ v: 1, source: "initial", text: d.request, at: "" }];
    this.facts = Array.isArray(d.facts) ? d.facts.filter((f: any) => f && typeof f.text === "string")
      .map((f: any) => ({ ...f, kind: f.kind ?? (f.source === "check_done (gate)" ? "check" : f.confirmed ? "tool_evidence" : "agent_note") })) : [];
    this.answers = Array.isArray(d.answers) ? d.answers.map(String) : [];
    this.refused = Array.isArray(d.refused) ? d.refused.map(String) : [];
    this.files = Array.isArray(d.files) ? d.files.map(String) : [];
  }

  /** Restore from the newest facts entry on the current branch (Pi's ctx.sessionManager.getBranch()). */
  restore(branch: any[]) {
    for (let i = (branch || []).length - 1; i >= 0; i--) {
      const e = branch[i];
      if (e?.type === "custom" && e.customType === ENTRY_TYPE) {
        this.load(e.data);
        return true;
      }
    }
    return false;
  }

  /** The user's messages for a summary: in full while they fit RENDER_TOTAL; past that the first and the newest
   *  three stay in full and the ones in between are shortened (and say so). The record itself is never cut. */
  renderMessages(total = RENDER_TOTAL, each = RENDER_EACH): string {
    if (!this.messages.length) return "User's request (verbatim): (see the first message)";
    const all = this.messages.reduce((n, m) => n + m.text.length, 0);
    const full = (i: number) => all <= total || i === 0 || i >= this.messages.length - 3;
    const head = this.messages.length === 1
      ? "User's request (verbatim):"
      : `What the user wrote, oldest first (${this.messages.length} messages; a later message corrects or adds to the earlier ones):`;
    const body = this.messages.map((m, i) => {
      const src = `${m.source}${m.task_v !== undefined ? `, task version ${m.task_v}` : ""}`;
      const t = full(i) ? m.text : shorten(m.text, each);
      return this.messages.length === 1 ? t : `[${m.v}] (${src}) ${t}`;
    }).join("\n");
    return `${head}\n${body}`;
  }

  /** Verbatim block for the compaction summary. */
  render(): string {
    const parts: string[] = [];
    parts.push(this.renderMessages());
    const of = (k: FactKind) => this.facts.filter((f) => (f.kind ?? (f.confirmed ? "tool_evidence" : "agent_note")) === k);
    const ev = of("tool_evidence");
    const notes = of("agent_note");
    const checks = of("check");
    if (ev.length) parts.push("Observed by a tool (evidence from the environment):\n" + ev.map((f) => `- ${f.text} - ${f.source}`).join("\n"));
    if (notes.length) parts.push("Noted by the agent (its own notes; not verified by a tool):\n" + notes.map((f) => `- ${f.text} - ${f.source}`).join("\n"));
    if (checks.length) parts.push("Checks (a model's judgement, not proof from the environment that the task succeeded):\n" + checks.map((f) => `- ${f.text} - ${f.source}`).join("\n"));
    if (this.answers.length) parts.push("What the person said or did:\n- " + this.answers.join("\n- "));
    if (this.refused.length) parts.push("Refused (do not retry the same effect):\n- " + this.refused.join("\n- "));
    if (this.files.length) parts.push("Files this run produced or changed:\n- " + this.files.join("\n- "));
    return parts.join("\n\n");
  }
}

/** Files a tool result says were produced (current gate: run_command structuredContent.changed_files). */
export function filesOf(sc: any): string[] {
  const out: string[] = [];
  for (const k of ["changed_files", "files_written", "files_produced", "files"]) {
    const v = sc?.[k];
    if (Array.isArray(v)) for (const f of v) if (typeof f === "string" || typeof f?.path === "string") out.push(typeof f === "string" ? f : f.path);
  }
  return out;
}
