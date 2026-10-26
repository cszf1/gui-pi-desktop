// Main-model latency per request: time to first token and total time, with how many images and input tokens the
// request carried. It is recorded so that "are 3 window screenshots + 3 pinned + 1 recalled too many?" can be
// answered from data (latency against image count), not from the image count alone.
//   start        before_provider_request (falls back to turn_start when Pi does not emit it)
//   first token  the first message_update of the assistant message
//   end          message_end of the assistant message (usage: input / output / cacheRead)
//   images       image parts in the messages the `context` handler returned for this request
// Lines go to a JSONL file (GUI_PI_LATENCY_LOG, else model.jsonl next to GUI_PI_GATE_LOG) as
//   {"ev":"model_latency","schema":"gui-pi-latency/1","turn","ttft_ms","total_ms","images","input","output","cache_read",
//    "stop","at"}
// A missing value is null (e.g. no streaming update: ttft_ms null), never a guess.
import { appendFileSync } from "node:fs";

export type LatencyRec = {
  ev: "model_latency"; schema: "gui-pi-latency/1"; turn: number; ttft_ms: number | null; total_ms: number | null;
  images: number | null; input: number | null; output: number | null; cache_read: number | null; stop: string | null; at: string;
};

const n = (v: any) => (Number.isFinite(Number(v)) && v !== null && v !== undefined ? Math.round(Number(v)) : null);

export class LatencyBook {
  file: string | null;
  now: () => number;
  recs: LatencyRec[] = [];
  private turn = 0;
  private start: number | null = null;
  private first: number | null = null;
  private images: number | null = null;

  constructor(file: string | null, now: () => number = () => Date.now()) {
    this.file = file;
    this.now = now;
  }

  onTurnStart() {
    this.turn++;
    // only a fallback: before_provider_request, when Pi emits it, is the real start
    this.start = this.now();
    this.first = null;
  }

  onContext(messages: any[]) {
    let k = 0;
    for (const m of messages || []) for (const c of m?.content || []) if (c?.type === "image") k++;
    this.images = k;
  }

  onProviderRequest() {
    this.start = this.now();
    this.first = null;
  }

  onUpdate(message: any) {
    if (message?.role !== "assistant" || this.start === null || this.first !== null) return;
    this.first = this.now();
  }

  onEnd(message: any): LatencyRec | null {
    if (message?.role !== "assistant" || this.start === null) return null;
    const t = this.now();
    const u = message.usage || {};
    const rec: LatencyRec = {
      ev: "model_latency", schema: "gui-pi-latency/1", turn: this.turn,
      ttft_ms: this.first !== null ? this.first - this.start : null, total_ms: t - this.start,
      images: this.images, input: n(u.input), output: n(u.output), cache_read: n(u.cacheRead ?? u.cache_read),
      stop: message.stopReason ? String(message.stopReason) : null, at: new Date().toISOString(),
    };
    this.start = null;
    this.first = null;
    this.recs.push(rec);
    if (this.recs.length > 500) this.recs.shift();
    if (this.file) {
      try {
        appendFileSync(this.file, JSON.stringify(rec) + "\n", { mode: 0o600 });
      } catch {}
    }
    return rec;
  }

  /** Median / p90 of a field over the recorded requests (null when nothing was measured). */
  quantile(field: "ttft_ms" | "total_ms", q: number): number | null {
    const v = this.recs.map((r) => r[field]).filter((x): x is number => typeof x === "number").sort((a, b) => a - b);
    if (!v.length) return null;
    return v[Math.min(v.length - 1, Math.floor(q * (v.length - 1) + 0.5))];
  }
}
