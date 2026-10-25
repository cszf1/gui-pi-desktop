// Reading the gate's structured result (structuredContent) into what the model must be told.
// Works with the current gate (status executed | blocked | partial | done, batch.executed) and the newer one
// (status unknown | input_required, human_outcome, diff). The status decides, never the "[gate]" text prefix.
// Nothing here retries anything: an unknown outcome is reported, and the model must re-read before acting.

export type HumanOutcome = "declined" | "cancelled" | "timed_out" | "closed" | "unavailable";

export const HUMAN_LABEL: Record<HumanOutcome, string> = {
  declined: "declined",
  cancelled: "cancelled the question",
  timed_out: "did not answer in time",
  closed: "closed the question window",
  unavailable: "could not be reached (no person available)",
};

/** Tools that only read: allowed after an unknown outcome (they are how the model re-checks). */
export const READ_TOOLS = new Set(["get_window_state", "list_windows", "list_apps", "inspect", "check_done", "set_plan", "gate_report"]);

export type Interp = {
  status: string;
  /** the gate's next_action (e.g. "rethink"), when it gives one */
  nextAction: string | null;
  /** a person did this step themselves (status done_by_person, or the older gate.done_by_person flag) */
  byPerson: boolean;
  /** true: it ran; false: nothing ran; "unknown": may or may not have run */
  executed: boolean | "unknown";
  human: HumanOutcome | null;
  refused: boolean;
  steps: { ran: number; total: number | null } | null;
  /** client notes appended to the result for the model */
  notes: string[];
};

function normHuman(v: any): HumanOutcome | null {
  if (v === null || v === undefined || v === false) return null;
  const s = String(typeof v === "object" ? v.outcome ?? v.reason ?? v.action ?? v.status ?? "" : v).toLowerCase();
  if (!s || s === "accept" || s === "accepted" || s === "allow" || s === "done") return null;
  if (s.startsWith("declin") || s === "deny" || s === "denied" || s === "no") return "declined";
  if (s.startsWith("cancel")) return "cancelled";
  if (s.startsWith("time") || s === "expired" || s === "no_response") return "timed_out";
  if (s.includes("clos")) return "closed"; // gate: window_closed
  if (s.includes("unavail") || s === "unattended" || s === "none" || s === "nobody") return "unavailable"; // gate: channel_unavailable, unattended
  return "declined";
}

/** The person's outcome: structuredContent.human_outcome (new gate), else the legacy text. */
export function humanOutcome(sc: any, text: string): HumanOutcome | null {
  const h = normHuman(sc?.human_outcome);
  if (h) return h;
  if (/did not answer within|no answer in time/i.test(text)) return "timed_out";
  if (/the person declined|person said no/i.test(text)) return "declined";
  return null;
}

function num(v: any): number | null {
  const n = Number(v);
  return v !== null && v !== undefined && v !== "" && Number.isFinite(n) ? n : null;
}

export function interpret(name: string, params: any, r: { content?: any[]; isError?: boolean; structuredContent?: any }): Interp {
  const sc = r.structuredContent || {};
  const text = (r.content || []).filter((c: any) => c?.type === "text").map((c: any) => c.text || "").join("\n");
  const byPerson = sc.status === "done_by_person" || sc?.gate?.done_by_person === true;
  const status = byPerson ? "done_by_person" : String(sc.status || (r.isError ? "blocked" : "executed"));
  const nextAction = sc.next_action ? String(sc.next_action) : null;
  const human = humanOutcome(sc, text);
  const notes: string[] = [];
  // done_by_person: the AGENT's step did not run (a person did the work); it must never be sent again
  const executed: boolean | "unknown" =
    status === "blocked" || status === "input_required" || status === "done_by_person" ? false : status === "unknown" ? "unknown" : true;
  let steps: Interp["steps"] = null;

  if (status === "unknown") {
    notes.push("[client: OUTCOME UNKNOWN - this step may or may not have happened. Do not send it again. " +
      "Read the window first (get_window_state) and decide from what it shows; actions are held until you do.]");
  } else if (status === "partial") {
    const b = sc.batch || {};
    const ran = num(b.executed) ?? num(sc.completed_steps) ?? num(sc.steps_completed) ?? 0;
    const total = num(b.total) ?? num(sc.steps_total) ?? num(sc.total_steps) ?? (Array.isArray(params?.steps) ? params.steps.length : null);
    steps = { ran, total };
    const why = b.stopped ? ` (stopped: ${String(typeof b.stopped === "object" ? JSON.stringify(b.stopped) : b.stopped).slice(0, 160)})` : "";
    notes.push(`[client: run_actions ran ${ran}${total !== null ? ` of ${total}` : ""} steps${why}. ` +
      `Steps after step ${ran} did NOT run; continue from the window shown, do not resend the steps that ran.]`);
  } else if (status === "input_required") {
    notes.push("[client: this step needs a person and nothing was executed.]");
  } else if (status === "done_by_person") {
    notes.push("[client: A PERSON DID THIS STEP themselves. Do not send it again (it is held if you do). " +
      "Read the window as it is now (the attached read, or get_window_state) and continue from there.]");
  }
  if (status === "blocked" && nextAction === "rethink") {
    notes.push("[client: the gate asks you to RETHINK: nothing was executed. Do not resend this step or a reworded copy of it. " +
      "Look at the current window, reconsider what the user asked, and generate a new next action.]");
  }
  if (human) {
    notes.push(`[client: the person ${HUMAN_LABEL[human]}. That counts as a no: the step was refused. ` +
      "Do not ask again for the same thing right away and do not try the same effect another way; take a safe way or stop and report.]");
  }
  const refused = (status === "blocked" || status === "input_required") && (!!human || /refus|declin|reviewer|risk|denied/i.test(text));
  return { status, nextAction, byPerson, executed, human, refused, steps, notes };
}

/** A short description of a refused effect, for the task facts (structured when the gate gives one). */
export function refusedEffect(name: string, sc: any, text: string): string {
  const e = sc?.effect ?? sc?.gate?.effect;
  if (e) return (typeof e === "string" ? e : JSON.stringify(e)).slice(0, 200);
  return `${name}: ${text.split("\n")[0].slice(0, 180)}`;
}
