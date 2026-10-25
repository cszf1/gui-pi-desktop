// Task facts: what must survive compaction word for word, kept by the program (never re-summarised by a model).
//   - the user's request, verbatim
//   - facts with their source (noted by the agent with note_fact, or confirmed by a tool result such as check_done)
//   - what the person answered (and every refusal outcome)
//   - effects the gate refused (do not retry them)
//   - files the run produced (run_command's changed_files, the gate's files fields)
// The store is saved in the session with pi.appendEntry after every change and restored from the branch on
// session_start, so a compaction (which only changes what the model sees) never loses it.

export const ENTRY_TYPE = "gui-pi-facts";
const CAP = 40;

export type Fact = { text: string; source: string; confirmed: boolean; at: string };

export type FactsData = { request: string; facts: Fact[]; answers: string[]; refused: string[]; files: string[] };

function pushCapped<T>(arr: T[], v: T, same: (a: T) => boolean): boolean {
  if (arr.some(same)) return false;
  arr.push(v);
  if (arr.length > CAP) arr.shift();
  return true;
}

export class TaskFacts {
  request = "";
  facts: Fact[] = [];
  answers: string[] = [];
  refused: string[] = [];
  files: string[] = [];

  setRequest(r: string): boolean {
    if (!r || this.request) return false;
    this.request = r.slice(0, 4000);
    return true;
  }

  addFact(text: string, source: string, confirmed: boolean, at = new Date().toISOString()): boolean {
    const t = String(text || "").trim().slice(0, 500);
    if (!t) return false;
    const existing = this.facts.find((f) => f.text === t);
    if (existing) {
      if (confirmed && !existing.confirmed) {
        existing.confirmed = true;
        existing.source = String(source || "").slice(0, 200);
        return true;
      }
      return false;
    }
    return pushCapped(this.facts, { text: t, source: String(source || "?").slice(0, 200), confirmed, at }, () => false);
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
    return { request: this.request, facts: this.facts, answers: this.answers, refused: this.refused, files: this.files };
  }

  load(d: Partial<FactsData> | null | undefined) {
    if (!d || typeof d !== "object") return;
    this.request = typeof d.request === "string" ? d.request : "";
    this.facts = Array.isArray(d.facts) ? d.facts.filter((f: any) => f && typeof f.text === "string") : [];
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

  /** Verbatim block for the compaction summary. */
  render(): string {
    const parts: string[] = [];
    parts.push(`User's request (verbatim): ${this.request || "(see the first message)"}`);
    if (this.facts.length)
      parts.push("Task facts (value - source):\n" + this.facts.map((f) => `- ${f.text} - ${f.source}${f.confirmed ? " (confirmed)" : " (noted by the agent)"}`).join("\n"));
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
