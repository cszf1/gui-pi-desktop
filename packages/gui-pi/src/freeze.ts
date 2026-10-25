// While a person is answering a gate question, no other GUI tool runs; and once a person declined, cancelled or
// let a question time out, the rest of that model turn's tool calls are not run (the model reads the outcome and
// decides again). The gate stays the authority: this only stops the client from racing ahead of the person.
import type { ElicitParams, ElicitResult } from "./mcp.ts";

export class PersonFreeze {
  open = 0;
  declinedThisTurn = false;

  wrap(ask: (p: ElicitParams, s?: AbortSignal) => Promise<ElicitResult>) {
    return async (p: ElicitParams, s?: AbortSignal): Promise<ElicitResult> => {
      this.open++;
      try {
        const r = await ask(p, s);
        if (!r || r.action !== "accept") this.declinedThisTurn = true;
        return r;
      } catch (e) {
        this.declinedThisTurn = true;
        throw e;
      } finally {
        this.open--;
      }
    };
  }

  newTurn() {
    this.declinedThisTurn = false;
  }

  /** null = may run; otherwise the reason the call is held back. */
  check(tool: string, gateTools: Set<string>): string | null {
    if (!gateTools.has(tool)) return null;
    if (this.open > 0) return "A person is answering a gate question right now; nothing else runs until they answer. Wait for that result.";
    if (this.declinedThisTurn)
      return "Not run: the person declined, cancelled or did not answer an earlier step in this turn, so the rest of this turn's actions were skipped. Read that result and decide again.";
    return null;
  }
}
