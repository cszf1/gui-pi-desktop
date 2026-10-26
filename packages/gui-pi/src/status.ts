// Reading the gate's structured result (structuredContent) into what the model must be told.
// Works with the current gate (status executed | blocked | partial | done, batch.executed) and the newer one
// (status unknown | input_required, human_outcome, diff) and gate v0.10.2 (human_choice, human_outcome task_changed /
// late / void / not_asked_again / expired, tier write_args and freeze; the reviewer's words are never returned). The status decides, never the "[gate]" text prefix.
// Nothing here retries anything: an unknown outcome is reported, and the model must re-read before acting.
//
// gate v0.10.3 (final field names, gui-gate tag v0.10.3; every field optional - when one is missing the 0.3.1
// behaviour holds, and the earlier client-chosen aliases are still accepted): one execution state on every tool result
//   status            done | partial | unknown | input_required | done_by_person | blocked  (older: executed)
//   executed          true | false | "unknown" (action results); the client never reads more into it than status says
//   continuation_id   the gate's step id for this result (aliases: resume_id, continue_id, state.*)
//   completed_steps   0-based indices of the run_actions steps that ran (aliases: completed, completed_indices,
//                     batch.completed, state.completed; a NUMBER there is the older count); with it the client says
//                     exactly which steps did NOT run
//   next_step         0-based index of the first step that did not run (partial) or whose outcome is unknown (unknown)
//   postcondition     {status: verified | unverifiable | reread | none, evidence, reread} (progress.ts reads it; the
//                     older list form [{what, met, detail}] too); only verified / met:true counts as evidence
//   loop_state        normal | switch_strategy | blocked_repeat | help_or_stop (the loop breaker's level, advice)
//   reread            after a takeover / "I did it": {window, focused_window, focused_title, files_changed, note}
//   gate.tier premise           what the person approved (recipient, amount, text, file versions) changed before the
//                               send: the approval is void, nothing was sent, a re-send asks the person again
//   gate.decision unknown_hold  an earlier send of this non-idempotent step has an unknown outcome and no person can
//                               confirm it did not happen: never resent; report it as possibly done
//   gate.decision review_unavailable  the step needed an independent review that did not answer and no person was
//                               reachable: nothing was sent
// (person_request / person_request_end are gate LOG events - one record and one end per question; the shell's
// human.jsonl is the client-side counterpart; neither is part of a tool result.)

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
  /** gate v0.10.3: why the gate held the step in its ledger / premise check (null: none of these) */
  hold: "premise" | "unknown_hold" | "review_unavailable" | null;
  /** gate v0.10.3 loop_state (null: not given / normal) */
  loopState: string | null;
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
  const raw = (Array.isArray(sc?.completed_steps) ? sc.completed_steps : null) ?? sc?.completed ?? sc?.completed_indices ??
    sc?.batch?.completed ?? sc?.batch?.completed_indices ?? sc?.state?.completed ?? null;
  if (!Array.isArray(raw)) return null;
  const base = Number(sc?.index_base ?? sc?.batch?.index_base ?? 0) === 1 ? 1 : 0;
  const out = raw.map((x: any) => Number(typeof x === "object" && x ? x.index ?? x.i : x)).filter((n: number) => Number.isInteger(n) && n - base >= 0).map((n: number) => n - base);
  return [...new Set(out)].sort((a, b) => a - b);
}

/** gate v0.10.3 next_step: 0-based index of the first step that did not run / whose outcome is unknown (null: none). */
export function nextStepOf(sc: any): number | null {
  const v = sc?.next_step ?? sc?.state?.next_step ?? null;
  const n = typeof v === "number" ? v : typeof v === "string" && v.trim() !== "" ? Number(v) : NaN;
  return Number.isInteger(n) && n >= 0 ? n : null;
}

/** gate v0.10.3 holds from the unknown ledger / premise check / review, from the structured gate result. */
export function holdOf(sc: any): Interp["hold"] {
  const g = sc?.gate || {};
  if (g.tier === "premise" || g.reason === "premise_changed" || g.premise_changed) return "premise";
  if (g.decision === "unknown_hold" || g.tier === "unknown_hold") return "unknown_hold";
  if (g.decision === "review_unavailable" || g.tier === "review_unavailable") return "review_unavailable";
  return null;
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
  // the status decides; the gate's own `executed` (v0.10.3) may only make it more cautious, never claim a run the
  // status does not
  let executed: boolean | "unknown" =
    status === "blocked" || status === "input_required" || status === "done_by_person" ? false : status === "unknown" ? "unknown" : true;
  if (executed === true && (sc.executed === false || sc.executed === "unknown")) executed = sc.executed;
  const hold = holdOf(sc);
  const nextIdx = nextStepOf(sc);
  const loopState = typeof sc.loop_state === "string" && sc.loop_state !== "normal" ? sc.loop_state : null;
  let steps: Interp["steps"] = null;
  const continuation = continuationOf(sc);
  const completed = completedOf(sc);

  if (status === "unknown") {
    notes.push("[client: RESULT UNKNOWN - this step may or may not have happened. Do not resend it directly (not even reworded). " +
      "Read the window first (get_window_state) and decide from what it shows; actions are held until you do." +
      (completed ? ` Batch steps known to have run: ${completed.map((i) => i + 1).join(", ") || "none"}; ` +
        (nextIdx !== null ? `step ${nextIdx + 1} is the one whose outcome is unknown; later steps did not run.` : "the others are unknown.") : "") + "]");
  } else if (status === "partial") {
    const b = sc.batch || {};
    const total = num(b.total) ?? num(sc.steps_total) ?? num(sc.total_steps) ?? (Array.isArray(params?.steps) ? params.steps.length : null);
    const ran = completed ? completed.length : num(b.executed) ?? (Array.isArray(sc.completed_steps) ? null : num(sc.completed_steps)) ?? num(sc.steps_completed) ?? 0;
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
  if (hold === "premise") {
    notes.push("[client: WHAT THE PERSON APPROVED HAS CHANGED (recipient, amount, text, a field or a file version differs from " +
      "what they saw), so that approval is void and nothing was sent. Re-read the window; if the step is still right as it is " +
      "now, send it again - the person is asked again with the current content. Never change the content back just to match the approval.]");
  } else if (hold === "unknown_hold") {
    notes.push("[client: HELD - an earlier send of this step has an UNKNOWN outcome and it is an effect that must not happen twice " +
      "(send / pay / delete / submit ...). A window read cannot prove it did not happen and no person can confirm it, so it is not sent " +
      "again. Do not try it again in any form; tell the user it may already have happened and let them check.]");
  } else if (hold === "review_unavailable") {
    notes.push("[client: this step needed an independent review; the reviewer did not answer and no person could be asked, so " +
      "NOTHING was sent. It was not approved and not refused. Re-read the window, check the step yourself, prefer a safer way; " +
      "sending it again triggers the review again.]");
  }
  if (loopState === "switch_strategy" || loopState === "blocked_repeat") {
    notes.push(`[client: the gate's loop state is ${loopState}: the same approach keeps failing. Change the method (another element, ` +
      "menu path, keyboard shortcut or tool), do not repeat it.]");
  } else if (loopState === "help_or_stop") {
    notes.push("[client: the gate's loop state is help_or_stop: repeated attempts failed. Ask the person for help or stop and report what blocks you.]");
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
    && (["deny", "ask_human", "handoff", "stop_loop", "ask_help", "unknown_hold"].includes(String(g.decision)) || ["review", "write_args"].includes(String(g.tier)))
    // premise / review_unavailable: the step was neither approved nor refused - it is asked / reviewed again
    && hold !== "premise" && hold !== "review_unavailable";
  const refused = (status === "blocked" || status === "input_required") && !waitingPerson && choice !== "another_way" &&
    (!!human || !!gateRefusal || (!g && /refus|declin|risk|denied/i.test(text)));
  if (continuation && (status === "partial" || status === "unknown" || status === "input_required" || status === "blocked"))
    notes.push(`[client: the gate's continuation id for this step is ${continuation}; it names this decision, it authorises nothing.]`);
  return { status: waitingPerson ? "waiting_person" : status, nextAction, byPerson, executed, human, choice, waitingPerson, refused, steps, continuation,
           hold, loopState, notes };
}

/** A short description of a refused effect, for the task facts (structured when the gate gives one). */
export function refusedEffect(name: string, sc: any, text: string): string {
  const e = sc?.effect ?? sc?.gate?.effect;
  if (e) return (typeof e === "string" ? e : JSON.stringify(e)).slice(0, 200);
  return `${name}: ${text.split("\n")[0].slice(0, 180)}`;
}
