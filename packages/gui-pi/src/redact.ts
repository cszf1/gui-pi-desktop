// Key values this extension handles (Pi's own model key, the gate's keys it received from the shell) never reach
// the model, the session or a log through it: every tool result and every message the extension writes is scrubbed
// of them first. (The gate keeps its own children's environment clean; this is the client's second line.)
export const MASK = "***";
const MIN = 8;

export class Secrets {
  values: string[] = [];

  add(v: string | undefined | null) {
    const s = String(v ?? "").trim();
    if (s.length >= MIN && !this.values.includes(s)) {
      this.values.push(s);
      this.values.sort((a, b) => b.length - a.length);
    }
  }

  /** Values of key-like variables (…_KEY, …TOKEN…) in an environment. */
  addFromEnv(env: Record<string, string | undefined>) {
    for (const [k, v] of Object.entries(env || {})) if (/_KEY$|TOKEN/.test(k)) this.add(v);
  }

  text(s: string): string {
    let out = String(s ?? "");
    for (const v of this.values) if (out.includes(v)) out = out.split(v).join(MASK);
    return out;
  }

  /** Scrub every text item (images are pixels; structured data is scrubbed as JSON). */
  content(items: any[]): any[] {
    return (items || []).map((c: any) => (c && c.type === "text" ? { ...c, text: this.text(c.text || "") } : c));
  }

  any<T>(v: T): T {
    if (!this.values.length || v === undefined || v === null) return v;
    try {
      const j = JSON.stringify(v);
      const r = this.text(j);
      return r === j ? v : JSON.parse(r);
    } catch {
      return v;
    }
  }
}
