import { test } from "node:test";
import assert from "node:assert/strict";
import { TaskFacts, ENTRY_TYPE, filesOf, parseTaskFile, shorten } from "../src/facts.ts";

test("facts survive a save / restore round trip (what compaction relies on)", () => {
  const f = new TaskFacts();
  f.setRequest("Rename report.ods to q3.ods and email it to Ana");
  f.setRequest("something the model said later"); // the first request stays
  f.addFact("Q3 total = 48,210", "Calc q3.ods F12", false);
  f.addFact("Q3 total = 48,210", "inspect", true); // upgraded to tool evidence, not duplicated
  f.addAnswer("answered: q3.ods");
  f.addRefused("send mail to bob@x.org");
  f.addFile("/home/u/q3.ods");
  const branch = [{ type: "message" }, { type: "custom", customType: ENTRY_TYPE, data: JSON.parse(JSON.stringify(f.toJSON())) }];
  const g = new TaskFacts();
  assert.ok(g.restore(branch));
  assert.deepEqual(g.toJSON(), f.toJSON());
  const r = g.render();
  assert.match(r, /verbatim\):\nRename report.ods to q3.ods and email it to Ana/);
  assert.match(r, /Observed by a tool[^\n]*\n- Q3 total = 48,210 - inspect/);
  assert.match(r, /answered: q3.ods/);
  assert.match(r, /do not retry[\s\S]*bob@x.org/);
  assert.match(r, /\/home\/u\/q3.ods/);
  assert.equal(g.facts.length, 1);
});

test("files from run_command's changed_files", () => {
  assert.deepEqual(filesOf({ changed_files: ["/a", "/b"], files_written: [{ path: "/c" }] }), ["/a", "/b", "/c"]);
  assert.deepEqual(filesOf(null), []);
});

test("every user message is kept verbatim, versioned, with its source; no 4000 cap, not only the first", () => {
  const f = new TaskFacts();
  const long = "x".repeat(9000) + " END";
  assert.ok(f.addUserMessage(long, "initial"));
  assert.ok(!f.addUserMessage(long + "\n", "session"), "the same text from another source is one message");
  assert.ok(f.addUserMessage("Actually use q4.ods, not q3.ods", "input"));
  assert.ok(f.addUserMessage("and keep a copy", "task_file", "2026-10-26T12:00:00Z", 3));
  assert.equal(f.messages.length, 3);
  assert.equal(f.request, long, "the first request is not cut");
  assert.deepEqual(f.messages.map((m) => [m.v, m.source]), [[1, "initial"], [2, "input"], [3, "task_file"]]);
  const g = new TaskFacts();
  g.restore([{ type: "custom", customType: ENTRY_TYPE, data: JSON.parse(JSON.stringify(f.toJSON())) }]);
  assert.equal(g.messages[0].text.length, long.length);
  const r = g.render();
  assert.match(r, /3 messages; a later message corrects/);
  assert.match(r, /\[2\] \(input\) Actually use q4.ods, not q3.ods/);
  assert.match(r, /\[3\] \(task_file, task version 3\) and keep a copy/);
  assert.ok(r.includes(long), "fits the budget: rendered in full");
});

test("rendering may shorten the middle of a long history (and says so); the record never changes", () => {
  const f = new TaskFacts();
  for (let i = 0; i < 8; i++) f.addUserMessage(`m${i} ` + "y".repeat(5000), "input");
  const r = f.renderMessages(20_000, 100);
  assert.ok(r.includes("m0 " + "y".repeat(5000)), "the first stays in full");
  assert.ok(r.includes("m7 " + "y".repeat(5000)) && r.includes("m5 " + "y".repeat(5000)), "the newest three stay in full");
  assert.match(r, /m2 y+ \.\.\. \[shortened for display: \d+ more characters are kept in the task record\]/);
  assert.ok(f.messages.every((m) => m.text.length === 5003));
  assert.equal(shorten("abc", 10), "abc");
});

test("old saved facts (0.3.1: request only) load as the first message", () => {
  const f = new TaskFacts();
  f.load({ request: "old request", facts: [{ text: "check_done found no open gaps", source: "check_done (gate)", confirmed: true, at: "" }] } as any);
  assert.deepEqual(f.messages.map((m) => m.text), ["old request"]);
  assert.equal(f.facts[0].kind, "check", "an old check_done fact is a check, not evidence");
  assert.match(f.render(), /Checks \(a model's judgement, not proof/);
});

test("the shell's task file (gui-pi-task/1) parses into versioned messages", () => {
  const body = "gui-pi-task/1 version=2\n--- user 1 2026-10-26T01:00:00Z initial request\nrename q3.ods\nto q3-final.ods\n--- user 2 2026-10-26T01:05:00Z later message\nand keep a copy\n";
  assert.deepEqual(parseTaskFile(body), [{ v: 1, text: "rename q3.ods\nto q3-final.ods", at: "2026-10-26T01:00:00Z" },
                                         { v: 2, text: "and keep a copy", at: "2026-10-26T01:05:00Z" }]);
  assert.deepEqual(parseTaskFile("not a task file"), []);
  const f = new TaskFacts();
  f.addUserMessage("rename q3.ods\nto q3-final.ods", "initial");
  assert.ok(f.syncTaskFile(body));
  assert.equal(f.messages.length, 2);
  assert.equal(f.messages[0].task_v, 1, "the env copy learns its task version");
  assert.ok(!f.syncTaskFile(body), "nothing new the second time");
});

test("agent notes, tool evidence and checks render apart", () => {
  const f = new TaskFacts();
  f.addFact("total = 5", "Calc F2", false);
  f.addFact("file q3.ods exists", "postcondition", true, undefined, "tool_evidence");
  f.addFact("check_done reported no open gaps", "check_done (gate)", false, undefined, "check");
  const r = f.render();
  assert.match(r, /Observed by a tool[^\n]*\n- file q3.ods exists/);
  assert.match(r, /Noted by the agent \(its own notes; not verified by a tool\):\n- total = 5/);
  assert.match(r, /Checks \(a model's judgement, not proof from the environment that the task succeeded\):\n- check_done/);
});
