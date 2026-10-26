// gate v0.10.3 (tag v0.10.3): the final structured field names, the unknown ledger / approval premise / review holds,
// and the gate's own re-read after a person's turn (the client reads only what the gate did not).
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, txt, read } from "./harness.ts";
import { interpret, completedOf, nextStepOf, holdOf } from "../src/status.ts";
import { Progress, postconditions } from "../src/progress.ts";
import { ObsTracker, rereadOf, afterPerson, PERSON_ACTED } from "../src/observe.ts";
import { ActionGuard } from "../src/guard.ts";

const turnEnd = (pi: any) => pi.emit("turn_end", { toolResults: [{}], entries: [] });

test("final names: completed_steps (array), next_step, executed, continuation_id, loop_state", () => {
  // what gate v0.10.3 set_status writes for a stopped batch
  const p = interpret("run_actions", { steps: [1, 2, 3, 4] }, { content: [], structuredContent: {
    status: "partial", executed: true, completed_steps: [0, 1], next_step: 2, continuation_id: "g1.a7", loop_state: "normal",
    batch: { executed: 2, stopped: "risk", total: 4 }, postcondition: { status: "none", evidence: null, reread: false } } });
  assert.deepEqual(p.steps, { ran: 2, total: 4, completed: [0, 1], notRun: [2, 3] });
  assert.match(p.notes.join(" "), /Steps that ran: 1, 2; steps that did NOT run: 3, 4/);
  assert.equal(p.continuation, "g1.a7");
  assert.equal(p.loopState, null, "normal is not news");
  assert.deepEqual(completedOf({ completed_steps: [2, 0] }), [0, 2]);
  assert.equal(completedOf({ completed_steps: 3 }), null, "a number is the older count, not indices");
  assert.equal(nextStepOf({ next_step: 2 }), 2);
  assert.equal(nextStepOf({ next_step: null }), null);
  // unknown batch step: which one is unknown
  const u = interpret("run_actions", {}, { isError: true, content: [], structuredContent: { status: "unknown", executed: "unknown", completed_steps: [0], next_step: 1 } });
  assert.equal(u.executed, "unknown");
  assert.match(u.notes.join(" "), /steps known to have run: 1; step 2 is the one whose outcome is unknown/);
  // executed from the gate can only make it more cautious
  assert.equal(interpret("click", {}, { content: [], structuredContent: { status: "done", executed: true } }).executed, true);
  assert.equal(interpret("click", {}, { content: [], structuredContent: { status: "blocked", executed: true } }).executed, false);
  // loop state advice
  const l = interpret("click", {}, { content: [], structuredContent: { status: "done", loop_state: "switch_strategy" } });
  assert.match(l.notes.join(" "), /loop state is switch_strategy.*Change the method/);
  const h = interpret("click", {}, { content: [], structuredContent: { status: "blocked", loop_state: "help_or_stop" } });
  assert.match(h.notes.join(" "), /help_or_stop.*Ask the person for help or stop/);
});

test("postcondition {status}: verified is evidence, unverifiable unchecked, none / reread report nothing", () => {
  assert.deepEqual(postconditions({ postcondition: { status: "verified", evidence: "verified", reread: false } }),
                   [{ what: "the step's effect", met: true, detail: "verified by the driver (verified)" }]);
  assert.equal(postconditions({ postcondition: { status: "unverifiable", evidence: "unverifiable" } })[0].met, null);
  assert.deepEqual(postconditions({ postcondition: { status: "none", evidence: null } }), []);
  assert.deepEqual(postconditions({ postcondition: { status: "reread", reread: true } }), []);
  // the older list form still reads
  assert.equal(postconditions({ postconditions: [{ what: "x", met: true }] })[0].met, true);
  // a done step with status none and a fresh read in the result is settled, without one it stays unverified
  const pr = new Progress();
  pr.onResult({ tool: "click", args: { element_token: "s1:1" }, status: "done", isError: false, refused: false, action: true, observed: false, why: "",
                sc: { postcondition: { status: "none" } } });
  assert.match(pr.unverified[0].text, /nothing observed its effect yet/);
  pr.onResult({ tool: "click", args: { element_token: "s1:1" }, status: "done", isError: false, refused: false, action: true, observed: false, why: "",
                sc: { postcondition: { status: "verified", evidence: "verified" } } });
  assert.equal(pr.unverified.length, 0);
});

test("holds: premise changed (re-ask, not a refusal), unknown_hold (never resent), review_unavailable (nothing sent)", () => {
  const prem = { status: "blocked", gate: { decision: "deny", tier: "premise", executed: false, reason: "premise_changed", premise_changed: "was: To: a | now: To: b" } };
  assert.equal(holdOf(prem), "premise");
  const ip = interpret("click", {}, { isError: true, content: [{ type: "text", text: "[gate] Not executed: the approved content changed" }], structuredContent: prem });
  assert.equal(ip.hold, "premise");
  assert.equal(ip.refused, false, "the person did not refuse; they are asked again");
  assert.match(ip.notes.join(" "), /WHAT THE PERSON APPROVED HAS CHANGED.*nothing was sent.*asked again/);
  const hold = { status: "blocked", gate: { decision: "unknown_hold", tier: "rule", executed: false } };
  const ih = interpret("click", {}, { isError: true, content: [{ type: "text", text: "[gate] Not executed: held" }], structuredContent: hold });
  assert.equal(ih.hold, "unknown_hold");
  assert.equal(ih.refused, true);
  assert.match(ih.notes.join(" "), /UNKNOWN outcome.*Do not try it again in any form.*may already have happened/);
  const rv = { status: "blocked", gate: { decision: "review_unavailable", tier: "rule", executed: false } };
  const ir = interpret("click", {}, { isError: true, content: [{ type: "text", text: "[gate] Not executed: review" }], structuredContent: rv });
  assert.equal(ir.hold, "review_unavailable");
  assert.equal(ir.refused, false);
  assert.match(ir.notes.join(" "), /NOTHING was sent.*not approved and not refused/);
  assert.equal(holdOf({ status: "done" }), null);

  // process state
  const pr = new Progress();
  const base = { args: { element_token: "s1:4" }, isError: true, action: true, observed: false, why: "x" };
  pr.onResult({ ...base, tool: "click", status: "blocked", refused: true, sc: hold, hold: "unknown_hold" });
  assert.match(pr.constraints.map((c) => c.text).join("\n"), /may already have happened.*never resend it/);
  pr.onResult({ ...base, tool: "type_text", status: "blocked", refused: false, sc: prem, hold: "premise" });
  assert.match(pr.open.map((c) => c.text).join("\n"), /approved changed before it was sent/);
  pr.onResult({ ...base, tool: "press_key", status: "blocked", refused: false, sc: rv, hold: "review_unavailable" });
  assert.equal(pr.failures.filter((f) => /press_key/.test(f.text)).length, 1, "one failure line, not two");
  // the premise item closes when the step later runs
  pr.onResult({ ...base, tool: "type_text", isError: false, status: "done", refused: false, sc: {}, observed: true });
  assert.ok(!pr.open.some((c) => /approved changed/.test(c.text)));

  // the client guard never resends an unknown_hold step
  const g = new ActionGuard();
  g.onResult("click", "blocked", true, { element_token: "s1:4" }, false, "unknown_hold");
  const tools = new Set(["click", "get_window_state"]);
  assert.match(g.check("click", tools, { element_token: "s1:4" })!, /may already have happened/);
  assert.equal(g.check("click", tools, { element_token: "s1:5" }), null);
});

test("reread after a person's turn: only what the gate did not read", () => {
  assert.equal(rereadOf({}), null);
  const rr = rereadOf({ reread: { window: ["button: OK"], focused_window: 7, focused_title: "Calc", files_changed: ["/home/u/q3.ods"], note: "n" } })!;
  assert.deepEqual([rr.focusedWindow, rr.rows, rr.files], [7, 1, ["/home/u/q3.ods"]]);
  // older gate: read the window and list the windows
  assert.deepEqual(afterPerson(null, false, 7), { force: true, listWindows: true, why: PERSON_ACTED });
  // the gate re-read the target window (attached) and the focus stayed: nothing more
  assert.deepEqual(afterPerson(rr, true, 7), { force: false, listWindows: false, why: "" });
  // the focus moved: read + window list
  const moved = afterPerson({ ...rr, focusedWindow: 9, focusedTitle: "Files" }, true, 7);
  assert.equal(moved.force && moved.listWindows, true);
  assert.match(moved.why, /focus is now on window 9 "Files"/);
  // no tree in the result: read, but the gate already named the focused window
  assert.deepEqual(afterPerson(rr, false, 7).listWindows, false);
  const o = new ObsTracker();
  o.force("1:7", "x", true);
  assert.ok(o.wantsList("1:7"));
  o.onObservation("1:7", { kind: "full", snapshot: "s1", base: null }, false);
  assert.ok(!o.wantsList("1:7") && o.anchorReason("1:7") === null);
});

test("end to end: done_by_person with the gate's re-read and a fresh read attached - no second read, no window list", async () => {
  const done = { content: [{ type: "text", text: "[gate] A person did this step themselves.\n[gate] Window right after the action (snapshot s10; full)\nwindow_id=7 pid=42\n[gate] Re-read after the person's turn: files changed meanwhile: /home/u/q3.ods; earlier reads, tokens and approvals are void" }],
                 structuredContent: { status: "done_by_person", pid: 42, window_id: 7, snapshot_id: "s10",
                                      reread: { window: ["button: OK"], focused_window: 7, focused_title: "Calc", files_changed: ["/home/u/q3.ods"], note: "n" },
                                      postcondition: { status: "reread", reread: true } } };
  const h = await load({ tools: ["get_window_state", "click", "list_windows"], results: { click: done, get_window_state: read(42, 7, "s9"),
    list_windows: { content: [{ type: "text", text: "42:7 Calc" }], structuredContent: { status: "done" } } } });
  await h.pi.call("get_window_state", { pid: 42, window_id: 7 });
  await h.pi.call("click", { pid: 42, window_id: 7, element_token: "s9:3" });
  await turnEnd(h.pi);
  assert.equal(h.calls().filter((c: any) => c.name === "get_window_state").length, 1, "the gate's read is not repeated");
  assert.equal(h.calls().filter((c: any) => c.name === "list_windows").length, 0, "the gate named the focused window");
  assert.ok(h.api.facts.files.includes("/home/u/q3.ods"), "files the person changed are kept");
  assert.equal((await h.pi.call("click", { pid: 42, window_id: 7, element_token: "s9:3" })).blocked, true, "never resent");
  h.api.mcp.close();
});

test("end to end: done_by_person and the focus moved - the client reads and lists the windows", async () => {
  const done = { content: [{ type: "text", text: "[gate] A person did this step themselves." }],
                 structuredContent: { status: "done_by_person", pid: 42, window_id: 7,
                                      reread: { window: [], focused_window: 9, focused_title: "Files", files_changed: [], note: "n" } } };
  const h = await load({ tools: ["get_window_state", "click", "list_windows"], results: { click: done, get_window_state: read(42, 7, "s9"),
    list_windows: { content: [{ type: "text", text: "42:7 Calc\n43:9 Files" }], structuredContent: { status: "done" } } } });
  await h.pi.call("get_window_state", { pid: 42, window_id: 7 });
  await h.pi.call("click", { pid: 42, window_id: 7, element_token: "s9:3" });
  const r = await turnEnd(h.pi);
  const m = r.entries.at(-1);
  assert.match(txt(m), /focus is now on window 9 "Files"/);
  assert.match(txt(m), /43:9 Files/);
  h.api.mcp.close();
});
