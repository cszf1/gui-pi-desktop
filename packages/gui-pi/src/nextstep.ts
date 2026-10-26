// Before each model turn: ask the gate what the next turn should look at (gate >= 0.10.1, client-only tool
// `gate_next_step`). The answer is advisory and grants nothing:
//   structuredContent {observe: "none" | "tree" | "tree+screenshot", extra_reason: bool, reason, decision_id}
// observe "tree" / "tree+screenshot": the extension reads the focused window itself (get_window_state, with the
// screenshot for the second) and attaches that read to the next turn; extra_reason: a fixed note asking the model
// to think the next step through again (regenerate the candidate; no authorisation follows from it).
// Feature detection: listed in tools/list -> used; not listed -> tried once; a reply without a valid `observe`
// (an older gate passes unknown names to the driver) -> never tried again, and the turn goes on as before.
// 0.3.2: the read is skipped when the focused window's newest observation is still fresh (observe.ts: the same
// judgement the automatic observation in an action's result counts for), and the client reads the whole window
// itself (an anchor) when its diff chain is too long, its base is missing, after a compaction or after a person acted.
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
  /** the gate's decision (null: no gate_next_step, but the client itself needed an anchor read) */
  decision: NextStep;
  /** content for the model (empty: nothing to attach, the decision is only logged) */
  content: any[];
  /** the read that was attached (tool result of get_window_state), if any */
  read: ToolResult | null;
  /** the read the gate asked for was skipped: the window's newest observation is still fresh (observe.ts) */
  skipped?: "fresh";
  /** why the client read the whole window itself (anchor: long diff chain, missing base, compaction, a person acted) */
  anchor?: string;
};

export type BeforeOpts = {
  /** is the focused window's newest observation still fresh (with a screenshot when asked)? */
  fresh?: (wantScreenshot: boolean) => boolean;
  /** the client needs a full read of the focused window regardless of the gate's answer (observe.ts anchor) */
  anchor?: string | null;
  /** also list the windows (a person acted: they may have opened / closed / switched windows) */
  listWindows?: boolean;
};

const NO_DECISION: NextStep = { observe: "none", extra_reason: false, reason: "", decision_id: "" };

export class NextStepper {
  tool: ClientTool;
  call: Call;
  log: (d: NextStep & { window_id?: number; skipped?: string; anchor?: string }) => void;
  constructor(call: Call, listed: boolean, log: (d: NextStep & { window_id?: number; skipped?: string; anchor?: string }) => void = () => {}) {
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
    if (d) this.tool.mark(true);
    return d;
  }

  /** What to attach before the next model turn; null = nothing (old gate and no anchor needed, or nothing to say). */
  async before(focus: Focus, opts: BeforeOpts = {}): Promise<Attachment | null> {
    const got = await this.decide(focus);
    const anchor = focus && opts.anchor ? opts.anchor : null;
    if (!got && !anchor) return null;
    const d = got ?? NO_DECISION;
    let observe: Observe = d.observe;
    if (anchor && observe === "none") observe = "tree";
    let skipped: "fresh" | undefined;
    if (!anchor && observe !== "none" && opts.fresh?.(observe === "tree+screenshot")) {
      skipped = "fresh";
      observe = "none";
    }
    if (got) this.log({ ...d, ...(focus ? { window_id: focus.window_id } : {}), ...(skipped ? { skipped } : {}), ...(anchor ? { anchor } : {}) });
    const content: any[] = [];
    let read: ToolResult | null = null;
    if (observe !== "none" && focus) {
      try {
        read = await this.call("get_window_state", { pid: focus.pid, window_id: focus.window_id,
                                                     include_screenshot: observe === "tree+screenshot" });
      } catch {
        read = null;
      }
      if (read && !read.isError) {
        const why = anchor
          ? `[client: a fresh full read of the focused window before this turn (${anchor}); it is the new base for later diffs. It is attached below.]`
          : `[client: the gate's next-step check${d.decision_id ? ` ${d.decision_id}` : ""} ` +
            `asked for a fresh read (${observe}) of the focused window before this turn${d.reason ? `: ${d.reason}` : ""}. It is attached below.]`;
        content.push({ type: "text", text: why });
        for (const c of read.content || []) {
          if (c?.type === "text") content.push({ type: "text", text: String(c.text || "") });
          else if (c?.type === "image") content.push({ type: "image", data: c.data, mimeType: c.mimeType || "image/jpeg" });
        }
        if (opts.listWindows) {
          try {
            const lw = await this.call("list_windows", {});
            if (lw && !lw.isError) {
              content.push({ type: "text", text: "[client: the open windows now (a person acted; they may have switched windows):]" });
              for (const c of lw.content || []) if (c?.type === "text") content.push({ type: "text", text: String(c.text || "") });
            }
          } catch {}
        }
      } else read = null;
    }
    if (d.extra_reason) content.push({ type: "text", text: EXTRA_REASON_NOTE });
    return { decision: d, content, read, ...(skipped ? { skipped } : {}), ...(anchor ? { anchor } : {}) };
  }
}
