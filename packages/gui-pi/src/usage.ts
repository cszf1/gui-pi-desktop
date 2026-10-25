// The model's real token usage, sent with each tool call in the request `_meta` as
//   "gui_pi/usage": {input, output, cache_read}
// taken from the assistant message that produced the call (Pi's message_end usage: input, output, cacheRead).
// One assistant message can hold several tool calls; its usage is reported once, with the first call, and the
// other calls of that message carry zeros plus same_message:true, so a sum over calls is the real total.
export type GateUsage = { input: number; output: number; cache_read: number; same_message?: boolean };

const n = (v: any) => (Number.isFinite(Number(v)) && Number(v) >= 0 ? Math.round(Number(v)) : 0);

export class UsageBook {
  byCall = new Map<string, GateUsage>();

  /** Pi `message_end` for an assistant message. */
  onAssistant(message: any) {
    if (!message || message.role !== "assistant") return;
    const u = message.usage || {};
    const calls = (message.content || []).filter((c: any) => c?.type === "toolCall" && c.id).map((c: any) => String(c.id));
    calls.forEach((id: string, i: number) => {
      this.byCall.set(id, i === 0
        ? { input: n(u.input), output: n(u.output), cache_read: n(u.cacheRead ?? u.cache_read) }
        : { input: 0, output: 0, cache_read: 0, same_message: true });
    });
    if (this.byCall.size > 500) {
      const first = this.byCall.keys().next().value;
      if (first !== undefined) this.byCall.delete(first);
    }
  }

  /** The `_meta` entry for a tool call (undefined when Pi gave no usage for it). */
  metaFor(toolCallId: string): Record<string, GateUsage> | undefined {
    const u = this.byCall.get(String(toolCallId));
    if (!u) return undefined;
    this.byCall.delete(String(toolCallId));
    return { "gui_pi/usage": u };
  }
}
