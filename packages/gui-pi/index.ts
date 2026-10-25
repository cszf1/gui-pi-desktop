// gui-pi: the Pi side of the desktop computer-use agent.
//   - starts gui-gate (which starts the pinned cua-driver) and speaks MCP 2026-07-28 to it, MRTR person requests included
//   - registers the gate's tools as Pi tools (the model only writes `arguments`)
//   - context strategy: window anchors + the gate's diffs, old reads summarised, newest screenshot only, plan line
//   - deterministic compaction summary (request, plan, refusals, person answers, last window)
// Nothing here can approve an action: that lives in gui-gate (rules, authorisation, reviewer, person).
//
// Environment (set by the desktop shell; all optional for a plain `pi -e` run):
//   GUI_PI_GATE          path to gui-gate (default: "gui-gate" on PATH)
//   GUI_PI_DRIVER        path to the bundled cua-driver (passed as --driver)
//   GUI_PI_GATE_ARGS     JSON array of extra gate arguments
//   GUI_PI_GATE_LOG      gate JSONL log path (default: none)
//   GUI_PI_TASK          the user's request, verbatim (given to the gate as the operator's words)
//   GUI_PI_HUMAN_TIMEOUT seconds a person has (default 120; timeout = refusal)
//   GUI_PI_HUMAN_IPC / GUI_PI_HUMAN_TOKEN   the shell's popup service; GUI_PI_HUMAN_SCRIPT (tests only)
import { McpClient } from "./src/mcp.ts";
import { prune, withPlanLine, obsKind, textOf } from "./src/context.ts";
import { makeAskHuman } from "./src/human.ts";
import { PersonFreeze } from "./src/freeze.ts";
import { PlanTracker } from "./src/plan.ts";

const VERSION = "0.1.0";

export function gateArgs(env: NodeJS.ProcessEnv): string[] {
  const a = ["--human", "elicit", "--human-timeout", String(Number(env.GUI_PI_HUMAN_TIMEOUT) || 120)];
  a.push("--log", env.GUI_PI_GATE_LOG || "none");
  if (env.GUI_PI_DRIVER) a.push("--driver", env.GUI_PI_DRIVER);
  if (env.GUI_PI_TASK) a.push("--task", env.GUI_PI_TASK);
  if (env.GUI_PI_GATE_ARGS) {
    try {
      const extra = JSON.parse(env.GUI_PI_GATE_ARGS);
      if (Array.isArray(extra)) a.push(...extra.map(String));
    } catch {}
  }
  return a;
}

/** Which window a tool call targets: "pid:window_id" when the arguments say so. */
function argWindow(args: any, snapWin: Map<string, string>): string | undefined {
  if (args?.pid && args?.window_id) return `${args.pid}:${args.window_id}`;
  const tok = String(args?.element_token || "");
  const snap = tok.split(":")[0];
  return snap ? snapWin.get(snap) : undefined;
}

export default async function (pi: any) {
  let ui: any = null;
  const env = process.env;
  const human = makeAskHuman(env, () => ui);
  const freeze = new PersonFreeze();
  const mcp = new McpClient(freeze.wrap(human.ask), VERSION);
  const plan = new PlanTracker();
  const snapWin = new Map<string, string>(); // snapshot id -> "pid:window_id"
  const refusals: string[] = [];
  const answers: string[] = [];
  let lastWindow = "";

  mcp.start(env.GUI_PI_GATE || "gui-gate", gateArgs(env), { ...env, GUI_GATE_NO_UPDATE_CHECK: "1" });
  await mcp.discover();
  const tools = await mcp.listTools();

  const gateTools = new Set<string>(tools.map((t: any) => String(t.name)));
  pi.on("turn_start", async () => freeze.newTurn());
  pi.on("tool_call", async (event: any) => {
    const why = freeze.check(String(event?.toolName || ""), gateTools);
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
        const after = text.match(/\(snapshot (s[0-9a-f]+);/);
        if (after && wkey) snapWin.set(after[1], wkey);
        const title = text.match(/frame = "([^"]{0,80})"/);
        if (title) lastWindow = title[1];
        if (name === "set_plan") plan.update(params);
        // the gate's structured status decides, never the "[gate]" text prefix (it also appears in post-action notes)
        const status = String(sc.status || (r.isError ? "blocked" : "executed"));
        if (status === "blocked" && /refus|declin|reviewer|risk/i.test(text)) refusals.push(text.split("\n")[0].slice(0, 200));
        if (text.startsWith("[human via")) answers.push(text.slice(0, 200));
        const content = (r.content || []).filter((c: any) => c && (c.type === "text" || c.type === "image"))
          .map((c: any) => (c.type === "image" ? { type: "image", data: c.data, mimeType: c.mimeType || "image/jpeg" } : { type: "text", text: c.text || "" }));
        return { content: content.length ? content : [{ type: "text", text: "(no output)" }],
                 isError: !!r.isError,
                 details: { windowKey: wkey, gateError: !!r.isError, status, gate: sc.gate || null } };
      },
    });
  }

  // a gate refusal / error keeps its attachments (fresh tree, screenshot) but is marked as an error for the model
  pi.on("tool_result", async (event: any) => {
    if (event?.details?.gateError) return { isError: true };
  });

  pi.on("session_start", async (_e: any, ctx: any) => {
    ui = ctx?.hasUI ? ctx.ui : null;
  });

  pi.on("context", async (event: any) => {
    return { messages: withPlanLine(prune(event.messages), plan.line()) };
  });

  pi.on("session_before_compact", async (event: any) => {
    const prep = event.preparation || {};
    const firstUser = (event.branchEntries || []).map((e: any) => e?.message).find((m: any) => m?.role === "user");
    const request = env.GUI_PI_TASK || (firstUser ? textOf(firstUser).slice(0, 2000) : "");
    const parts = [
      "[gui-pi compaction] Older window reads were dropped; the gate still holds this run's state (refused effects, plan, history).",
      `User's request (verbatim): ${request || "(see first message)"}`,
      plan.summary(),
      refusals.length ? `Refused by the gate (do not retry the same effect):\n- ${refusals.slice(-10).join("\n- ")}` : "",
      answers.length ? `What the person said:\n- ${answers.slice(-10).join("\n- ")}` : "",
      lastWindow ? `Last window seen: "${lastWindow}". Read it again before the next action.` : "",
    ].filter(Boolean);
    return { compaction: { summary: parts.join("\n\n"), firstKeptEntryId: prep.firstKeptEntryId, tokensBefore: prep.tokensBefore } };
  });

  pi.on("session_shutdown", async () => mcp.close());
}

export { obsKind };
