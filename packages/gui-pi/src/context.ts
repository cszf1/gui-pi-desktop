// Context strategy for GUI runs (Pi `context` event: non-destructive, the session file keeps everything).
//
// The gate is the one owner of tree diffs. After an action it sends either a full tree or a DIFF against an earlier
// snapshot of that window: new gate structuredContent.diff = {base_snapshot_id, snapshot_id, changed, removed,
// reordered}; old gate text "Only rows that changed since snapshot X" / "Nothing in the tree changed since snapshot X".
// Rows not in a diff keep their [N] from the base. So the client keeps, per window, the newest observation and the
// chain of bases it builds on, verbatim:
//   1. per window: the newest observation is current; follow base_snapshot_id back until a full tree; every
//      observation on that chain stays as it is;
//   2. a diff whose base is no longer in context (compacted away) stays full and gets a note: rows not listed
//      are unknown, re-read before using other tokens;
//   3. every other observation of that window becomes a one-line summary (no screenshot);
//   4. screenshots: per window only its newest one, at most MAX_IMAGES windows (the most recent ones), plus every
//      image the model pinned as evidence (pin_evidence, at most 3), plus the newest recall_image result. Every
//      other image becomes a one-line note naming its id, so recall_image can bring it back (images.ts keeps the
//      bytes). Images whose window is unknown count as one window "?";
//   5. text before the observation marker (what the action did, gate notes, client notes) is kept, capped.
// Removed and reordered rows are made explicit on the diff itself (diffNote, added when the result arrives).
// Observations whose window is unknown fall back to "newest one stays full".

export const MARKERS = ["[gate] Window right after the action", "[gate] Window now (nothing was executed)"];

/** Where a gate marker starts a line (the gate's own text). gate v0.10.2 prefixes app / page / terminal text that
 *  starts like a gate line with "(app text) ", so a marker in the middle of a line is never the gate's. -1 if none. */
export function markerAt(text: string, k: string): number {
  let i = text.indexOf(k);
  while (i >= 0) {
    if (i === 0 || text[i - 1] === "\n") return i;
    i = text.indexOf(k, i + 1);
  }
  return -1;
}
/** Custom message the extension attaches before a turn (gate_next_step): a window read like a tool result's. */
export const OBSERVE_TYPE = "gui-pi-observe";
export const RECALL_TOOL = "recall_image";
const DIFF_RX = /Only rows that changed since snapshot|Nothing in the tree changed since snapshot/;
const BASE_RX = /changed since snapshot ([A-Za-z0-9_.-]+)/;
const HEAD_KEEP = 600;
export const MAX_IMAGES = 3;

export type Msg = { role: string; toolName?: string; content?: any[]; details?: any; [k: string]: any };
export type ObsMeta = { kind: "full" | "diff"; snapshot: string | null; base: string | null };

export function textOf(m: Msg): string {
  return (m.content || []).filter((c: any) => c && c.type === "text").map((c: any) => c.text || "").join("\n");
}

export function hasImage(m: Msg): boolean {
  return (m.content || []).some((c: any) => c && c.type === "image");
}

/** What a tool result observed, from the gate's structured diff when present, else from its text. */
export function obsMeta(name: string, text: string, sc: any = {}): ObsMeta | null {
  const isRead = String(name || "").endsWith("get_window_state");
  const mi = MARKERS.map((k) => markerAt(text, k)).filter((i) => i >= 0);
  const hasMarker = mi.length > 0;
  const diff = sc && typeof sc.diff === "object" && sc.diff ? sc.diff : null;
  if (!isRead && !hasMarker && !diff) return null;
  const tail = hasMarker ? text.slice(Math.min(...mi)) : text;
  const snapshot = (diff?.snapshot_id ? String(diff.snapshot_id) : null)
    ?? (tail.match(/\(snapshot ([A-Za-z0-9_.-]+);/) || [])[1]
    ?? (sc?.snapshot_id ? String(sc.snapshot_id) : null)
    ?? (text.match(/snapshot_id=([A-Za-z0-9_.-]+)/) || [])[1]
    ?? null;
  const base = (diff?.base_snapshot_id ? String(diff.base_snapshot_id) : null) ?? (hasMarker ? (tail.match(BASE_RX) || [])[1] ?? null : null);
  const isDiff = !!diff?.base_snapshot_id || (hasMarker && DIFF_RX.test(tail));
  return { kind: isDiff ? "diff" : "full", snapshot, base: isDiff ? base : null };
}

function isObserveMsg(m: Msg) {
  return m?.role === "custom" && m.customType === OBSERVE_TYPE;
}

function metaOf(m: Msg): ObsMeta | null {
  if (!m || (m.role !== "toolResult" && !isObserveMsg(m))) return null;
  const d = m.details?.obs;
  if (d && (d.kind === "full" || d.kind === "diff")) return { kind: d.kind, snapshot: d.snapshot ?? null, base: d.base ?? null };
  if (isObserveMsg(m)) return null;
  return obsMeta(String(m.toolName || ""), textOf(m));
}

/** "full" | "diff" | null: what kind of window observation this tool result carries. */
export function obsKind(m: Msg): "full" | "diff" | null {
  return metaOf(m)?.kind ?? null;
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

function list(v: any, max = 20): string {
  const items = (Array.isArray(v) ? v : [v]).map((x: any) => {
    if (typeof x === "number") return `[${x}]`;
    if (typeof x === "string") return x.trim().slice(0, 80);
    if (x && typeof x === "object") {
      const i = x.index ?? x.n ?? x.from;
      const row = x.row ?? x.text ?? x.label ?? "";
      const to = x.to !== undefined ? ` -> [${x.to}]` : "";
      return `${i !== undefined ? `[${i}]` : ""}${to}${row ? ` ${String(row).trim().slice(0, 80)}` : ""}`.trim();
    }
    return String(x);
  }).filter(Boolean);
  return items.slice(0, max).join("; ") + (items.length > max ? `; ... (${items.length - max} more)` : "");
}

/** Client note for a structured diff: removed and reordered rows made explicit. Empty when there is nothing to say. */
export function diffNote(diff: any): string {
  if (!diff || typeof diff !== "object") return "";
  const base = diff.base_snapshot_id ? String(diff.base_snapshot_id) : "the previous read";
  const parts: string[] = [];
  const removed = diff.removed;
  const nRemoved = Array.isArray(removed) ? removed.length : Number(removed) || 0;
  if (nRemoved > 0) {
    parts.push(`rows gone since snapshot ${base}: ${Array.isArray(removed) ? list(removed) : `${nRemoved} rows`} - their tokens no longer work`);
  }
  const re = diff.reordered;
  const nRe = Array.isArray(re) ? re.length : re === true ? 1 : Number(re) || 0;
  if (nRe > 0) {
    parts.push(`rows were renumbered${Array.isArray(re) ? ` (${list(re)})` : ""}: an [N] from snapshot ${base} that is not listed here may now point at another element - use the tokens shown here or read the window again`);
  }
  return parts.length ? `[client: ${parts.join(". ")}.]` : "";
}

function pruneText(text: string, name?: string): string {
  for (const k of MARKERS) {
    const i = markerAt(text, k);
    if (i >= 0) {
      const head = text.slice(0, i).trimEnd();
      return (head.length > HEAD_KEEP ? head.slice(0, HEAD_KEEP) + " ..." : head) + (head ? "\n" : "") + summary(text.slice(i));
    }
  }
  if (String(name || "").endsWith("get_window_state")) return summary(text);
  return text;
}

export const ORPHAN_NOTE = (base: string) =>
  `[client: this tree is a diff against snapshot ${base}, which is no longer in context; rows not listed here are unknown. ` +
  "Read the window again (get_window_state) before using any token not shown here.]";

export function prune(messages: Msg[], opts: { pinned?: Set<string> } = {}): Msg[] {
  const metas = messages.map(metaOf);
  const wins = messages.map((m, i) => (metas[i] ? windowKey(m) : ""));
  // snapshot id -> newest index carrying it
  const bySnap = new Map<string, number>();
  metas.forEach((o, i) => {
    if (o?.snapshot) bySnap.set(o.snapshot, i);
  });
  const newest = new Map<string, number>();
  metas.forEach((o, i) => {
    if (o) newest.set(wins[i], i);
  });
  const keep = new Set<number>();
  const orphan = new Map<number, string>();
  for (const [w, n] of newest) {
    if (w === "?") {
      keep.add(n);
      continue;
    }
    let i = n;
    const seen = new Set<number>();
    while (i >= 0 && !seen.has(i)) {
      seen.add(i);
      keep.add(i);
      const o = metas[i]!;
      if (o.kind === "full") break;
      if (o.base) {
        const b = bySnap.get(o.base);
        if (b === undefined || b >= i) {
          orphan.set(i, o.base);
          break;
        }
        i = b;
        continue;
      }
      // a diff with no readable base: keep back to the newest full read of this window
      let j = i - 1;
      while (j >= 0 && !(metas[j] && wins[j] === w && metas[j]!.kind === "full")) j--;
      if (j < 0) {
        orphan.set(i, "?");
        break;
      }
      for (let k = j; k < i; k++) if (metas[k] && wins[k] === w) keep.add(k);
      break;
    }
  }
  const old = new Set<number>();
  metas.forEach((o, i) => {
    if (o && !keep.has(i)) old.add(i);
  });
  // newest image per window, newest MAX_IMAGES windows only; pinned images and the newest recall always stay
  const pinned = opts.pinned || new Set<string>();
  const imgByWin = new Map<string, number>();
  let newestRecall = -1;
  messages.forEach((m, i) => {
    if (!hasImage(m)) return;
    if (m.role === "toolResult" && m.toolName === RECALL_TOOL) {
      newestRecall = i;
      return;
    }
    if (!old.has(i)) imgByWin.set(windowKey(m), i);
  });
  const keepImg = new Set([...imgByWin.values()].sort((a, b) => b - a).slice(0, MAX_IMAGES));
  if (newestRecall >= 0) keepImg.add(newestRecall);
  return messages.map((m, i) => {
    const isOld = old.has(i);
    const ids = imageIds(m);
    const anyPinned = ids.some((id) => pinned.has(id));
    const dropImg = hasImage(m) && (isOld || !keepImg.has(i));
    const orph = orphan.get(i);
    if (!isOld && !dropImg && orph === undefined) return m;
    const content: any[] = [];
    let dropped = 0;
    let k = 0;
    const gone: string[] = [];
    for (const c of m.content || []) {
      if (c && c.type === "image") {
        const id = ids[k++];
        if (dropImg && !(id && pinned.has(id))) {
          dropped++;
          if (id) gone.push(id);
          continue;
        }
        content.push(c);
        continue;
      }
      if (isOld && c && c.type === "text") content.push({ ...c, text: pruneText(c.text || "", m.toolName) });
      else content.push(c);
    }
    if (dropped) {
      content.push({ type: "text", text: gone.length
        ? `[client: screenshot ${gone.join(", ")} removed from context (a newer one of this window is current); recall_image brings it back]`
        : "[client: an older screenshot was removed; the newest one of this window is current]" });
    }
    if (anyPinned && dropImg) {
      const lab = ids.filter((id) => pinned.has(id));
      content.push({ type: "text", text: `[client: pinned evidence kept: ${lab.join(", ")}]` });
    }
    if (orph !== undefined && !textOf(m).includes("which is no longer in context")) content.push({ type: "text", text: ORPHAN_NOTE(orph) });
    return { ...m, content };
  });
}

/** Ids of a message's images, in order: details.images (set when the result arrived), else the client tags. */
export function imageIds(m: Msg): string[] {
  const d = m?.details?.images;
  if (Array.isArray(d)) return d.map(String);
  const out: string[] = [];
  for (const c of m?.content || []) {
    if (c?.type !== "text") continue;
    for (const x of String(c.text || "").matchAll(/\[client: screenshot (img-\d+)/g)) out.push(x[1]);
  }
  return out;
}
