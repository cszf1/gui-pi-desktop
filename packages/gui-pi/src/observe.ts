// One freshness judgement for every window observation the model gets, whichever way it came:
//   - the gate's automatic observation in an action's result ("Window right after the action"),
//   - a read the model asked for (get_window_state),
//   - the read the extension attaches before a turn when gate_next_step asks for one.
// So the extension does not read a window again right after a result already carried a fresh read of it (no action
// since, newer than FRESH_MS, with a screenshot when one is wanted).
//
// It also keeps, per window, how the diff chain stands and asks for a full ANCHOR read (a plain get_window_state,
// which the gate answers with the whole tree and makes the new diff base) before the next turn when:
//   - the chain of diffs since the last full tree is longer than MAX_DIFF_CHAIN,
//   - a diff arrived whose base snapshot the client never saw or lost in a compaction (rows not listed are unknown),
//   - the context was compacted (the summary asks for a re-read anyway; the client does it),
//   - a person acted (takeover / did it): the affected state is re-read, the original action is never replayed.
import type { ObsMeta } from "./context.ts";

export const FRESH_MS = 10_000;
export const MAX_DIFF_CHAIN = 6;

type WinState = { snapshot: string | null; at: number; screenshot: boolean; diffs: number; stale: boolean };

export class ObsTracker {
  win = new Map<string, WinState>();
  /** snapshot ids the model has seen in this context */
  known = new Set<string>();
  /** snapshot ids that were compacted away */
  gone = new Set<string>();
  /** window -> why a full anchor read is needed */
  anchor = new Map<string, string>();
  now: () => number;
  freshMs: number;
  maxChain: number;

  constructor(opts: { now?: () => number; freshMs?: number; maxChain?: number } = {}) {
    this.now = opts.now ?? (() => Date.now());
    this.freshMs = opts.freshMs ?? FRESH_MS;
    this.maxChain = opts.maxChain ?? MAX_DIFF_CHAIN;
  }

  /** A window observation reached the model (any of the three ways). */
  onObservation(wkey: string | undefined, obs: ObsMeta | null, screenshot: boolean) {
    if (!wkey || !obs) return;
    const prev = this.win.get(wkey);
    let diffs = 0;
    if (obs.kind === "full") {
      this.anchor.delete(wkey);
    } else {
      diffs = (prev?.diffs ?? 0) + 1;
      if (obs.base && (this.gone.has(obs.base) || !this.known.has(obs.base)))
        this.anchor.set(wkey, `the diff's base snapshot ${obs.base} is not in context`);
      else if (diffs > this.maxChain) this.anchor.set(wkey, `${diffs} diffs in a row since the last full tree`);
    }
    if (obs.snapshot) this.known.add(obs.snapshot);
    this.win.set(wkey, { snapshot: obs.snapshot, at: this.now(), screenshot, diffs, stale: false });
  }

  /** An action ran without a fresh observation of its window (unknown window: every window is stale). */
  onAction(wkey: string | undefined) {
    if (wkey && this.win.has(wkey)) this.win.get(wkey)!.stale = true;
    else if (!wkey) for (const w of this.win.values()) w.stale = true;
  }

  /** Is the newest observation of this window still current enough to skip another read? */
  isFresh(wkey: string | undefined, wantScreenshot: boolean): boolean {
    if (!wkey) return false;
    const w = this.win.get(wkey);
    if (!w || w.stale || this.anchor.has(wkey)) return false;
    if (wantScreenshot && !w.screenshot) return false;
    return this.now() - w.at <= this.freshMs;
  }

  snapshotOf(wkey: string | undefined): string | null {
    return (wkey && this.win.get(wkey)?.snapshot) || null;
  }

  /** Force a full re-read of a window before the next turn. */
  force(wkey: string | undefined, why: string) {
    if (wkey) this.anchor.set(wkey, why);
  }

  anchorReason(wkey: string | undefined): string | null {
    return (wkey && this.anchor.get(wkey)) || null;
  }

  /** The context was compacted: everything seen so far may be gone; every window needs a full read before use. */
  onCompaction() {
    for (const s of this.known) this.gone.add(s);
    this.known.clear();
    for (const [k, w] of this.win) {
      w.stale = true;
      this.anchor.set(k, "the context was compacted");
    }
  }
}
