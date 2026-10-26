// gui-pi: the Pi side of the desktop computer-use agent.
//   - starts gui-gate (which starts the pinned cua-driver) and speaks MCP 2026-07-28 to it, MRTR person requests included
//   - registers the gate's tools as Pi tools (the model only writes `arguments`); client-only gate tools
//     (gate_next_step, gate_reset) and the old authorisation/config tools are never offered to the model
//   - before each model turn asks the gate what to look at (gate_next_step) and attaches that read (advisory only)
//   - reads the gate's structured status (executed / blocked / partial / unknown / input_required / done_by_person,
//     next_action rethink, human_outcome)
//   - context strategy: per window the newest observation + the diff bases it builds on, old reads summarised,
//     newest screenshot per window + pinned evidence (pin_evidence), every image stored and recallable (recall_image),
//     task facts kept across compaction, one plan state with reminders only at milestones / after compaction / on drift
//   - skills listed by name and loaded on demand (load_skill)
//   - sends the model's real token usage with each tool call (_meta gui_pi/usage)
//   - Stop from the shell: no new action, gate_reset, gate killed
// Nothing here can approve an action: that lives in gui-gate (rules, authorisation, reviewer, person).
//
// Environment (set by the desktop shell; all optional for a plain `pi -e` run):
//   GUI_PI_GATE          path to gui-gate (default: "gui-gate" on PATH)
//   GUI_PI_DRIVER        path to the bundled cua-driver (passed as --driver)
//   GUI_PI_GATE_ARGS     JSON array of extra gate arguments (--allow-config and task arguments are dropped)
//   GUI_PI_GATE_LOG      gate JSONL log path (default: none)
//   GUI_PI_TASK_FILE     the trusted task channel the shell writes (passed to the gate as --task-file; never written here)
//   GUI_PI_TASK          the user's first request, verbatim (task facts; the gate's --task only without a task file)
//   GUI_PI_SKILLS        skills root (fallback when Pi's own skill list is not available)
//   GUI_PI_HUMAN_TIMEOUT seconds a person has (default 120; timeout = refusal)
//   GUI_PI_HUMAN_IPC / GUI_PI_HUMAN_TOKEN   the shell's popup service (also hands over the gate's settings once,
//                        and tells the extension when the user pressed Stop); GUI_PI_HUMAN_SCRIPT (tests only)
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { mkdtempSync } from "node:fs";
import { McpClient, CLIENT_ONLY_TOOLS, CLIENT_META, type ToolResult } from "./src/mcp.ts";
import { prune, obsKind, obsMeta, diffNote, textOf, OBSERVE_TYPE, RECALL_TOOL } from "./src/context.ts";
import { makeAskHuman } from "./src/human.ts";
import { PersonFreeze } from "./src/freeze.ts";
import { PlanTracker, Reminder, stripGatePlan } from "./src/plan.ts";
import { interpret, refusedEffect, READ_TOOLS, HUMAN_LABEL, PERSON_SAID_NO } from "./src/status.ts";
import { TaskFacts, ENTRY_TYPE, filesOf } from "./src/facts.ts";
import { gateArgs, gateEnv, fetchGateEnv } from "./src/env.ts";
import { NextStepper, ClientTool, isClientOnly, NEXT_STEP_TOOL, type Focus } from "./src/nextstep.ts";
import { ImageStore, imageTag, PINS_ENTRY, MAX_PINS } from "./src/images.ts";
import { UsageBook } from "./src/usage.ts";
import { Secrets } from "./src/redact.ts";
import { SkillIndex, scanSkills, SECTION as SKILL_SECTION } from "./src/skills.ts";
import { watchStop } from "./src/stop.ts";
import { ActionGuard, UnknownGuard } from "./src/guard.ts";

export const VERSION = "0.3.1";
/** Tools an old gate may still list that the agent must never call (authorisation and config are not the agent's). */
export const HIDDEN_GATE_TOOLS = new Set(["gate_confirm", "gate_task", "gate_config"]);
export const NOTE_FACT = "note_fact";
export const PIN_EVIDENCE = "pin_evidence";
export const LOAD_SKILL = "load_skill";
export const RESET_TOOL = "gate_reset";
export const NEXT_STEP_ENTRY = "gui-pi-nextstep";
/** Tools the extension itself adds (the shell lists them in Pi's --tools). */
export const OWN_TOOLS = [NOTE_FACT, PIN_EVIDENCE, RECALL_TOOL, LOAD_SKILL];
export { gateArgs, UnknownGuard, ActionGuard };

/** The gate's reset / freeze result says a person is still answering one of its questions. */
export function personStillAnswering(r: ToolResult | undefined): boolean {
  if (!r) return false;
  if (r.structuredContent?.gate?.tier === "freeze") return true;
  return /A person is still answering a request from the gate|a person is answering a request from the gate right now/i.test(textItems(r));
}

/** Which window a tool call targets: "pid:window_id" when the arguments say so. */
function argWindow(args: any, snapWin: Map<string, string>): string | undefined {
  if (args?.pid && args?.window_id) return `${args.pid}:${args.window_id}`;
  const tok = String(args?.element_token || "");
  const snap = tok.split(":")[0];
  return snap ? snapWin.get(snap) : undefined;
}

function focusOf(key: string | undefined): Focus {
  const m = String(key || "").match(/^(\d+):(\d+)$/);
  return m ? { pid: Number(m[1]), window_id: Number(m[2]) } : null;
}

const textItems = (r: ToolResult) => (r.content || []).filter((c: any) => c?.type === "text").map((c: any) => c.text).join("\n");

export default async function (pi: any, envOverride?: NodeJS.ProcessEnv) {
  let ui: any = null;
  const env = envOverride || process.env;
  const secrets = new Secrets();
  secrets.add(env.GUI_PI_MAIN_KEY);
  secrets.add(env.GUI_PI_HUMAN_TOKEN);
  const human = makeAskHuman(env, () => ui);
  const freeze = new PersonFreeze();
  const mcp = new McpClient(freeze.wrap(human.ask), VERSION);
  const plan = new PlanTracker();
  const reminder = new Reminder(plan);
  const facts = new TaskFacts();
  const guard = new ActionGuard();
  const usage = new UsageBook();
  const skills = new SkillIndex();
  const snapWin = new Map<string, string>(); // snapshot id -> "pid:window_id"
  let lastWindow = "";
  let focusKey = "";
  let images: ImageStore | null = null;
  const imageStore = () => {
    if (!images) images = new ImageStore(mkdtempSync(join(tmpdir(), "gui-pi-images-")));
    return images;
  };
  facts.setRequest(env.GUI_PI_TASK || "");
  const save = (type: string, data: any) => {
    try {
      pi.appendEntry(type, secrets.any(data));
    } catch {}
  };
  const saveFacts = () => save(ENTRY_TYPE, facts.toJSON());
  const notify = (msg: string, level: string) => {
    const m = secrets.text(msg);
    try {
      ui?.notify?.(m, level);
    } catch {}
    console.error(m);
  };

  // the gate's own settings and keys come from the shell once, never through Pi's environment
  const fromShell = await fetchGateEnv(env.GUI_PI_HUMAN_IPC, env.GUI_PI_HUMAN_TOKEN || "");
  secrets.addFromEnv(fromShell);
  mcp.onExit = (code) => {
    if (mcp.stopped || mcp.closing) return;
    notify(`[gui-pi] gui-gate exited (code ${code}); no GUI action can run. ${mcp.stderr.join("").slice(-300)}`, "error");
  };
  mcp.start(env.GUI_PI_GATE || "gui-gate", gateArgs(env), gateEnv(env, fromShell));
  await mcp.discover();
  const listed = await mcp.listTools();
  const listedNames = new Set(listed.map((t: any) => String(t.name)));
  const tools = listed.filter((t: any) => !HIDDEN_GATE_TOOLS.has(String(t.name)) && !isClientOnly(t));
  const gateTools = new Set<string>(tools.map((t: any) => String(t.name)));

  // client-only gate tools, feature-detected (listed -> used; else tried once)
  const clientCall = (name: string, args: any, timeoutMs = 30_000) => mcp.callTool(name, args, undefined,
    CLIENT_ONLY_TOOLS.has(name) ? { [CLIENT_META]: true } : undefined, timeoutMs);
  const nextStep = new NextStepper((n, a) => clientCall(n, a), listedNames.has(NEXT_STEP_TOOL), (d) => {
    save(NEXT_STEP_ENTRY, { ...d, at: new Date().toISOString() });
  });
  const reset = new ClientTool(listedNames.has(RESET_TOOL));

  // Stop from the shell: nothing new runs, the gate forgets approvals and observations, then it is killed
  const onStop = async () => {
    guard.stopped = true;
    let ok = false;
    if (reset.usable && !mcp.closed) {
      try {
        const r = await clientCall(RESET_TOOL, {}, 1500);
        ok = !r.isError && (reset.state === "yes" || r.structuredContent?.reset === true);
        reset.mark(ok);
        // gate v0.10.2: a reset voids but does not withdraw a question a person is answering (input stays frozen
        // until they answer or it times out): say so instead of looking stuck
        if (personStillAnswering(r)) notify("[gui-pi] waiting for your answer: a question from the gate is still open; nothing runs until you answer or it times out.", "info");
      } catch {}
    }
    mcp.kill();
    return { reset: ok };
  };
  const stopWatch = watchStop(env.GUI_PI_HUMAN_IPC, env.GUI_PI_HUMAN_TOKEN || "", onStop);

  /** Store each image of a result, tag it with its id; returns the new content and the ids in order. */
  const ingest = (content: any[], wkey: string, tool: string) => {
    const out: any[] = [];
    const ids: string[] = [];
    for (const c of content) {
      out.push(c);
      if (c?.type === "image") {
        try {
          const id = imageStore().save(c.data, c.mimeType || "image/jpeg", wkey || "?", tool);
          ids.push(id);
          out.push({ type: "text", text: imageTag(id, wkey) });
        } catch {}
      }
    }
    return { content: out, ids };
  };

  /** Remember which window a result observed; returns its window key. */
  const learnWindow = (name: string, params: any, sc: any, text: string) => {
    let wkey: string | undefined;
    if (sc.snapshot_id && sc.pid && sc.window_id) {
      wkey = `${sc.pid}:${sc.window_id}`;
      snapWin.set(String(sc.snapshot_id), wkey);
    }
    wkey = wkey || argWindow(params, snapWin);
    const obs = obsMeta(name, text, sc);
    if (obs?.snapshot && wkey) snapWin.set(obs.snapshot, wkey);
    const title = text.match(/frame = "([^"]{0,80})"/);
    if (title) lastWindow = title[1];
    if (wkey && /^\d+:\d+$/.test(wkey)) focusKey = wkey;
    return { wkey, obs };
  };

  /** The read gate_next_step asked for, as a custom message (kept in the session like a tool result's read). */
  const attachment = async () => {
    if (guard.stopped || mcp.closed) return null;
    const a = await nextStep.before(focusOf(focusKey));
    if (!a || !a.content.length) return null;
    let details: any = { decision_id: a.decision.decision_id, observe: a.decision.observe, extra_reason: a.decision.extra_reason };
    let content = a.content;
    if (a.read) {
      const sc = a.read.structuredContent || {};
      const f = focusOf(focusKey);
      const { wkey, obs } = learnWindow("get_window_state", f || {}, sc, textItems(a.read));
      const ing = ingest(content, wkey || "?", "get_window_state");
      content = ing.content;
      details = { ...details, obs, windowKey: wkey, images: ing.ids };
      guard.onRead();
    }
    return { customType: OBSERVE_TYPE, content: secrets.content(content), display: false, details };
  };

  pi.on("turn_start", async () => freeze.newTurn());
  pi.on("message_end", async (event: any) => {
    usage.onAssistant(event?.message);
  });
  pi.on("tool_call", async (event: any) => {
    const name = String(event?.toolName || "");
    // Stop first (holds every tool), then a person answering, then the unknown / done-by-person holds
    const why = guard.stopped ? guard.check(name, gateTools, event?.input) : freeze.check(name, gateTools) || guard.check(name, gateTools, event?.input);
    if (why) return { block: true, reason: why };
  });

  for (const t of tools) {
    const name = String(t.name);
    pi.registerTool({
      name,
      label: name,
      // GUI actions share one screen and one gate session: never run two at once
      executionMode: "sequential",
      description: String(t.description || name),
      parameters: t.inputSchema && typeof t.inputSchema === "object" ? t.inputSchema : { type: "object", properties: {} },
      async execute(toolCallId: string, params: any, signal?: AbortSignal) {
        const r = await mcp.callTool(name, params, signal, usage.metaFor(toolCallId));
        const sc = r.structuredContent || {};
        const text = textItems(r);
        const { wkey, obs } = learnWindow(name, params, sc, text);

        const st = interpret(name, params, r);
        guard.onResult(name, st.status, !!r.isError, params, !!obs);
        const before = plan.counts();
        if (name === "set_plan") plan.update(params);
        let changed = false;
        if (st.refused) changed = facts.addRefused(refusedEffect(name, sc, text)) || changed;
        if (st.choice === "another_way") changed = facts.addAnswer(`${name}: the person asked to try another way (not a refusal; do not repeat this action)`) || changed;
        else if (st.human) changed = facts.addAnswer(`${sc?.human_outcome?.kind || name}: the person ${HUMAN_LABEL[st.human]} (${PERSON_SAID_NO.has(st.human) ? "refused" : "no answer"}; not done)`) || changed;
        if (st.byPerson) changed = facts.addAnswer(`${name}: a person did this step themselves (do not repeat it)`) || changed;
        if (typeof sc.answer === "string" && sc.answer.trim()) changed = facts.addAnswer(`answered: ${sc.answer}`) || changed;
        else if (text.startsWith("[human via")) changed = facts.addAnswer(text.split("\n")[0]) || changed;
        for (const f of filesOf(sc)) changed = facts.addFile(f) || changed;
        if (name === "check_done" && Array.isArray(sc.gaps) && sc.gaps.length === 0 && !r.isError)
          changed = facts.addFact("check_done found no open gaps", "check_done (gate)", true) || changed;
        if (changed) saveFacts();

        const notes = [...st.notes];
        const dn = diffNote(sc.diff);
        if (dn) notes.push(dn);
        const rem = reminder.onResult({
          tool: name,
          action: gateTools.has(name) && !READ_TOOLS.has(name) && name !== "request_human",
          refused: st.refused,
          ...(name === "set_plan" ? { before, after: plan.counts() } : {}),
        });
        if (rem) notes.push(rem);

        let content = stripGatePlan((r.content || []).filter((c: any) => c && (c.type === "text" || c.type === "image")))
          .map((c: any) => (c.type === "image" ? { type: "image", data: c.data, mimeType: c.mimeType || "image/jpeg" } : { type: "text", text: c.text || "" }));
        const ing = ingest(content, wkey || "?", name);
        content = ing.content;
        for (const n of notes) content.push({ type: "text", text: n });
        return { content: secrets.content(content.length ? content : [{ type: "text", text: "(no output)" }]),
                 isError: !!r.isError,
                 details: secrets.any({ windowKey: wkey, gateError: !!r.isError, status: st.status, executed: st.executed,
                            next_action: st.nextAction, human_outcome: st.human, human_choice: st.choice, steps: st.steps, obs, images: ing.ids, gate: sc.gate || null }) };
      },
    });
  }

  // facts with their source, kept verbatim across compaction (the agent's own note; the gate never reads it)
  pi.registerTool({
    name: NOTE_FACT,
    label: NOTE_FACT,
    executionMode: "sequential",
    description: "Write down a value you will need later (a number, a name, a path you read) with where you read it. " +
      "It is kept word for word for the rest of the task, also after older window reads are dropped. It authorises nothing.",
    parameters: { type: "object", properties: {
      fact: { type: "string", description: "the value, e.g. 'Q3 total = 48,210 (cell F12)'" },
      source: { type: "string", description: "where you read it, e.g. 'Calc window \"q3.ods\", snapshot s0000012'" } },
      required: ["fact", "source"] },
    async execute(_id: string, params: any) {
      const ok = facts.addFact(secrets.text(String(params?.fact || "")), secrets.text(String(params?.source || "")), false);
      if (ok) saveFacts();
      return { content: [{ type: "text", text: ok ? "noted" : "nothing new to note" }], details: {} };
    },
  });

  pi.registerTool({
    name: PIN_EVIDENCE,
    label: PIN_EVIDENCE,
    executionMode: "sequential",
    description: `Keep a screenshot in view as evidence (at most ${MAX_PINS}), e.g. the state before a change you must compare with, ` +
      "or data you copy from another window. Every screenshot has an id like img-12 (in the line after it). Other screenshots " +
      "leave the context when a newer one of the same window arrives; recall_image brings any of them back. unpin:true releases one.",
    parameters: { type: "object", properties: {
      id: { type: "string", description: "the screenshot id, e.g. img-12" },
      label: { type: "string", description: "what it shows and why you keep it, e.g. 'Calc totals before the edit'" },
      unpin: { type: "boolean", description: "true: release this pin" } },
      required: ["id"] },
    async execute(_id: string, params: any) {
      const st = imageStore();
      const id = String(params?.id || "").trim();
      let msg: string;
      let isError = false;
      if (params?.unpin === true) msg = st.unpin(id) ? `unpinned ${id}` : `${id} was not pinned`;
      else {
        const err = st.pin(id, String(params?.label || ""));
        if (err) {
          msg = err;
          isError = true;
        } else msg = `pinned ${id} (${st.pins.length}/${MAX_PINS})`;
      }
      save(PINS_ENTRY, st.pinsJSON());
      return { content: [{ type: "text", text: msg }], isError, details: { pins: st.pins } };
    },
  });

  pi.registerTool({
    name: RECALL_TOOL,
    label: RECALL_TOOL,
    executionMode: "sequential",
    description: "Show again a screenshot that left the context, by its id (e.g. img-7). It is the image exactly as it was taken; " +
      "the window may look different now.",
    parameters: { type: "object", properties: { id: { type: "string", description: "the screenshot id, e.g. img-7" } }, required: ["id"] },
    async execute(_id: string, params: any) {
      const id = String(params?.id || "").trim();
      const got = imageStore().get(id);
      if (!got) return { content: [{ type: "text", text: `no stored screenshot with id ${id}` }], isError: true, details: {} };
      return { content: [{ type: "text", text: `[client: ${id} as taken at ${got.meta.at} (window ${got.meta.window}, ${got.meta.tool}); ` +
                 "it may be out of date]" }, { type: "image", data: got.data, mimeType: got.meta.mimeType }],
               details: { images: [id], recalled: id } };
    },
  });

  pi.registerTool({
    name: LOAD_SKILL,
    label: LOAD_SKILL,
    executionMode: "sequential",
    description: "Load the instructions of one listed skill by name (see the skills section of the system prompt).",
    parameters: { type: "object", properties: { name: { type: "string", description: "skill name, e.g. office-libreoffice" } }, required: ["name"] },
    async execute(_id: string, params: any) {
      const body = skills.load(String(params?.name || ""));
      if (body === null)
        return { content: [{ type: "text", text: `no such skill. Available: ${[...skills.skills.keys()].join(", ") || "(none)"}` }], isError: true, details: {} };
      return { content: [{ type: "text", text: body }], details: { skill: params.name } };
    },
  });

  // a gate refusal / error keeps its attachments (fresh tree, screenshot) but is marked as an error for the model
  pi.on("tool_result", async (event: any) => {
    if (event?.details?.gateError) return { isError: true };
  });

  pi.on("session_start", async (_e: any, ctx: any) => {
    ui = ctx?.hasUI ? ctx.ui : null;
    const sm = ctx?.sessionManager;
    try {
      facts.restore(sm?.getBranch?.() || []);
    } catch {}
    facts.setRequest(env.GUI_PI_TASK || "");
    try {
      const file = sm?.getSessionFile?.();
      const base = file ? dirname(file) : sm?.getSessionDir?.();
      if (base && !images) images = new ImageStore(join(base, "gui-pi-images", String(sm?.getSessionId?.() || "session")));
    } catch {}
    try {
      imageStore().restore(sm?.getBranch?.() || []);
    } catch {}
  });

  // skills by name only (load_skill), and the gate's next-step read before the first turn of a run
  pi.on("before_agent_start", async (event: any) => {
    try {
      const opts = event?.systemPromptOptions;
      if (!skills.skills.size) skills.set(opts?.skills?.length ? opts.skills : scanSkills(env.GUI_PI_SKILLS));
      if (opts?.sections) opts.sections[SKILL_SECTION] = skills.section();
    } catch {}
    const m = await attachment();
    if (m) return { message: m };
  });

  // before every later turn: the next-step check, attached as a session entry the next request sees
  pi.on("turn_end", async (event: any) => {
    if (!(event?.toolResults?.length > 0)) return;
    const m = await attachment();
    if (!m) return;
    return { entries: [...(event.entries || []), { type: "custom_message", ...m }] };
  });

  pi.on("context", async (event: any) => {
    return { messages: prune(event.messages, { pinned: new Set(imageStore().pins.map((p) => p.id)) }) };
  });

  pi.on("session_before_compact", async (event: any) => {
    const prep = event.preparation || {};
    if (!facts.request) {
      const firstUser = (event.branchEntries || []).map((e: any) => e?.message).find((m: any) => m?.role === "user");
      if (firstUser) facts.setRequest(textOf(firstUser).slice(0, 2000));
    }
    reminder.afterCompaction();
    const pins = imageStore().pins;
    const parts = [
      "[gui-pi compaction] Older window reads were dropped; the gate still holds this run's state (refused effects, history). " +
        "Everything below is kept word for word by the program.",
      facts.render(),
      plan.summary(),
      pins.length ? "Pinned evidence (recall_image shows it): " + pins.map((p) => `${p.id} "${p.label}"`).join("; ") : "",
      lastWindow ? `Last window seen: "${lastWindow}". Read it again before the next action.` : "Read the window again before the next action.",
    ].filter(Boolean);
    return { compaction: { summary: secrets.text(parts.join("\n\n")), firstKeptEntryId: prep.firstKeptEntryId, tokensBefore: prep.tokensBefore,
                           details: { facts: secrets.any(facts.toJSON()) } } };
  });

  pi.on("session_shutdown", async () => {
    stopWatch.close();
    mcp.close();
  });

  return { mcp, guard, images: imageStore, nextStep, reset, onStop, secrets };
}

export { obsKind, CLIENT_ONLY_TOOLS };
