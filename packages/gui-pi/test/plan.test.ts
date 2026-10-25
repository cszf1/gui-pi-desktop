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
