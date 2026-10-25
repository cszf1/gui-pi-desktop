import { test } from "node:test";
import assert from "node:assert/strict";
import { prune, obsKind, withPlanLine } from "../src/context.ts";

const read = (snap: string, rows: number, img = false, w = 777) => ({
  role: "toolResult", toolName: "get_window_state",
  content: [{ type: "text", text: `window_id=${w} pid=4242 snapshot_id=${snap}\n- frame = "Calc"\n` + Array.from({ length: rows }, (_, i) => `    - [${i}] button "b${i}"`).join("\n") },
            ...(img ? [{ type: "image", data: "AAAA", mimeType: "image/jpeg" }] : [])],
  details: { windowKey: `4242:${w}` },
});
const act = (snap: string, diff: boolean, w = 777) => ({
  role: "toolResult", toolName: "click",
  content: [{ type: "text", text: `Clicked.\n[gate] Window right after the action (snapshot ${snap}; tokens are ${snap}:N, older tokens are stale).` +
    (diff ? ` Only rows that changed since snapshot s1 are shown (1 of 9 rows, with their parents);` : "") + `\n    - [0] button "x"` }],
  details: { windowKey: `4242:${w}` },
});

test("kinds", () => {
  assert.equal(obsKind(read("s1", 3) as any), "full");
  assert.equal(obsKind(act("s2", true) as any), "diff");
  assert.equal(obsKind(act("s2", false) as any), "full");
  assert.equal(obsKind({ role: "toolResult", toolName: "gate_report", content: [{ type: "text", text: "x" }] } as any), null);
});

test("the anchor a later diff builds on stays verbatim", () => {
  const msgs: any[] = [{ role: "user", content: [{ type: "text", text: "do it" }] }, read("s0", 5), read("s1", 9), act("s2", true), act("s3", true)];
  const out = prune(msgs);
  assert.match(out[1].content[0].text, /older window read summarised/); // before the anchor
  assert.equal(out[2], msgs[2]); // the anchor
  assert.equal(out[3], msgs[3]);
  assert.equal(out[4], msgs[4]);
});

test("a new full read becomes the anchor; windows are independent", () => {
  const msgs: any[] = [read("s1", 9), act("s2", true), read("t1", 4, false, 888), act("s3", false)];
  const out = prune(msgs);
  assert.match(out[0].content[0].text, /summarised/);
  assert.match(out[1].content[0].text, /^Clicked\.\n\[client: older window read summarised/);
  assert.equal(out[2], msgs[2]); // other window untouched
  assert.equal(out[3], msgs[3]);
});

test("only the newest screenshot survives", () => {
  const msgs: any[] = [read("s1", 3, true, 1), read("s2", 3, true, 2)];
  const out = prune(msgs);
  assert.ok(!out[0].content.some((c: any) => c.type === "image"));
  assert.ok(out[0].content.some((c: any) => /older screenshot was removed/.test(c.text || "")));
  assert.ok(out[1].content.some((c: any) => c.type === "image"));
});

test("plan line goes on the newest tool result unless the gate printed one", () => {
  const msgs: any[] = [read("s1", 2)];
  assert.match(withPlanLine(msgs, "[client: plan 1/3]")[0].content.at(-1).text, /plan 1\/3/);
  const g: any[] = [{ role: "toolResult", toolName: "click", content: [{ type: "text", text: "ok\n[plan] 1/3 done" }] }];
  assert.equal(withPlanLine(g, "[client: plan 1/3]"), g);
});
