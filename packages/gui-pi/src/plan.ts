// A read-only copy of the plan the agent gave the gate with set_plan (the gate owns the real one), and when to
// remind the model of it. Reminders come only at milestones - a plan step done or failed, right after a refusal,
// or after STALL_STEPS actions without progress - never on every result (a mechanical line on every result can
// lock in a wrong route).
export const STALL_STEPS = 8;

export class PlanTracker {
  feasible: boolean | null = null;
  milestones: { text: string; done: boolean; failed: boolean }[] = [];

  update(args: any) {
    if (typeof args?.feasible === "boolean") this.feasible = args.feasible;
    const ms = args?.milestones ?? args?.steps ?? args?.plan;
    if (Array.isArray(ms)) {
      this.milestones = ms.map((m: any) => {
        if (typeof m === "string") return { text: m, done: false, failed: false };
        return { text: String(m?.text ?? m?.milestone ?? m?.step ?? ""), done: !!(m?.done ?? m?.status === "done"), failed: m?.status === "failed" || !!m?.failed };
      }).filter((m) => m.text);
    }
    const mark = (idx: any, key: "done" | "failed") => {
      if (Array.isArray(idx)) for (const n of idx) if (this.milestones[n - 1]) this.milestones[n - 1][key] = true; // 1-based, like the gate
    };
    mark(args?.done, "done");
    mark(args?.failed, "failed");
  }

  counts() {
    return { done: this.milestones.filter((m) => m.done).length, failed: this.milestones.filter((m) => m.failed && !m.done).length };
  }

  line(): string | null {
    if (!this.milestones.length) return null;
    const k = this.milestones.filter((m) => m.done).length;
    const next = this.milestones.find((m) => !m.done);
    return `[client: plan ${k}/${this.milestones.length} done${next ? `; next: ${next.text.slice(0, 120)}` : "; all milestones done - check_done, then answer"}]`;
  }

  summary(): string {
    if (!this.milestones.length) return this.feasible === false ? "Plan: judged NOT feasible." : "";
    return "Plan:\n" + this.milestones.map((m, i) => `${i + 1}. [${m.done ? "x" : m.failed ? "!" : " "}] ${m.text}`).join("\n");
  }
}

export type StepEvent = {
  tool: string;
  /** a GUI action (not a read, not set_plan) */
  action: boolean;
  refused: boolean;
  /** milestone counts before / after this result (set_plan) */
  before?: { done: number; failed: number };
  after?: { done: number; failed: number };
};

/** Decides when a plan reminder goes on a result. */
export class Reminder {
  sinceProgress = 0;
  plan: PlanTracker;
  stall: number;
  constructor(plan: PlanTracker, stall = STALL_STEPS) {
    this.plan = plan;
    this.stall = stall;
  }

  onResult(e: StepEvent): string | null {
    const line = this.plan.line();
    if (e.before && e.after) {
      if (e.after.done > e.before.done) {
        this.sinceProgress = 0;
        return line ? `${line} (milestone reached)` : null;
      }
      if (e.after.failed > e.before.failed) {
        this.sinceProgress = 0;
        return line ? `${line} (a milestone failed: change the approach or report why it cannot be done)` : null;
      }
      return null;
    }
    if (e.refused) {
      this.sinceProgress = 0;
      return line ? `${line} (after a refusal: take the safe way, or stop and report)` : null;
    }
    if (e.action) {
      this.sinceProgress++;
      if (this.sinceProgress >= this.stall) {
        this.sinceProgress = 0;
        return line
          ? `${line} (${this.stall} actions since the last milestone: check you are still on the plan; if stuck, change the approach)`
          : `[client: ${this.stall} actions without a plan milestone: if this is a multi-step task, set_plan; if stuck, change the approach]`;
      }
    }
    return null;
  }
}
