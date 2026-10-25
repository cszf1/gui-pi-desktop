import { test } from "node:test";
import assert from "node:assert/strict";
import { PlanTracker, Reminder } from "../src/plan.ts";

test("reminders only at milestones, refusals and stalls", () => {
  const p = new PlanTracker();
  const r = new Reminder(p, 3);
  const setPlan = (args: any) => {
    const before = p.counts();
    p.update(args);
    return r.onResult({ tool: "set_plan", action: false, refused: false, before, after: p.counts() });
  };
  assert.equal(setPlan({ feasible: true, milestones: ["open", "edit", "save"] }), null);
  const act = () => r.onResult({ tool: "click", action: true, refused: false });
  assert.equal(act(), null);
  assert.equal(act(), null);
  assert.match(act()!, /3 actions since the last milestone/);
  assert.equal(act(), null); // counter reset
  assert.match(setPlan({ done: [1] })!, /plan 1\/3 done; next: edit\] \(milestone reached\)/);
  assert.match(setPlan({ failed: [2] })!, /milestone failed/);
  assert.equal(r.onResult({ tool: "get_window_state", action: false, refused: false }), null);
  assert.match(r.onResult({ tool: "click", action: true, refused: true })!, /after a refusal/);
  assert.match(p.summary(), /2\. \[!\] edit/);
});

test("one plan voice: the gate's per-result [plan] line is removed; reminders after compaction and on drift only", async () => {
  const { stripGatePlan } = await import("../src/plan.ts");
  const c = stripGatePlan([{ type: "text", text: "Clicked" }, { type: "text", text: "[plan] 1/3 done; next: 2. save" }, { type: "image", data: "x" }]);
  assert.deepEqual(c.map((x: any) => x.type), ["text", "image"]);
  const p = new PlanTracker();
  p.update({ feasible: true, milestones: ["a", "b"] });
  const r = new Reminder(p, 3);
  assert.equal(r.onResult({ tool: "click", action: true, refused: false }), null);
  r.afterCompaction();
  assert.match(r.onResult({ tool: "click", action: true, refused: false })!, /after compaction/);
  assert.equal(r.onResult({ tool: "click", action: true, refused: false }), null, "once only");
  r.onResult({ tool: "click", action: true, refused: false });
  assert.match(r.onResult({ tool: "click", action: true, refused: false })!, /check you are still on the plan/);
});
