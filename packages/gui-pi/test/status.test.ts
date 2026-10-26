import { test } from "node:test";
import assert from "node:assert/strict";
import { interpret, humanOutcome } from "../src/status.ts";
import { UnknownGuard, HIDDEN_GATE_TOOLS } from "../index.ts";

const R = (sc: any, text = "ok", isError = false) => ({ content: [{ type: "text", text }], structuredContent: sc, isError });

test("unknown: the model is told to re-read, nothing is retried", () => {
  const i = interpret("click", {}, R({ status: "unknown" }, "[gate] driver timed out", true));
  assert.equal(i.executed, "unknown");
  assert.match(i.notes.join(), /RESULT UNKNOWN.*Do not resend it directly.*get_window_state/);
});

test("partial: how many batch steps ran (current gate batch.executed, newer names too)", () => {
  const steps = [{}, {}, {}, {}];
  let i = interpret("run_actions", { steps }, R({ status: "partial", batch: { executed: 2, stopped: "risk" } }));
  assert.deepEqual(i.steps, { ran: 2, total: 4 });
  assert.match(i.notes[0], /ran 2 of 4 steps \(stopped: risk\).*after step 2 did NOT run/);
  i = interpret("run_actions", {}, R({ status: "partial", completed_steps: 1, steps_total: 3 }));
  assert.deepEqual(i.steps, { ran: 1, total: 3 });
});

test("person outcomes: every reason is a refusal; old text still understood", () => {
  for (const [v, want] of [["decline", "declined"], ["cancel", "cancelled"], ["timeout", "timed_out"], ["closed", "closed"],
                           ["unavailable", "unavailable"], [{ outcome: "timeout", kind: "approve" }, "timed_out"]] as const) {
    const i = interpret("click", {}, R({ status: "blocked", human_outcome: v }, "[gate] Not executed", true));
    assert.equal(i.human, want);
    assert.equal(i.refused, true);
    assert.match(i.notes.join(), /step was NOT done/);
    if (want === "declined") assert.match(i.notes.join(), /That is a no/);
    else assert.match(i.notes.join(), /No answer is not a refusal by the person/);
  }
  assert.equal(humanOutcome({}, "[gate] Not executed: the person did not answer within 120 s"), "timed_out");
  assert.equal(humanOutcome({ human_outcome: "accept" }, ""), null);
});

test("input_required as a final status: nothing ran", () => {
  const i = interpret("click", {}, R({ status: "input_required" }, "needs a person", true));
  assert.equal(i.executed, false);
  assert.match(i.notes[0], /needs a person/);
});

test("old gate shapes: no status falls back to isError", () => {
  assert.equal(interpret("click", {}, R(undefined, "Clicked")).status, "executed");
  assert.equal(interpret("click", {}, R(undefined, "[gate] Not executed: risky; reviewer said no", true)).refused, true);
});

test("after an unknown outcome GUI actions are held until a window read", () => {
  const g = new UnknownGuard();
  const G = new Set(["click", "get_window_state", "type_text"]);
  g.onResult("click", "unknown", true);
  assert.match(g.check("type_text", G)!, /unknown/);
  assert.equal(g.check("get_window_state", G), null);
  assert.equal(g.check("note_fact", G), null);
  g.onResult("get_window_state", "done", false);
  assert.equal(g.check("click", G), null);
});

test("authorisation tools of an old gate are never registered", () => {
  for (const t of ["gate_confirm", "gate_task", "gate_config"]) assert.ok(HIDDEN_GATE_TOOLS.has(t));
});

test("gate v0.10.0 human_outcome words map to the shown reason", () => {
  const m: Record<string, string> = { declined: "declined", denied: "declined", cancelled: "cancelled", timeout: "timed_out",
    no_response: "timed_out", window_closed: "closed", channel_unavailable: "unavailable", unattended: "unavailable" };
  for (const [k, v] of Object.entries(m)) assert.equal(humanOutcome({ human_outcome: k }, ""), v, k);
  assert.equal(humanOutcome({ human_outcome: "accepted" }, ""), null);
});
