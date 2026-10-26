// 0.3.2 (review 2026-10-26 v2, desktop client): user messages kept verbatim, process state across compaction,
// one freshness judgement + anchor reads, gate v0.10.3 structured execution state, platform conditions, latency.
import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:net";
import { mkdtempSync, writeFileSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { load, txt, read, ipcPath } from "./harness.ts";
import { interpret, continuationOf, completedOf } from "../src/status.ts";
import { Progress, postconditions, describeCall } from "../src/progress.ts";
import { ObsTracker } from "../src/observe.ts";
import { classify, PlatformHealth } from "../src/platform.ts";
import { LatencyBook } from "../src/latency.ts";
import { NEXT_STEP_TOOL } from "../src/nextstep.ts";

const NS = (observe: string, id = "d") => ({ content: [{ type: "text", text: "next" }],
  structuredContent: { observe, extra_reason: false, reason: "r", decision_id: id } });
const turnEnd = (pi: any) => pi.emit("turn_end", { toolResults: [{}], entries: [] });
const after = (snap: string, base: string | null, win = 7) => ({
  content: [{ type: "text", text: `Clicked\n[gate] Window right after the action (snapshot ${snap}; ${base ? `Only rows that changed since snapshot ${base}` : "full"})\nwindow_id=${win} pid=42` }],
  structuredContent: { status: "executed", pid: 42, window_id: win, snapshot_id: snap,
    ...(base ? { diff: { base_snapshot_id: base, snapshot_id: snap, changed: [], removed: [], reordered: [] } } : {}) },
});

// ---------------------------------------------------------------- 1. user messages

test("every user correction is recorded (input / message_end / task file) and survives compaction in full", async () => {
  const dir = mkdtempSync(join(tmpdir(), "v032-"));
  const tf = join(dir, "task.txt");
  const long = "Rename report.ods to q3.ods. " + "Details: ".repeat(700);
  writeFileSync(tf, `gui-pi-task/1 version=1\n--- user 1 2026-10-26T01:00:00Z initial request\n${long}\n`);
  const h = await load({ tools: ["click"] }, { GUI_PI_TASK: long, GUI_PI_TASK_FILE: tf });
  await h.pi.emit("input", { text: "Actually call it q4.ods", source: "rpc" });
  await h.pi.emit("message_end", { message: { role: "user", content: "Actually call it q4.ods" } }); // same text: one message
  writeFileSync(tf, `gui-pi-task/1 version=2\n--- user 1 2026-10-26T01:00:00Z initial request\n${long}\n--- user 2 2026-10-26T01:02:00Z later message\nand keep the old one\n`);
  await h.pi.emit("turn_start", {});
  await h.pi.emit("message_end", { message: { role: "user", content: [{ type: "text", text: "no wait, q5.ods" }] } });
  const msgs = h.api.facts.messages.map((m: any) => [m.v, m.source, m.text.length > 100 ? "long" : m.text]);
  assert.deepEqual(msgs, [[1, "initial", "long"], [2, "input", "Actually call it q4.ods"], [3, "task_file", "and keep the old one"], [4, "session", "no wait, q5.ods"]]);
  assert.equal(h.api.facts.messages[0].text, long, "not cut at 4000 characters");
  const r = await h.pi.emit("session_before_compact", { preparation: {}, branchEntries: [] });
  const sum = r.compaction.summary;
  assert.ok(sum.includes(long), "the first request in full");
  assert.match(sum, /\[2\] \(input\) Actually call it q4.ods/);
  assert.match(sum, /\[4\] \(session\) no wait, q5.ods/);
  // what is saved in the session is the full record
  const saved = h.pi.entries.filter((e) => e.type === "gui-pi-facts").at(-1)!.data;
  assert.equal(saved.messages.length, 4);
  h.api.mcp.close();
});

test("compaction fallback: every user message on the branch, verbatim (not the first 2000 characters of the first)", async () => {
  const h = await load({ tools: ["click"] });
  const first = "A".repeat(3000) + " first";
  const r = await h.pi.emit("session_before_compact", { preparation: {}, branchEntries: [
    { message: { role: "user", content: [{ type: "text", text: first }] } },
    { message: { role: "assistant", content: [{ type: "text", text: "ok" }] } },
    { message: { role: "user", content: "and then close the window" } }] });
  assert.ok(r.compaction.summary.includes(first));
  assert.match(r.compaction.summary, /\(session\) and then close the window/);
  h.api.mcp.close();
});

// ---------------------------------------------------------------- 2. process state

test("process state is extracted from tool results, not from note_fact, and rendered at compaction", async () => {
  const h = await load({ tools: ["click", "run_actions", "check_done", "get_window_state", "set_plan"], results: {
    click: [{ content: [{ type: "text", text: "[gate] Not executed: the target is disabled" }], isError: true,
              structuredContent: { status: "blocked", gate: { decision: "look_full", executed: false } } },
            { content: [{ type: "text", text: "[gate] outcome unknown: the driver timed out" }], isError: true, structuredContent: { status: "unknown" } }],
    run_actions: { content: [{ type: "text", text: "[gate] batch stopped" }], structuredContent: { status: "partial", completed: [0], batch: { total: 3, stopped: "window changed" } } },
    check_done: { content: [{ type: "text", text: "no gaps" }], structuredContent: { status: "done", gaps: [] } },
  } });
  await h.pi.call("set_plan", { milestones: ["open q3.ods", "save as q4.ods"] });
  await h.pi.call("click", { element_token: "s1:4" });
  await h.pi.call("run_actions", { steps: [{ tool: "click" }, { tool: "type_text" }, { tool: "press_key" }] });
  await h.pi.call("click", { element_token: "s1:9" });
  await h.pi.call("check_done", {});
  const p = h.api.progress;
  assert.ok(p.failures.some((x: any) => /click element_token=s1:4: not executed - \[gate\] Not executed: the target is disabled/.test(x.text)));
  assert.ok(p.failures.some((x: any) => /batch stopped after 1 of 3/.test(x.text)));
  assert.ok(p.open.some((x: any) => /steps 2, 3 did not run/.test(x.text)));
  assert.ok(p.open.some((x: any) => /plan step 1: open q3.ods/.test(x.text)));
  assert.ok(p.unverified.some((x: any) => /s1:9: outcome unknown/.test(x.text)));
  assert.ok(p.unverified.some((x: any) => /check_done reported no gap: a model's judgement, not proof/.test(x.text)));
  // facts: the check is a check, not evidence
  assert.ok(h.api.facts.facts.some((f: any) => f.kind === "check"));
  assert.ok(!h.api.facts.facts.some((f: any) => f.kind === "tool_evidence"));
  const sum = (await h.pi.emit("session_before_compact", { preparation: {}, branchEntries: [] })).compaction.summary;
  assert.match(sum, /Process state \(extracted by the program/);
  assert.match(sum, /Tried and did not work[\s\S]*target is disabled/);
  assert.match(sum, /Not verified yet[\s\S]*outcome unknown/);
  assert.match(sum, /Still open[\s\S]*did not run/);
  assert.match(sum, /Checks \(a model's judgement, not proof from the environment/);
  // saved and restorable
  const saved = h.pi.entries.filter((e) => e.type === "gui-pi-progress").at(-1)!.data;
  const q = new Progress();
  assert.ok(q.restore([{ type: "custom", customType: "gui-pi-progress", data: JSON.parse(JSON.stringify(saved)) }]));
  assert.equal(q.render(), p.render());
  // a read settles the unknown one
  await h.pi.call("get_window_state", { pid: 42, window_id: 7 });
  assert.ok(!p.unverified.some((x: any) => /outcome unknown/.test(x.text)));
  assert.ok(!p.open.some((x: any) => x.key === "reread"));
  h.api.mcp.close();
});

test("postconditions: met is tool evidence, unmet a failed method, unchecked unverified; constraints from the gate", () => {
  assert.deepEqual(postconditions({ postconditions: [{ what: "file q4.ods exists", met: true, detail: "12 kB" }, { check: "title", ok: false }, "saved"] }),
    [{ what: "file q4.ods exists", met: true, detail: "12 kB" }, { what: "title", met: false, detail: "" }, { what: "saved", met: null, detail: "" }]);
  assert.deepEqual(postconditions({}), []);
  const p = new Progress();
  p.onResult({ tool: "click", args: { element_token: "s1:2" }, status: "done", isError: false, refused: false, action: true, observed: true, why: "",
               sc: { postconditions: [{ what: "dialog closed", met: false, detail: "still open" }], constraints: ["do not close Calc"] } });
  assert.match(p.failures[0].text, /ran, but "dialog closed" was not met \(still open\)/);
  assert.match(p.constraints[0].text, /gate: do not close Calc/);
  p.onResult({ tool: "type_text", args: { text: "hi" }, status: "executed", isError: false, refused: false, action: true, observed: false, why: "", sc: {} });
  assert.match(p.unverified[0].text, /type_text text=hi: ran; nothing observed its effect yet/);
  assert.equal(describeCall("run_command", { command: "ls -la /tmp" }), "run_command command=ls -la /tmp");
});

// ---------------------------------------------------------------- 3. freshness and anchors

test("the next-step read is skipped when the action's own observation is fresh; read again after a blind action", async () => {
  const h = await load({ tools: ["get_window_state", "click", "type_text", NEXT_STEP_TOOL],
    results: { [NEXT_STEP_TOOL]: NS("tree"), click: after("s2", null), type_text: { content: [{ type: "text", text: "typed" }], structuredContent: { status: "executed" } },
               get_window_state: read(42, 7, "s3") } });
  await h.pi.call("click", { pid: 42, window_id: 7, element_token: "s1:1" });
  assert.equal(await turnEnd(h.pi), undefined, "the tree that came with the action is current: not read again");
  const logged = h.pi.entries.filter((e) => e.type === "gui-pi-nextstep").at(-1)!.data;
  assert.equal(logged.skipped, "fresh");
  assert.equal(h.calls().filter((c: any) => c.name === "get_window_state").length, 0);
  await h.pi.call("type_text", { pid: 42, window_id: 7, text: "x" }); // ran, no observation: stale
  const r = await turnEnd(h.pi);
  assert.equal(r.entries.at(-1).details.obs.snapshot, "s3");
  assert.equal(h.calls().filter((c: any) => c.name === "get_window_state").length, 1);
  h.api.mcp.close();
});

test("a screenshot request is not satisfied by a tree-only observation", () => {
  let t = 0;
  const o = new ObsTracker({ now: () => t, freshMs: 1000 });
  o.onObservation("1:2", { kind: "full", snapshot: "s1", base: null }, false);
  assert.ok(o.isFresh("1:2", false));
  assert.ok(!o.isFresh("1:2", true));
  t = 2000;
  assert.ok(!o.isFresh("1:2", false), "too old");
  assert.ok(!o.isFresh(undefined, false));
});

test("anchors: long diff chain, missing base, compaction, a person acted", () => {
  const o = new ObsTracker({ maxChain: 3 });
  o.onObservation("w", { kind: "full", snapshot: "s0", base: null }, false);
  for (let i = 1; i <= 3; i++) o.onObservation("w", { kind: "diff", snapshot: `s${i}`, base: `s${i - 1}` }, false);
  assert.equal(o.anchorReason("w"), null);
  o.onObservation("w", { kind: "diff", snapshot: "s4", base: "s3" }, false);
  assert.match(o.anchorReason("w")!, /4 diffs in a row/);
  o.onObservation("w", { kind: "full", snapshot: "s5", base: null }, false);
  assert.equal(o.anchorReason("w"), null, "a full read is the new anchor");
  o.onObservation("w", { kind: "diff", snapshot: "s6", base: "s-never-seen" }, false);
  assert.match(o.anchorReason("w")!, /base snapshot s-never-seen is not in context/);
  o.onObservation("w", { kind: "full", snapshot: "s7", base: null }, false);
  o.onCompaction();
  assert.match(o.anchorReason("w")!, /compacted/);
  o.onObservation("w", { kind: "full", snapshot: "s8", base: null }, false);
  o.onObservation("w", { kind: "diff", snapshot: "s9", base: "s7" }, false);
  assert.match(o.anchorReason("w")!, /s7 is not in context/, "a base from before the compaction is gone");
});

test("end to end: a long diff chain makes the client read the whole window even without gate_next_step", async () => {
  const clicks = Array.from({ length: 8 }, (_, i) => after(`s${i + 1}`, `s${i}`));
  const h = await load({ tools: ["get_window_state", "click"], results: { get_window_state: [read(42, 7, "s0"), read(42, 7, "s20")], click: clicks } },
                       { GUI_PI_FRESH_MS: "0" });
  await h.pi.call("get_window_state", { pid: 42, window_id: 7 });
  for (let i = 0; i < 6; i++) await h.pi.call("click", { pid: 42, window_id: 7, element_token: `s${i}:1` });
  assert.equal(await turnEnd(h.pi), undefined, "6 diffs: no anchor yet");
  await h.pi.call("click", { pid: 42, window_id: 7, element_token: "s6:1" });
  const r = await turnEnd(h.pi);
  const m = r.entries.at(-1);
  assert.match(txt(m), /a fresh full read of the focused window before this turn \(7 diffs in a row/);
  assert.equal(m.details.obs.kind, "full");
  const reads = h.calls().filter((c: any) => c.name === "get_window_state");
  assert.equal(reads.length, 2);
  assert.equal(reads[1].arguments.include_screenshot, false);
  assert.equal(await turnEnd(h.pi), undefined, "the anchor resets the chain");
  h.api.mcp.close();
});

test("end to end: a diff whose base is missing is followed by an anchor read", async () => {
  const h = await load({ tools: ["get_window_state", "click"], results: { click: after("s5", "s4"), get_window_state: read(42, 7, "s6") } });
  await h.pi.call("click", { pid: 42, window_id: 7, element_token: "s4:1" });
  const r = await turnEnd(h.pi);
  assert.match(txt(r.entries.at(-1)), /base snapshot s4 is not in context/);
  h.api.mcp.close();
});

test("done_by_person: the affected window and the window list are re-read; the action is never replayed", async () => {
  const h = await load({ tools: ["get_window_state", "click", "list_windows"], results: {
    click: { content: [{ type: "text", text: "[gate] A person did this step themselves." }], structuredContent: { status: "done_by_person", pid: 42, window_id: 7 } },
    get_window_state: read(42, 7, "s9"), list_windows: { content: [{ type: "text", text: "42:7 Calc\n43:1 Files" }], structuredContent: { status: "done" } } } });
  await h.pi.call("get_window_state", { pid: 42, window_id: 7 });
  const r1 = await h.pi.call("click", { pid: 42, window_id: 7, element_token: "s9:3" });
  assert.match(txt(r1), /re-read the affected state first/);
  const r = await turnEnd(h.pi);
  const m = r.entries.at(-1);
  assert.match(txt(m), /a person acted/);
  assert.match(txt(m), /43:1 Files/);
  assert.equal((await h.pi.call("click", { pid: 42, window_id: 7, element_token: "s9:3" })).blocked, true, "never resent");
  assert.equal(h.calls().filter((c: any) => c.name === "click").length, 1);
  h.api.mcp.close();
});

// ---------------------------------------------------------------- 6. gate v0.10.3 structured state

test("v0.10.3: partial with completed indices names the steps that did not run", () => {
  const i = interpret("run_actions", { steps: [1, 2, 3, 4] }, { content: [], structuredContent: { status: "partial", completed: [0, 2], continuation_id: "c-7", batch: { total: 4 } } });
  assert.deepEqual(i.steps, { ran: 2, total: 4, completed: [0, 2], notRun: [1, 3] });
  assert.match(i.notes.join(" "), /Steps that ran: 1, 3; steps that did NOT run: 2, 4 \(1-based\)/);
  assert.match(i.notes.join(" "), /continuation id for this step is c-7; it names this decision, it authorises nothing/);
  assert.equal(i.continuation, "c-7");
  // 1-based indices when the gate says so; aliases
  assert.deepEqual(completedOf({ batch: { completed_indices: [1, 2], index_base: 1 } }), [0, 1]);
  assert.equal(continuationOf({ state: { resume_id: "r1" } }), "r1");
  assert.equal(continuationOf({}), null);
  assert.equal(completedOf({}), null);
});

test("v0.10.3: unknown says the result is unknown and must not be resent; done counts as executed; missing fields = old behaviour", () => {
  const u = interpret("click", {}, { isError: true, content: [], structuredContent: { status: "unknown", completed: [] } });
  assert.equal(u.executed, "unknown");
  assert.match(u.notes.join(" "), /RESULT UNKNOWN[\s\S]*Do not resend it directly/);
  const d = interpret("click", {}, { content: [], structuredContent: { status: "done" } });
  assert.equal(d.executed, true);
  assert.equal(d.notes.length, 0);
  // without completed: the 0.3.1 wording (ran k of n, steps after k did not run)
  const p = interpret("run_actions", { steps: [1, 2, 3] }, { content: [], structuredContent: { status: "partial", batch: { executed: 1, total: 3 } } });
  assert.deepEqual(p.steps, { ran: 1, total: 3 });
  assert.match(p.notes.join(" "), /Steps after step 1 did NOT run/);
  assert.equal(p.continuation, null);
});

test("v0.10.3: a met postcondition becomes tool evidence in the task facts", async () => {
  const h = await load({ tools: ["click"], results: { click: { content: [{ type: "text", text: "Clicked" }],
    structuredContent: { status: "done", postconditions: [{ what: "q4.ods saved", met: true, detail: "mtime changed" }, { what: "title", met: null }] } } } });
  await h.pi.call("click", { element_token: "s1:1" });
  assert.ok(h.api.facts.facts.some((f: any) => f.kind === "tool_evidence" && /q4.ods saved: mtime changed/.test(f.text)));
  assert.ok(h.api.progress.unverified.some((x: any) => /"title" not checked/.test(x.text)));
  h.api.mcp.close();
});

// ---------------------------------------------------------------- 8. platform conditions

test("platform: classification is narrow (error results only)", () => {
  assert.equal(classify("Error: The name org.a11y.Bus was not provided by any .service files", true), "a11y_lost");
  assert.equal(classify("driver: Couldn't connect to accessibility bus: Failed to connect to socket", true), "a11y_lost");
  assert.equal(classify("dbus: connection closed by peer", true), "a11y_lost");
  assert.equal(classify("UIA: the foreground window is on the secure desktop (consent.exe)", true), "secure_desktop");
  assert.equal(classify("The workstation is locked", true), "locked");
  assert.equal(classify("No interactive desktop: the RDP session is disconnected", true), "no_desktop");
  assert.equal(classify("SendInput: access denied (target window is elevated)", true), "elevated_target");
  assert.equal(classify("Document 'locked.ods': the workstation is locked", false, "executed"), null, "an app's own text is not an error");
  assert.equal(classify("Clicked OK", true), null);
});

test("platform: a lost accessibility bus twice holds GUI actions until a read works", async () => {
  const lost = { content: [{ type: "text", text: "driver error: org.a11y.Bus not reachable" }], isError: true, structuredContent: { status: "blocked" } };
  const h = await load({ tools: ["get_window_state", "click", "request_human"], results: {
    click: lost, get_window_state: [lost, read(42, 7, "s1")] } });
  const r1 = await h.pi.call("click", { element_token: "s0:1" });
  assert.match(txt(r1), /PLATFORM - the accessibility bus/);
  await h.pi.call("click", { element_token: "s0:1" });
  const held = await h.pi.call("click", { element_token: "s0:2" });
  assert.equal(held.blocked, true);
  assert.match(held.reason, /accessibility bus was lost/);
  await h.pi.call("get_window_state", { pid: 42, window_id: 7 }); // still lost: allowed (reads always are)
  assert.equal((await h.pi.call("click", { element_token: "s0:2" })).blocked, true);
  await h.pi.call("get_window_state", { pid: 42, window_id: 7 }); // works again
  assert.notEqual((await h.pi.call("click", { element_token: "s1:2" })).blocked, true);
  assert.ok(h.api.progress.constraints.some((x: any) => /accessibility bus was lost/.test(x.text)));
  h.api.mcp.close();
});

test("platform: a UAC prompt is released by a person doing the step", () => {
  const p = new PlatformHealth();
  const tools = new Set(["click"]);
  p.onResult("click", "secure desktop: UAC prompt is showing", true, "blocked");
  p.onResult("click", "secure desktop: UAC prompt is showing", true, "blocked");
  assert.match(p.check("click", tools)!, /UAC prompt needs the person/);
  p.onResult("click", "A person did this step", false, "done_by_person", true);
  assert.equal(p.check("click", tools), null);
});

// ---------------------------------------------------------------- 4. latency

test("latency: time to first token, total, images in context, to JSONL", async () => {
  let t = 1000;
  const dir = mkdtempSync(join(tmpdir(), "lat-"));
  const f = join(dir, "model.jsonl");
  const b = new LatencyBook(f, () => t);
  b.onTurnStart();
  b.onContext([{ role: "toolResult", content: [{ type: "image" }, { type: "text" }] }, { role: "custom", content: [{ type: "image" }] }]);
  t = 1100; b.onProviderRequest();
  t = 1900; b.onUpdate({ role: "assistant" });
  t = 2000; b.onUpdate({ role: "assistant" });
  t = 3100;
  const r = b.onEnd({ role: "assistant", usage: { input: 5000, output: 40, cacheRead: 3000 }, stopReason: "toolUse" })!;
  assert.deepEqual([r.ttft_ms, r.total_ms, r.images, r.input, r.cache_read, r.stop], [800, 2000, 2, 5000, 3000, "toolUse"]);
  const line = JSON.parse(readFileSync(f, "utf8").trim());
  assert.equal(line.schema, "gui-pi-latency/1");
  // no streaming update: ttft is missing, not guessed
  b.onTurnStart(); t = 4000;
  assert.equal(b.onEnd({ role: "assistant" })!.ttft_ms, null);
  assert.equal(b.quantile("total_ms", 0.5) !== null, true);
  assert.equal(b.onEnd({ role: "user" }), null);
});

test("latency: the extension writes model.jsonl next to the gate log", async () => {
  const dir = mkdtempSync(join(tmpdir(), "latx-"));
  const h = await load({ tools: ["click"] }, { GUI_PI_GATE_LOG: join(dir, "gate.jsonl") });
  await h.pi.emit("turn_start", {});
  await h.pi.emit("context", { messages: [{ role: "user", content: [{ type: "text", text: "hi" }] }] });
  await h.pi.emit("before_provider_request", { payload: {} });
  await h.pi.emit("message_update", { message: { role: "assistant" } });
  await h.pi.emit("message_end", { message: { role: "assistant", content: [], usage: { input: 10, output: 2 } } });
  assert.ok(existsSync(join(dir, "model.jsonl")));
  const l = JSON.parse(readFileSync(join(dir, "model.jsonl"), "utf8").trim());
  assert.equal(l.images, 0);
  assert.equal(typeof l.ttft_ms, "number");
  h.api.mcp.close();
});

// ---------------------------------------------------------------- 5. help popup: what was tried

test("a help request carries the client's record of failed methods when the gate sends none", async () => {
  const path = ipcPath("v032");
  const seen: any[] = [];
  const socks: any[] = [];
  const srv = createServer((sock) => {
    socks.push(sock);
    let buf = "";
    sock.on("data", (d) => {
      buf += d;
      const i = buf.indexOf("\n");
      if (i < 0) return;
      const req = JSON.parse(buf.slice(0, i));
      if (req.op === "gate_env") return sock.write(JSON.stringify({ env: {} }) + "\n");
      if (req.op === "watch_stop") return;
      seen.push(req);
      sock.write(JSON.stringify({ id: req.id, action: "accept", content: { choice: "try_other", note: "use the menu" } }) + "\n");
    });
  });
  await new Promise<void>((res) => srv.listen(path, () => res()));
  const blocked = { content: [{ type: "text", text: "[gate] Not executed: nothing happened" }], isError: true, structuredContent: { status: "blocked" } };
  const ask = { resultType: "input_required", requestState: "st1", inputRequests: { help: { method: "elicitation/create", params: {
    mode: "form", message: "stuck", requestedSchema: {}, _meta: { "io.github.cszf1/gui-gate": { kind: "ask_help", timeout_s: 5 } } } } } };
  const h = await load({ tools: ["click"], results: { click: [blocked, blocked, ask, { content: [{ type: "text", text: "try another way" }], structuredContent: { status: "blocked", human_choice: "another_way" } }] } },
                       { GUI_PI_HUMAN_IPC: path, GUI_PI_HUMAN_TOKEN: "tok" });
  await h.pi.call("click", { element_token: "s1:1" });
  await h.pi.call("click", { element_token: "s1:2" });
  await h.pi.call("click", { element_token: "s1:3" });
  assert.equal(seen.length, 1);
  assert.equal(seen[0].kind, "ask_help");
  assert.ok(Array.isArray(seen[0].meta.client_tried) && seen[0].meta.client_tried.length === 2, JSON.stringify(seen[0].meta));
  assert.match(seen[0].meta.client_tried[0], /click element_token=s1:1: not executed/);
  h.api.mcp.close();
  for (const s of socks) s.destroy();
  srv.close();
});
