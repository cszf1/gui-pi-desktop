// Before each model turn: ask the gate what the next turn should look at (gate >= 0.10.1, client-only tool
// `gate_next_step`). The answer is advisory and grants nothing:
//   structuredContent {observe: "none" | "tree" | "tree+screenshot", extra_reason: bool, reason, decision_id}
// observe "tree" / "tree+screenshot": the extension reads the focused window itself (get_window_state, with the
// screenshot for the second) and attaches that read to the next turn; extra_reason: a fixed note asking the model
// to think the next step through again (regenerate the candidate; no authorisation follows from it).
// Feature detection: listed in tools/list -> used; not listed -> tried once; a reply without a valid `observe`
// (an older gate passes unknown names to the driver) -> never tried again, and the turn goes on as before.
import { CLIENT_ONLY_TOOLS, type ToolResult } from "./mcp.ts";

export const NEXT_STEP_TOOL = "gate_next_step";
export const OBSERVE = ["none", "tree", "tree+screenshot"] as const;
export type Observe = (typeof OBSERVE)[number];
export type NextStep = { observe: Observe; extra_reason: boolean; reason: string; decision_id: string };

/** Fixed text, the same every time (it must not become a channel for anything else). */
export const EXTRA_REASON_NOTE =
  "[client: the gate suggests more thought before the next step. Do not just send the step you had in mind: " +
  "look at the current window again, check that the target is the right one and that the step serves the user's request, " +
  "and generate your next action afresh. This note authorises nothing.]";

export type Call = (name: string, args: any) => Promise<ToolResult>;

export function parseNextStep(r: ToolResult | null | undefined): NextStep | null {
  if (!r || r.isError) return null;
  const sc = r.structuredContent || {};
  const observe = String(sc.observe ?? "");
  if (!(OBSERVE as readonly string[]).includes(observe)) return null;
  return {
    observe: observe as Observe,
    extra_reason: sc.extra_reason === true,
    reason: String(sc.reason ?? "").slice(0, 300),
    decision_id: String(sc.decision_id ?? ""),
  };
}

/** Is this tool one only the client may call (by name, or marked client-only by the gate in tools/list)? */
export function isClientOnly(t: any): boolean {
  if (CLIENT_ONLY_TOOLS.has(String(t?.name))) return true;
  const m = t?._meta?.["io.github.cszf1/gui-gate"] || {};
  return m.client_only === true || m.audience === "client";
}

/** Feature-detected client-only gate tool: "yes" once a valid reply came back, "no" after an invalid one. */
export class ClientTool {
  state: "untried" | "yes" | "no";
  constructor(listed: boolean) {
    this.state = listed ? "yes" : "untried";
  }
  get usable() {
    return this.state !== "no";
  }
  mark(ok: boolean) {
    if (ok) this.state = "yes";
    else this.state = "no";
  }
}

export type Focus = { pid: number; window_id: number } | null;

export type Attachment = {
  decision: NextStep;
  /** content for the model (empty: nothing to attach, the decision is only logged) */
  content: any[];
  /** the read that was attached (tool result of get_window_state), if any */
  read: ToolResult | null;
};

export class NextStepper {
  tool: ClientTool;
  call: Call;
  log: (d: NextStep & { window_id?: number }) => void;
  constructor(call: Call, listed: boolean, log: (d: NextStep & { window_id?: number }) => void = () => {}) {
    this.call = call;
    this.tool = new ClientTool(listed);
    this.log = log;
  }

  async decide(focus: Focus): Promise<NextStep | null> {
    if (!this.tool.usable) return null;
    let r: ToolResult | null = null;
    try {
      r = await this.call(NEXT_STEP_TOOL, focus ? { window_id: focus.window_id } : {});
    } catch {
      r = null;
    }
    const d = parseNextStep(r);
    // only a malformed reply switches the feature off; a gate that is gone is reported elsewhere
    if (!d && !(r?.structuredContent?.gate_lost || r?.structuredContent?.stopped)) this.tool.mark(false);
    if (d) {
      this.tool.mark(true);
      this.log({ ...d, ...(focus ? { window_id: focus.window_id } : {}) });
    }
    return d;
  }

  /** What to attach before the next model turn; null = nothing (old gate, or the gate says look at nothing). */
  async before(focus: Focus): Promise<Attachment | null> {
    const d = await this.decide(focus);
    if (!d) return null;
    const content: any[] = [];
    let read: ToolResult | null = null;
    if (d.observe !== "none" && focus) {
      try {
        read = await this.call("get_window_state", { pid: focus.pid, window_id: focus.window_id,
                                                     include_screenshot: d.observe === "tree+screenshot" });
      } catch {
        read = null;
      }
      if (read && !read.isError) {
        content.push({ type: "text", text: `[client: the gate's next-step check${d.decision_id ? ` ${d.decision_id}` : ""} ` +
          `asked for a fresh read (${d.observe}) of the focused window before this turn${d.reason ? `: ${d.reason}` : ""}. It is attached below.]` });
        for (const c of read.content || []) {
          if (c?.type === "text") content.push({ type: "text", text: String(c.text || "") });
          else if (c?.type === "image") content.push({ type: "image", data: c.data, mimeType: c.mimeType || "image/jpeg" });
        }
      } else read = null;
    }
    if (d.extra_reason) content.push({ type: "text", text: EXTRA_REASON_NOTE });
    return { decision: d, content, read };
  }
}
