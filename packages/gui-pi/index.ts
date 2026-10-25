// gui-pi: the Pi side of the desktop computer-use agent.
//   - starts gui-gate (which starts the pinned cua-driver) and speaks MCP 2026-07-28 to it, MRTR person requests included
//   - registers the gate's tools as Pi tools (the model only writes `arguments`)
//   - reads the gate's structured status (executed / blocked / partial / unknown / input_required, human_outcome)
//   - context strategy: per window the newest observation + the diff bases it builds on, old reads summarised,
//     newest screenshot per window, task facts kept across compaction, plan reminders only at milestones
// Nothing here can approve an action: that lives in gui-gate (rules, authorisation, reviewer, person).
//
// Environment (set by the desktop shell; all optional for a plain `pi -e` run):
//   GUI_PI_GATE          path to gui-gate (default: "gui-gate" on PATH)
//   GUI_PI_DRIVER        path to the bundled cua-driver (passed as --driver)
//   GUI_PI_GATE_ARGS     JSON array of extra gate arguments (--allow-config is dropped)
//   GUI_PI_GATE_LOG      gate JSONL log path (default: none)
//   GUI_PI_TASK          the user's request, verbatim (given to the gate as the operator's words)
//   GUI_PI_HUMAN_TIMEOUT seconds a person has (default 120; timeout = refusal)
//   GUI_PI_HUMAN_IPC / GUI_PI_HUMAN_TOKEN   the shell's popup service (also hands over the gate's settings once);
//   GUI_PI_HUMAN_SCRIPT (tests only)
import { McpClient } from "./src/mcp.ts";
import { prune, obsKind, obsMeta, diffNote, textOf } from "./src/context.ts";
import { makeAskHuman } from "./src/human.ts";
import { PersonFreeze } from "./src/freeze.ts";
import { PlanTracker, Reminder } from "./src/plan.ts";
import { interpret, refusedEffect, READ_TOOLS, HUMAN_LABEL } from "./src/status.ts";
import { TaskFacts, ENTRY_TYPE, filesOf } from "./src/facts.ts";
import { gateArgs, gateEnv, fetchGateEnv } from "./src/env.ts";

const VERSION = "0.2.0";
/** Tools an old gate may still list that the agent must never call (authorisation and config are not the agent's). */
export const HIDDEN_GATE_TOOLS = new Set(["gate_confirm", "gate_task", "gate_config"]);
export const NOTE_FACT = "note_fact";
export { gateArgs };

/** Which window a tool call targets: "pid:window_id" when the arguments say so. */
function argWindow(args: any, snapWin: Map<string, string>): string | undefined {
  if (args?.pid && args?.window_id) return `${args.pid}:${args.window_id}`;
  const tok = String(args?.element_token || "");
  const snap = tok.split(":")[0];
  return snap ? snapWin.get(snap) : undefined;
}

/** Holds GUI actions after an outcome-unknown result until the model has read a window again. */
export class UnknownGuard {
  pending = false;
  onResult(tool: string, status: string, isError: boolean) {
    if (status === "unknown") this.pending = true;
    else if (tool.endsWith("get_window_state") && !isError) this.pending = false;
  }
  check(tool: string, gateTools: Set<string>): string | null {
    if (!this.pending || !gateTools.has(tool) || READ_TOOLS.has(tool)) return null;
    return "Held: the outcome of an earlier step is unknown. Read the window first (get_window_state) and decide from what it shows; do not resend that step blind.";
  }
}

export default async function (pi: any) {
  let ui: any = null;
  const env = process.env;
  const human = makeAskHuman(env, () => ui);
  const freeze = new PersonFreeze();
  const mcp = new McpClient(freeze.wrap(human.ask), VERSION);
  const plan = new PlanTracker();
  const reminder = new Reminder(plan);
  const facts = new TaskFacts();
  const unknown = new UnknownGuard();
  const snapWin = new Map<string, string>(); // snapshot id -> "pid:window_id"
  let lastWindow = "";
  facts.setRequest(env.GUI_PI_TASK || "");
  const saveFacts = () => {
    try {
      pi.appendEntry(ENTRY_TYPE, facts.toJSON());
    } catch {}
  };

  // the gate's own settings and keys come from the shell once, never through Pi's environment
  const fromShell = await fetchGateEnv(env.GUI_PI_HUMAN_IPC, env.GUI_PI_HUMAN_TOKEN || "");
  mcp.onExit = (code) => {
    const msg = `[gui-pi] gui-gate exited (code ${code}); no GUI action can run. ${mcp.stderr.join("").slice(-300)}`;
    try {
      ui?.notify?.(msg, "error");
    } catch {}
    console.error(msg);
  };
  mcp.start(env.GUI_PI_GATE || "gui-gate", gateArgs(env), gateEnv(env, fromShell));
  await mcp.discover();
  const tools = (await mcp.listTools()).filter((t: any) => !HIDDEN_GATE_TOOLS.has(String(t.name)));

  const gateTools = new Set<string>(tools.map((t: any) => String(t.name)));
  pi.on("turn_start", async () => freeze.newTurn());
  pi.on("tool_call", async (event: any) => {
    const name = String(event?.toolName || "");
    const why = freeze.check(name, gateTools) || unknown.check(name, gateTools);
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
      async execute(_id: string, params: any, signal?: AbortSignal) {
        const r = await mcp.callTool(name, params, signal);
        const sc = r.structuredContent || {};
        // remember which window each snapshot came from (for the context anchors)
        let wkey: string | undefined;
        if (sc.snapshot_id && sc.pid && sc.window_id) {
          wkey = `${sc.pid}:${sc.window_id}`;
          snapWin.set(String(sc.snapshot_id), wkey);
        }
        wkey = wkey || argWindow(params, snapWin);
        const text = (r.content || []).filter((c: any) => c?.type === "text").map((c: any) => c.text).join("\n");
        const obs = obsMeta(name, text, sc);
        if (obs?.snapshot && wkey) snapWin.set(obs.snapshot, wkey);
        const title = text.match(/frame = "([^"]{0,80})"/);
        if (title) lastWindow = title[1];

        const st = interpret(name, params, r);
        unknown.onResult(name, st.status, !!r.isError);
        const before = plan.counts();
        if (name === "set_plan") plan.update(params);
        let changed = false;
        if (st.refused) changed = facts.addRefused(refusedEffect(name, sc, text)) || changed;
        if (st.human) changed = facts.addAnswer(`${sc?.human_outcome?.kind || name}: the person ${HUMAN_LABEL[st.human]} (refused)`) || changed;
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
        if (rem && !text.includes("[plan]")) notes.push(rem);

        const content = (r.content || []).filter((c: any) => c && (c.type === "text" || c.type === "image"))
          .map((c: any) => (c.type === "image" ? { type: "image", data: c.data, mimeType: c.mimeType || "image/jpeg" } : { type: "text", text: c.text || "" }));
        for (const n of notes) content.push({ type: "text", text: n });
        return { content: content.length ? content : [{ type: "text", text: "(no output)" }],
                 isError: !!r.isError,
                 details: { windowKey: wkey, gateError: !!r.isError, status: st.status, executed: st.executed,
                            human_outcome: st.human, steps: st.steps, obs, gate: sc.gate || null } };
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
      const ok = facts.addFact(String(params?.fact || ""), String(params?.source || ""), false);
      if (ok) saveFacts();
      return { content: [{ type: "text", text: ok ? "noted" : "nothing new to note" }], details: {} };
    },
  });

  // a gate refusal / error keeps its attachments (fresh tree, screenshot) but is marked as an error for the model
  pi.on("tool_result", async (event: any) => {
    if (event?.details?.gateError) return { isError: true };
  });

  pi.on("session_start", async (_e: any, ctx: any) => {
    ui = ctx?.hasUI ? ctx.ui : null;
    try {
      facts.restore(ctx?.sessionManager?.getBranch?.() || []);
    } catch {}
    facts.setRequest(env.GUI_PI_TASK || "");
  });

  pi.on("context", async (event: any) => {
    return { messages: prune(event.messages) };
  });

  pi.on("session_before_compact", async (event: any) => {
    const prep = event.preparation || {};
    if (!facts.request) {
      const firstUser = (event.branchEntries || []).map((e: any) => e?.message).find((m: any) => m?.role === "user");
      if (firstUser) facts.setRequest(textOf(firstUser).slice(0, 2000));
    }
    const parts = [
      "[gui-pi compaction] Older window reads were dropped; the gate still holds this run's state (refused effects, plan, history). " +
        "Everything below is kept word for word by the program.",
      facts.render(),
      plan.summary(),
      lastWindow ? `Last window seen: "${lastWindow}". Read it again before the next action.` : "Read the window again before the next action.",
    ].filter(Boolean);
    return { compaction: { summary: parts.join("\n\n"), firstKeptEntryId: prep.firstKeptEntryId, tokensBefore: prep.tokensBefore,
                           details: { facts: facts.toJSON() } } };
  });

  pi.on("session_shutdown", async () => mcp.close());
}

export { obsKind };
