// Context strategy for GUI runs (Pi `context` event: non-destructive, the session file keeps everything).
//
// The gate already sends trees as DIFFS: after an action, "[gate] Window right after the action" lists only rows
// that changed since the last tree of that window the agent saw ("Only rows that changed since snapshot X" /
// "Nothing in the tree changed since snapshot X"), and unchanged rows keep their [N]. So the client must keep the
// newest FULL tree of each window (its anchor) verbatim while later diffs build on it. Rules:
//   1. per window: the newest full tree is the anchor; it and every diff after it stay as they are;
//   2. every observation of that window before its anchor becomes a one-line summary (no screenshot);
//   3. per window only its newest screenshot is kept, at most MAX_IMAGES windows (the most recent ones); older
//      images become a one-line note. Images whose window is unknown count as one window "?";
//   4. text before the observation marker (what the action did, gate notes) is kept, capped.
// Observations whose window is unknown fall back to "newest one stays full" (bench/pi_ext/obs_prune.ts).

export const MARKERS = ["[gate] Window right after the action", "[gate] Window now (nothing was executed)"];
const DIFF_RX = /Only rows that changed since snapshot|Nothing in the tree changed since snapshot/;
const HEAD_KEEP = 600;
export const MAX_IMAGES = 3;

export type Msg = { role: string; toolName?: string; content?: any[]; details?: any; [k: string]: any };

export function textOf(m: Msg): string {
  return (m.content || []).filter((c: any) => c && c.type === "text").map((c: any) => c.text || "").join("\n");
}

export function hasImage(m: Msg): boolean {
  return (m.content || []).some((c: any) => c && c.type === "image");
}

/** "full" | "diff" | null: what kind of window observation this tool result carries. */
export function obsKind(m: Msg): "full" | "diff" | null {
  if (!m || m.role !== "toolResult") return null;
  const t = textOf(m);
  const name = String(m.toolName || "");
  const isRead = name.endsWith("get_window_state");
  const hasMarker = MARKERS.some((k) => t.includes(k));
  if (!isRead && !hasMarker) return null;
  if (hasMarker && DIFF_RX.test(t.slice(t.indexOf("[gate] Window")))) return "diff";
  return "full";
}

/** The window an observation belongs to: details.windowKey set by the tool wrapper, else parsed from the text. */
export function windowKey(m: Msg): string {
  const k = m.details?.windowKey;
  if (k) return String(k);
  const t = textOf(m);
  const w = t.match(/window_id=(\d+)\s+pid=(\d+)/);
  if (w) return `${w[2]}:${w[1]}`;
  return "?";
}

export function summary(text: string): string {
  const title = (text.match(/(?:frame|dialog|window) = "([^"]{0,80})/) || [])[1];
  const snap = (text.match(/snapshot_id=(\S+)/) || text.match(/\(snapshot (s[0-9a-f]+)/) || [])[1];
  const rows = (text.match(/^\s*- \[\d+\]/gm) || []).length;
  return `[client: older window read summarised - "${title || "?"}", snapshot ${snap || "?"}, ${rows} rows; a newer read of this window is current]`;
}

function pruneText(text: string, name?: string): string {
  for (const k of MARKERS) {
    const i = text.indexOf(k);
    if (i >= 0) {
      const head = text.slice(0, i).trimEnd();
      return (head.length > HEAD_KEEP ? head.slice(0, HEAD_KEEP) + " ..." : head) + (head ? "\n" : "") + summary(text.slice(i));
    }
  }
  if (String(name || "").endsWith("get_window_state")) return summary(text);
  return text;
}

export function prune(messages: Msg[]): Msg[] {
  // anchors: newest full observation per window
  const anchor = new Map<string, number>();
  let newestUnknown = -1;
  messages.forEach((m, i) => {
    const k = obsKind(m);
    if (!k) return;
    const w = windowKey(m);
    if (w === "?") newestUnknown = i;
    else if (k === "full") anchor.set(w, i);
  });
  const old = new Set<number>();
  messages.forEach((m, i) => {
    const k = obsKind(m);
    if (!k) return;
    const w = windowKey(m);
    if (w === "?") {
      if (i < newestUnknown) old.add(i);
      return;
    }
    const a = anchor.get(w);
    if (a !== undefined && i < a) old.add(i);
  });
  // newest image per window, newest MAX_IMAGES windows only
  const imgByWin = new Map<string, number>();
  messages.forEach((m, i) => {
    if (!old.has(i) && hasImage(m)) imgByWin.set(windowKey(m), i);
  });
  const keepImg = new Set([...imgByWin.values()].sort((a, b) => b - a).slice(0, MAX_IMAGES));
  return messages.map((m, i) => {
    const isOld = old.has(i);
    const dropImg = hasImage(m) && !keepImg.has(i);
    if (!isOld && !dropImg) return m;
    const content: any[] = [];
    let dropped = 0;
    for (const c of m.content || []) {
      if (c && c.type === "image") {
        if (isOld || dropImg) {
          dropped++;
          continue;
        }
      }
      if (isOld && c && c.type === "text") content.push({ ...c, text: pruneText(c.text || "", m.toolName) });
      else content.push(c);
    }
    if (dropped && !isOld) content.push({ type: "text", text: "[client: an older screenshot was removed; the newest one of this window is current]" });
    return { ...m, content };
  });
}

/** Append a short plan line to the newest tool result (read-only reminder; the gate owns the plan). */
export function withPlanLine(messages: Msg[], line: string | null): Msg[] {
  if (!line) return messages;
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (m.role === "toolResult") {
      if (textOf(m).includes("[plan]")) return messages; // the gate already printed its own progress line
      const out = messages.slice();
      out[i] = { ...m, content: [...(m.content || []), { type: "text", text: line }] };
      return out;
    }
    if (m.role === "user") return messages; // a fresh user turn: no reminder needed
  }
  return messages;
}
