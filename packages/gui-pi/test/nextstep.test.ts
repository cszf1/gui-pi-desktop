// Items 1-2: gate_next_step before each turn (with and without the tool), rethink, done_by_person.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, txt, read, IMG } from "./harness.ts";
import { parseNextStep, EXTRA_REASON_NOTE, NEXT_STEP_TOOL } from "../src/nextstep.ts";
import { interpret } from "../src/status.ts";
import { ActionGuard } from "../src/guard.ts";
import { OBSERVE_TYPE, prune } from "../src/context.ts";

const NS = (observe: string, extra = false, id = "d-1") => ({ content: [{ type: "text", text: "next" }],
  structuredContent: { observe, extra_reason: extra, reason: "window changed", decision_id: id } });
const turnEnd = (pi: any) => pi.emit("turn_end", { toolResults: [{}], entries: [] });

test("parseNextStep accepts only the three observe values", () => {
  assert.equal(parseNextStep(NS("tree") as any)?.observe, "tree");
  assert.equal(parseNextStep({ content: [], structuredContent: { status: "done" } } as any), null);
  assert.equal(parseNextStep({ content: [], structuredContent: { observe: "everything" } } as any), null);
  assert.equal(parseNextStep({ content: [], isError: true, structuredContent: { observe: "tree" } } as any), null);
});

test("next step: tree, tree+screenshot, none, extra_reason; decision ids logged; never offered to the model", async () => {
  const h = await load({ tools: ["get_window_state", "click", NEXT_STEP_TOOL, "gate_reset"],
    results: { [NEXT_STEP_TOOL]: [NS("tree", false, "d-1"), NS("tree+screenshot", false, "d-2"), NS("none", false, "d-3"), NS("none", true, "d-4")],
               get_window_state: [read(42, 7, "s0"), read(42, 7, "s1"), read(42, 7, "s2", true)] } }, { GUI_PI_FRESH_MS: "0" });
  assert.ok(!h.pi.tools.has(NEXT_STEP_TOOL) && !h.pi.tools.has("gate_reset"), "client-only tools are not Pi tools");
  await h.pi.call("get_window_state", { pid: 42, window_id: 7 }); // focus = 42:7
  // 1: tree
  let r = await turnEnd(h.pi);
  let m = r.entries.at(-1);
  assert.equal(m.type, "custom_message");
  assert.equal(m.customType, OBSERVE_TYPE);
  assert.match(txt(m), /d-1/);
  assert.ok(!m.content.some((c: any) => c.type === "image"));
  assert.equal(m.details.obs.snapshot, "s1");
  // 2: tree + screenshot (the image arrives byte for byte, with its id)
  r = await turnEnd(h.pi);
  m = r.entries.at(-1);
  const img = m.content.find((c: any) => c.type === "image");
  assert.equal(img.data, IMG);
  assert.deepEqual(m.details.images, ["img-1"]);
  // 3: none -> nothing attached
  assert.equal(await turnEnd(h.pi), undefined);
  // 4: none + extra_reason -> only the fixed note
  r = await turnEnd(h.pi);
  assert.deepEqual(r.entries.at(-1).content, [{ type: "text", text: EXTRA_REASON_NOTE }]);
  const calls = h.calls();
  const ns = calls.filter((c: any) => c.name === NEXT_STEP_TOOL);
  assert.equal(ns.length, 4);
  assert.deepEqual(ns[0].arguments, { window_id: 7 });
  // B9: the client's own calls carry the gate's client marker (no client_tool_unmarked); the model's calls never do
  assert.ok(ns.every((c: any) => c._meta?.["io.github.cszf1/gui-gate/client"] === true), "gate_next_step is marked");
  assert.ok(calls.filter((c: any) => c.name !== NEXT_STEP_TOOL && c.name !== "gate_reset").every((c: any) => c._meta?.["io.github.cszf1/gui-gate/client"] === undefined));
  const reads = calls.filter((c: any) => c.name === "get_window_state");
  assert.deepEqual(reads.map((c: any) => c.arguments.include_screenshot), [undefined, false, true]);
  const logged = h.pi.entries.filter((e) => e.type === "gui-pi-nextstep").map((e) => e.data.decision_id);
  assert.deepEqual(logged, ["d-1", "d-2", "d-3", "d-4"]);
  // the attached read is a diff base like a tool result's read
  const ctx = prune([{ role: "custom", customType: OBSERVE_TYPE, content: [{ type: "text", text: "x" }], details: { obs: { kind: "full", snapshot: "s9", base: null }, windowKey: "42:7" } },
                     { role: "toolResult", toolName: "click", content: [{ type: "text", text: "[gate] Window right after the action (snapshot s10; Only rows that changed since snapshot s9)" }], details: { windowKey: "42:7", obs: { kind: "diff", snapshot: "s10", base: "s9" } } }]);
  assert.equal(txt(ctx[0]), "x", "the base of the newest diff stays verbatim");
  h.api.mcp.close();
});

test("without gate_next_step (current gate): tried once, then the turn goes on as before", async () => {
  const h = await load({ tools: ["get_window_state", "click"] }); // unknown names get "ok" like an old gate's driver
  await h.pi.call("get_window_state", { pid: 42, window_id: 7 });
  assert.equal(await turnEnd(h.pi), undefined);
  assert.equal(await turnEnd(h.pi), undefined);
  assert.equal(await h.pi.emit("before_agent_start", { systemPromptOptions: { sections: {}, skills: [] } }), undefined);
  assert.equal(h.calls().filter((c: any) => c.name === NEXT_STEP_TOOL).length, 1);
  assert.equal(h.api.nextStep.tool.state, "no");
  h.api.mcp.close();
});

test("rethink and done_by_person: notes, and the person's step is never resent", async () => {
  const rethink = interpret("click", {}, { isError: true, content: [{ type: "text", text: "[gate] Not executed" }],
                                         structuredContent: { status: "blocked", next_action: "rethink" } });
  assert.equal(rethink.executed, false);
  assert.match(rethink.notes.join(" "), /RETHINK/);
  const p = interpret("click", {}, { content: [{ type: "text", text: "[gate] A person did this step themselves" }],
                                    structuredContent: { status: "done_by_person" } });
  assert.equal(p.status, "done_by_person");
  assert.equal(p.executed, false);
  assert.match(p.notes.join(" "), /A PERSON DID THIS STEP/);
  // the older gate flag maps to the same
  assert.equal(interpret("click", {}, { content: [], structuredContent: { gate: { done_by_person: true } } }).status, "done_by_person");

  const g = new ActionGuard();
  const tools = new Set(["click", "get_window_state", "type_text"]);
  g.onResult("click", "done_by_person", false, { element_token: "s1:3" }, false);
  assert.match(g.check("type_text", tools, { text: "x" })!, /Read the window first/);
  g.onResult("get_window_state", "done", false);
  assert.equal(g.check("type_text", tools, { text: "x" }), null);
  assert.match(g.check("click", tools, { element_token: "s1:3" })!, /already did exactly this step/);
  assert.equal(g.check("click", tools, { element_token: "s2:3" }), null);

  // end to end: the gate answers done_by_person; the identical call is held, nothing more reaches the gate
  const h = await load({ tools: ["get_window_state", "click"], results: {
    click: { content: [{ type: "text", text: "[gate] A person did this step themselves." }], structuredContent: { status: "done_by_person" } } } });
  const r1 = await h.pi.call("click", { element_token: "s1:3" });
  assert.match(txt(r1), /Do not send it again/);
  const r2 = await h.pi.call("click", { element_token: "s1:3" });
  assert.equal(r2.blocked, true);
  assert.equal(h.calls().filter((c: any) => c.name === "click").length, 1);
  h.api.mcp.close();
});
