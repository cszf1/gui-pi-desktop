// Reading the gate's structured result (structuredContent) into what the model must be told.
// Works with the current gate (status executed | blocked | partial | done, batch.executed) and the newer one
// (status unknown | input_required, human_outcome, diff) and gate v0.10.2 (human_choice, human_outcome task_changed /
// late / void / not_asked_again / expired, tier write_args and freeze; the reviewer's words are never returned). The status decides, never the "[gate]" text prefix.
// Nothing here retries anything: an unknown outcome is reported, and the model must re-read before acting.
//
// gate v0.10.3 (expected; parsed forward-compatibly, every field optional - when one is missing the 0.3.1 behaviour
// holds): a unified execution state on every tool result
//   status            done | partial | unknown | input_required | done_by_person | blocked  (older: executed)
//   continuation_id   id of the decision / batch the next call continues (aliases: resume_id, continue_id, state.*)
//   completed         indices of the run_actions steps that ran (0-based; aliases: completed_indices, batch.completed,
//                     state.completed); with it the client says exactly which steps did NOT run
//   postconditions    [{what, met: true|false|null, detail}] evidence the gate checked after the step (aliases:
//                     postcondition, evidence, state.postconditions); only met:true counts as evidence

export type HumanOutcome =
  | "declined" | "cancelled" | "timed_out" | "closed" | "unavailable"
  // gate v0.10.2
  | "expired" | "late" | "void" | "task_changed" | "answer_rejected" | "not_asked_again";

export const HUMAN_LABEL: Record<HumanOutcome, string> = {
  declined: "declined",
  cancelled: "cancelled the question",
  timed_out: "did not answer in time",
  closed: "closed the question window",
  unavailable: "could not be reached (no person available)",
  expired: "never saw the question (it expired while another question was open)",
  late: "answered too late (after the question had timed out)",
  void: "was asked a question that was voided (the agent was reset); their answer approves nothing",
  task_changed: "answered about an older version of the task (you wrote again meanwhile); that answer approves nothing now",
  answer_rejected: "answered a question that was no longer open (voided or timed out); the answer approves nothing",
  not_asked_again: "was already asked about this exact step without answering; they are not asked again in this task",
};

/** Outcomes where the person said no. Everything else in HumanOutcome is "no (usable) answer": not a refusal by the
 *  person, but the step was not done either. */
export const PERSON_SAID_NO = new Set<HumanOutcome>(["declined"]);

/** Tools that only read: allowed after an unknown outcome (they are how the model re-checks). */
export const READ_TOOLS = new Set(["get_window_state", "list_windows", "list_apps", "inspect", "check_done", "set_plan", "gate_report"]);

/** The gate's ask_help result: what the person chose (gate v0.10.2 `human_choice`). */
export type HumanChoice = "another_way" | "did_it" | null;

export type Interp = {
  status: string;
  /** the gate's next_action (e.g. "rethink"), when it gives one */
  nextAction: string | null;
  /** a person did this step themselves (status done_by_person, or the older gate.done_by_person flag) */
  byPerson: boolean;
  /** true: it ran; false: nothing ran; "unknown": may or may not have run */
  executed: boolean | "unknown";
  human: HumanOutcome | null;
  /** ask_help: the person asked for another way (not a refusal) or did the step */
  choice: HumanChoice;
  /** a person is answering a gate question right now (gate freeze): nothing was sent, wait for them */
  waitingPerson: boolean;
  refused: boolean;
  steps: { ran: number; total: number | null; completed?: number[]; notRun?: number[] } | null;
  /** gate v0.10.3: the id the next call continues from (null: not given) */
  continuation: string | null;
  /** client notes appended to the result for the model */
  notes: string[];
};

/** gate v0.10.3 continuation id, read tolerantly (null when the gate gives none). */
export function continuationOf(sc: any): string | null {
  const v = sc?.continuation_id ?? sc?.resume_id ?? sc?.continue_id ?? sc?.state?.continuation_id ?? sc?.state?.resume_id ?? null;
  return v === null || v === undefined || v === "" ? null : String(v).slice(0, 120);
}

/** gate v0.10.3 indices of the batch steps that ran (0-based), or null when the gate does not say. */
export function completedOf(sc: any): number[] | null {
  const raw = sc?.completed ?? sc?.completed_indices ?? sc?.batch?.completed ?? sc?.batch?.completed_indices ?? sc?.state?.completed ?? null;
  if (!Array.isArray(raw)) return null;
  const base = Number(sc?.index_base ?? sc?.batch?.index_base ?? 0) === 1 ? 1 : 0;
  const out = raw.map((x: any) => Number(typeof x === "object" && x ? x.index ?? x.i : x)).filter((n: number) => Number.isInteger(n) && n - base >= 0).map((n: number) => n - base);
  return [...new Set(out)].sort((a, b) => a - b);
}

function normHuman(v: any, voided = false): HumanOutcome | null {
  if (v === null || v === undefined || v === false) return null;
  if (typeof v === "object" && v.voided === true) voided = true;
  const late = typeof v === "object" && v.late === true;
  const s = String(typeof v === "object" ? v.outcome ?? v.reason ?? v.action ?? v.status ?? "" : v).toLowerCase();
  if (!s || s === "accept" || s === "accepted" || s === "allow" || s === "done") return null;
  if (s === "task_changed") return "task_changed";
  if (s === "void" || s === "voided") return "void";
  if (s === "answer_rejected" || s === "rejected") return "answer_rejected";
  if (s === "not_asked_again") return "not_asked_again";
  if (s === "late") return "late";
  if (s === "expired") return "expired";
  if (s.startsWith("declin") || s === "deny" || s === "denied" || s === "no") return "declined";
  if (s.startsWith("cancel")) return "cancelled";
  if (s.startsWith("time") || s === "no_response") return voided ? "void" : late ? "late" : "timed_out";
  if (s.includes("clos")) return "closed"; // gate: window_closed
  if (s.includes("unavail") || s === "unattended" || s === "none" || s === "nobody") return "unavailable"; // gate: channel_unavailable, unattended
  return "declined"; // anything unknown fails closed
}

/** The person's outcome: structuredContent.human_outcome (new gate), else the legacy text. */
export function humanOutcome(sc: any, text: string): HumanOutcome | null {
  const h = normHuman(sc?.human_outcome, sc?.voided === true || sc?.gate?.voided === true);
  if (h) return h;
  if (/did not answer within|no answer in time/i.test(text)) return "timed_out";
  if (/the person declined|person said no/i.test(text)) return "declined";
  return null;
}

export function humanChoice(sc: any): HumanChoice {
  const c = String(sc?.human_choice ?? "").toLowerCase();
  if (c === "another_way" || c === "try_other" || c === "hint") return "another_way";
  if (c === "did_it") return "did_it";
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
  const choice = humanChoice(sc);
  // gate freeze: a person is answering a gate question (also after a reset, until they answer or it times out)
  const waitingPerson = sc?.gate?.tier === "freeze";
  const notes: string[] = [];
  // done_by_person: the AGENT's step did not run (a person did the work); it must never be sent again
  const executed: boolean | "unknown" =
    status === "blocked" || status === "input_required" || status === "done_by_person" ? false : status === "unknown" ? "unknown" : true;
  let steps: Interp["steps"] = null;
  const continuation = continuationOf(sc);
  const completed = completedOf(sc);

  if (status === "unknown") {
    notes.push("[client: RESULT UNKNOWN - this step may or may not have happened. Do not resend it directly (not even reworded). " +
      "Read the window first (get_window_state) and decide from what it shows; actions are held until you do." +
      (completed ? ` Batch steps known to have run: ${completed.map((i) => i + 1).join(", ") || "none"}; the others are unknown.` : "") + "]");
  } else if (status === "partial") {
    const b = sc.batch || {};
    const total = num(b.total) ?? num(sc.steps_total) ?? num(sc.total_steps) ?? (Array.isArray(params?.steps) ? params.steps.length : null);
    const ran = completed ? completed.length : num(b.executed) ?? num(sc.completed_steps) ?? num(sc.steps_completed) ?? 0;
    const notRun = completed && total !== null ? Array.from({ length: total }, (_, i) => i).filter((i) => !completed.includes(i)) : undefined;
    steps = completed ? { ran, total, completed, ...(notRun ? { notRun } : {}) } : { ran, total };
    const why = b.stopped ? ` (stopped: ${String(typeof b.stopped === "object" ? JSON.stringify(b.stopped) : b.stopped).slice(0, 160)})` : "";
    const which = completed
      ? `Steps that ran: ${completed.map((i) => i + 1).join(", ") || "none"}${notRun ? `; steps that did NOT run: ${notRun.map((i) => i + 1).join(", ") || "none"}` : ""} (1-based).`
      : `Steps after step ${ran} did NOT run.`;
    notes.push(`[client: run_actions ran ${ran}${total !== null ? ` of ${total}` : ""} steps${why}. ` +
      `${which} Continue from the window shown, do not resend the steps that ran.]`);
  } else if (status === "input_required") {
    notes.push("[client: this step needs a person and nothing was executed.]");
  } else if (status === "done_by_person") {
    notes.push("[client: A PERSON DID THIS STEP themselves. Do not send it again and do not replay it another way (it is held if you do). " +
      "They may have changed more than this step (another window, a file, the focus): re-read the affected state first - the client " +
      "reads the window again before your next turn - and continue from what it shows.]");
  }
  if (status === "blocked" && nextAction === "rethink") {
    notes.push("[client: the gate asks you to RETHINK: nothing was executed. Do not resend this step or a reworded copy of it. " +
      "Look at the current window, reconsider what the user asked, and generate a new next action.]");
  }
  if (waitingPerson) {
    notes.push("[client: WAITING FOR THE PERSON: they are answering a question from the gate right now. Nothing was sent. " +
      "Do not resend this or any other action; wait for their answer, then read the window.]");
  }
  if (sc?.gate?.tier === "write_args") {
    notes.push("[client: a read tool cannot write files. Nothing was sent. Call it again WITHOUT the destination argument " +
      "(screenshot_out_file, output_path, save_to, ...); to save something use the app itself or run_command.]");
  }
  if (choice === "another_way") {
    notes.push("[client: the person asked you to TRY ANOTHER WAY (this is not a refusal). Do not send the same action again; " +
      "use a clearly different way (another menu path, a keyboard shortcut, another element); a hint they gave is in the gate's text above.]");
  } else if (human === "task_changed") {
    notes.push("[client: the user's task changed while the person was answering; that answer approves nothing now. " +
      "Re-read the window and the current task; send the step again only if the current task still needs it (the person is asked again).]");
  } else if (human) {
    notes.push((PERSON_SAID_NO.has(human)
      ? `[client: the person declined. That is a no: the step was NOT done. `
      : `[client: the person ${HUMAN_LABEL[human]}. No answer is not a refusal by the person, but the step was NOT done. `) +
      "Do not ask again for the same thing right away and do not try the same effect another way; take a safe way or stop and report.]");
  }
  // structured first (gate.decision / tier); the text only for old gates without a structured gate result. Never the
  // reviewer's words: the gate no longer returns them.
  const g = sc?.gate;
  const gateRefusal = g && g.executed === false
    && (["deny", "ask_human", "handoff", "stop_loop", "ask_help"].includes(String(g.decision)) || ["review", "write_args"].includes(String(g.tier)));
  const refused = (status === "blocked" || status === "input_required") && !waitingPerson && choice !== "another_way" &&
    (!!human || !!gateRefusal || (!g && /refus|declin|risk|denied/i.test(text)));
  if (continuation && (status === "partial" || status === "unknown" || status === "input_required" || status === "blocked"))
    notes.push(`[client: the gate's continuation id for this step is ${continuation}; it names this decision, it authorises nothing.]`);
  return { status: waitingPerson ? "waiting_person" : status, nextAction, byPerson, executed, human, choice, waitingPerson, refused, steps, continuation, notes };
}

/** A short description of a refused effect, for the task facts (structured when the gate gives one). */
export function refusedEffect(name: string, sc: any, text: string): string {
  const e = sc?.effect ?? sc?.gate?.effect;
  if (e) return (typeof e === "string" ? e : JSON.stringify(e)).slice(0, 200);
  return `${name}: ${text.split("\n")[0].slice(0, 180)}`;
}
