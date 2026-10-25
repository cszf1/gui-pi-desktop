// A read-only copy of the plan the agent gave the gate with set_plan (the gate owns the real one).
export class PlanTracker {
  feasible: boolean | null = null;
  milestones: { text: string; done: boolean }[] = [];

  update(args: any) {
    if (typeof args?.feasible === "boolean") this.feasible = args.feasible;
    const ms = args?.milestones ?? args?.steps ?? args?.plan;
    if (Array.isArray(ms)) {
      this.milestones = ms.map((m: any) => {
        if (typeof m === "string") return { text: m, done: false };
        return { text: String(m?.text ?? m?.milestone ?? m?.step ?? ""), done: !!(m?.done ?? m?.status === "done") };
      }).filter((m) => m.text);
    }
    const doneIdx = args?.done;
    if (Array.isArray(doneIdx)) for (const n of doneIdx) if (this.milestones[n - 1]) this.milestones[n - 1].done = true; // 1-based, like the gate
  }

  line(): string | null {
    if (!this.milestones.length) return null;
    const k = this.milestones.filter((m) => m.done).length;
    const next = this.milestones.find((m) => !m.done);
    return `[client: plan ${k}/${this.milestones.length} done${next ? `; next: ${next.text.slice(0, 120)}` : "; all milestones done - check_done, then answer"}]`;
  }

  summary(): string {
    if (!this.milestones.length) return this.feasible === false ? "Plan: judged NOT feasible." : "";
    return "Plan:\n" + this.milestones.map((m, i) => `${i + 1}. [${m.done ? "x" : " "}] ${m.text}`).join("\n");
  }
}
