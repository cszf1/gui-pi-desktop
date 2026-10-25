// Screenshots outside the model's context are not lost: every image a tool returns gets an id ("img-N"), is
// written byte for byte (JPEG q60 as the gate sent it, never re-encoded or scaled) to a private directory under
// the session's dir, and can be brought back with `recall_image {id}`. In context the model keeps the newest
// screenshot per window plus up to MAX_PINS evidence images it pinned with `pin_evidence {id, label}`.
// Pins are saved in the session (pi.appendEntry) so they survive compaction and a restart.
import { mkdirSync, writeFileSync, readFileSync, readdirSync, existsSync, chmodSync } from "node:fs";
import { join } from "node:path";

export const MAX_PINS = 3;
export const PINS_ENTRY = "gui-pi-pins";
export const IMG_RX = /^img-(\d{1,9})$/;

export type ImgMeta = { id: string; window: string; tool: string; mimeType: string; at: string };
export type Pin = { id: string; label: string };

export class ImageStore {
  dir: string;
  next = 1;
  pins: Pin[] = [];

  constructor(dir: string) {
    this.dir = dir;
    mkdirSync(dir, { recursive: true, mode: 0o700 });
    try {
      chmodSync(dir, 0o700);
    } catch {}
    for (const f of readdirSync(dir)) {
      const m = f.match(/^img-(\d+)\.json$/);
      if (m) this.next = Math.max(this.next, Number(m[1]) + 1);
    }
  }

  /** Store one image (base64 as received) and return its id. */
  save(data: string, mimeType: string, window: string, tool: string): string {
    const id = `img-${this.next++}`;
    const meta: ImgMeta = { id, window, tool, mimeType: mimeType || "image/jpeg", at: new Date().toISOString() };
    writeFileSync(join(this.dir, `${id}.bin`), Buffer.from(String(data || ""), "base64"), { mode: 0o600 });
    writeFileSync(join(this.dir, `${id}.json`), JSON.stringify(meta), { mode: 0o600 });
    return id;
  }

  get(id: string): { data: string; meta: ImgMeta } | null {
    if (!IMG_RX.test(String(id))) return null;
    const b = join(this.dir, `${id}.bin`);
    const j = join(this.dir, `${id}.json`);
    if (!existsSync(b) || !existsSync(j)) return null;
    try {
      return { data: readFileSync(b).toString("base64"), meta: JSON.parse(readFileSync(j, "utf8")) };
    } catch {
      return null;
    }
  }

  has(id: string) {
    return !!this.get(id);
  }

  isPinned(id: string) {
    return this.pins.some((p) => p.id === id);
  }

  /** Pin an image as evidence. Error text when it cannot be pinned. */
  pin(id: string, label: string): string | null {
    const l = String(label || "").trim().slice(0, 120);
    if (!l) return "give a short label saying what the image shows (e.g. 'Calc: totals before the change')";
    if (!this.has(id)) return `no stored image with id ${id}`;
    const ex = this.pins.find((p) => p.id === id);
    if (ex) {
      ex.label = l;
      return null;
    }
    if (this.pins.length >= MAX_PINS)
      return `at most ${MAX_PINS} pinned images; unpin one first (pin_evidence with unpin:true). Pinned: ` +
        this.pins.map((p) => `${p.id} "${p.label}"`).join(", ");
    this.pins.push({ id, label: l });
    return null;
  }

  unpin(id: string): boolean {
    const n = this.pins.length;
    this.pins = this.pins.filter((p) => p.id !== id);
    return this.pins.length !== n;
  }

  pinsJSON() {
    return { pins: this.pins.map((p) => ({ ...p })) };
  }

  /** Restore pins from the newest entry on the branch. */
  restore(branch: any[]) {
    for (let i = (branch || []).length - 1; i >= 0; i--) {
      const e = branch[i];
      if (e?.type === "custom" && e.customType === PINS_ENTRY && Array.isArray(e.data?.pins)) {
        this.pins = e.data.pins.filter((p: any) => p && IMG_RX.test(String(p.id))).slice(0, MAX_PINS)
          .map((p: any) => ({ id: String(p.id), label: String(p.label || "").slice(0, 120) }));
        return true;
      }
    }
    return false;
  }
}

/** The text line put right after each image, so the model can name it. */
export function imageTag(id: string, window: string): string {
  return `[client: screenshot ${id}${window && window !== "?" ? ` (window ${window})` : ""}; pin_evidence to keep it in view, recall_image to see it again later]`;
}

/** Ids tagged in a message's text content, in order. */
export function taggedIds(m: any): string[] {
  const out: string[] = [];
  for (const c of m?.content || []) {
    if (c?.type !== "text") continue;
    for (const x of String(c.text || "").matchAll(/\[client: screenshot (img-\d+)/g)) out.push(x[1]);
  }
  return out;
}
