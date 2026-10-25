// Client-side holds on GUI actions (the gate stays the authority; these only stop the client from racing ahead):
//   - after an outcome-unknown result, actions wait until the model has read a window again;
//   - after a step a PERSON did (status done_by_person), the same step (tool + arguments) is never sent again in
//     this task, and actions wait for a read unless the result already carried a fresh one;
//   - after the user pressed Stop, nothing runs at all.
import { READ_TOOLS } from "./status.ts";

export function signature(tool: string, args: any): string {
  const norm = (v: any): any => {
    if (Array.isArray(v)) return v.map(norm);
    if (v && typeof v === "object") return Object.fromEntries(Object.keys(v).sort().map((k) => [k, norm(v[k])]));
    return v;
  };
  return `${tool}:${JSON.stringify(norm(args ?? {}))}`;
}

export class ActionGuard {
  /** a read is needed before the next action ("unknown" | "person") */
  pending: "" | "unknown" | "person" = "";
  /** steps a person did: never resent */
  byPerson = new Set<string>();
  stopped = false;

  onResult(tool: string, status: string, isError: boolean, args?: any, freshRead = false) {
    if (status === "unknown") this.pending = "unknown";
    else if (status === "done_by_person") {
      this.byPerson.add(signature(tool, args));
      if (!freshRead) this.pending = "person";
    } else if (tool.endsWith("get_window_state") && !isError) this.pending = "";
  }

  /** A fresh window read reached the model some other way (the next-step attachment). */
  onRead() {
    this.pending = "";
  }

  check(tool: string, gateTools: Set<string>, args?: any): string | null {
    if (this.stopped) return "Stopped by the user: nothing more runs in this task.";
    if (!gateTools.has(tool)) return null;
    if (this.byPerson.has(signature(tool, args)))
      return "Held: a person already did exactly this step. Never send it again; read the window and continue from what it shows.";
    if (!this.pending || READ_TOOLS.has(tool)) return null;
    return this.pending === "unknown"
      ? "Held: the outcome of an earlier step is unknown. Read the window first (get_window_state) and decide from what it shows; do not resend that step blind."
      : "Held: a person just did a step themselves. Read the window first (get_window_state) and continue from what it shows.";
  }
}

/** Kept for older callers and tests: the unknown-outcome part of ActionGuard. */
export class UnknownGuard extends ActionGuard {}
