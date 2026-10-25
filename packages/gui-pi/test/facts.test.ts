import { test } from "node:test";
import assert from "node:assert/strict";
import { TaskFacts, ENTRY_TYPE, filesOf } from "../src/facts.ts";

test("facts survive a save / restore round trip (what compaction relies on)", () => {
  const f = new TaskFacts();
  f.setRequest("Rename report.ods to q3.ods and email it to Ana");
  f.setRequest("something the model said later"); // the first request stays
  f.addFact("Q3 total = 48,210", "Calc q3.ods F12", false);
  f.addFact("Q3 total = 48,210", "check_done", true); // upgraded, not duplicated
  f.addAnswer("answered: q3.ods");
  f.addRefused("send mail to bob@x.org");
  f.addFile("/home/u/q3.ods");
  const branch = [{ type: "message" }, { type: "custom", customType: ENTRY_TYPE, data: JSON.parse(JSON.stringify(f.toJSON())) }];
  const g = new TaskFacts();
  assert.ok(g.restore(branch));
  assert.deepEqual(g.toJSON(), f.toJSON());
  const r = g.render();
  assert.match(r, /verbatim\): Rename report.ods to q3.ods and email it to Ana/);
  assert.match(r, /Q3 total = 48,210 - check_done \(confirmed\)/);
  assert.match(r, /answered: q3.ods/);
  assert.match(r, /do not retry[\s\S]*bob@x.org/);
  assert.match(r, /\/home\/u\/q3.ods/);
  assert.equal(g.facts.length, 1);
});

test("files from run_command's changed_files", () => {
  assert.deepEqual(filesOf({ changed_files: ["/a", "/b"], files_written: [{ path: "/c" }] }), ["/a", "/b", "/c"]);
  assert.deepEqual(filesOf(null), []);
});
