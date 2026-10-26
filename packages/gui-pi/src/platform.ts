// Platform conditions under which the agent can neither see nor safely act, recognised in tool results:
//   a11y_lost        Linux: the AT-SPI accessibility bus or the DBus session bus went away mid-task (window reads
//                    fail or come back empty; the driver cannot reach apps)
//   secure_desktop   Windows: a UAC prompt is up on the secure desktop (the agent cannot see or click it)
//   locked           Windows / macOS: the session is locked (no interactive desktop)
//   no_desktop       Windows: an RDP session was disconnected / minimised (no interactive desktop)
//   elevated_target  Windows: the target window runs as administrator; a normal-privilege agent cannot send it input
// What the client does (the gate stays the authority on every action):
//   - the result gets a fixed note: do not retry blind; ask a person to take over (request_human takeover) or stop
//     and report; for a lost bus, a read is the only way to see whether it is back;
//   - after HOLD_AFTER consecutive results with the same condition, GUI actions are held (reads, request_human and
//     the plan / report tools still run) until a read succeeds again (or, for elevated_target, a person took over).
// The patterns are deliberately narrow (driver / OS error wording); an app window that merely shows such words in
// its own text does not count, because only error results and the gate's driver error lines are classified.

export type Condition = "a11y_lost" | "secure_desktop" | "locked" | "no_desktop" | "elevated_target";
export const HOLD_AFTER = 2;

const RULES: [Condition, RegExp][] = [
  ["a11y_lost", /org\.a11y\.Bus|at-spi[^\n]{0,40}(not|unavailable|lost|failed|disconnect|could ?n.?t)|accessibility bus[^\n]{0,40}(not|unavailable|lost|failed|could ?n.?t)|could ?n.?t connect to (the )?accessibility bus|(dbus|d-bus)[^\n]{0,60}(connection (closed|refused|reset)|disconnected|no such file|not available|failed to connect)|DBUS_SESSION_BUS_ADDRESS[^\n]{0,30}(unset|not set)|org\.freedesktop\.DBus\.Error\.(NoReply|Disconnected|ServiceUnknown)/i],
  ["secure_desktop", /secure desktop|consent\.exe|user account control|\bUAC\b[^\n]{0,40}(prompt|dialog|active|showing)/i],
  ["locked", /(workstation|session|screen|desktop) (is )?locked|lock ?screen is (on|active|showing)|LogonUI/i],
  ["no_desktop", /no interactive desktop|rdp[^\n]{0,40}(disconnected|minimi[sz]ed)|session (is )?disconnected|desktop (is )?not available/i],
  ["elevated_target", /UIPI|access (is )?denied[^\n]{0,60}(elevated|administrator|integrity)|(elevated|administrator|higher integrity)[^\n]{0,40}(window|process)[^\n]{0,40}(cannot|can.t|denied|blocked)/i],
];

export const NOTE: Record<Condition, string> = {
  a11y_lost: "[client: PLATFORM - the accessibility bus (AT-SPI / DBus) seems to be gone, so windows cannot be read or driven reliably. " +
    "Do not retry actions blind. Read the window once to see whether it is back; if it is not, ask the person (request_human takeover: " +
    "restart the app or log out and in) or stop and report. GUI actions are held after repeated failures until a read works.]",
  secure_desktop: "[client: PLATFORM - a Windows UAC prompt is on the secure desktop: the agent cannot see or click it. Do not retry. " +
    "Ask the person to take over (request_human takeover: answer the UAC prompt themselves) or stop and report.]",
  locked: "[client: PLATFORM - the session is locked: there is no desktop to act on. Do not retry. Ask the person to unlock it " +
    "(request_human takeover) or stop and report.]",
  no_desktop: "[client: PLATFORM - there is no interactive desktop (Remote Desktop disconnected or minimised). Do not retry. " +
    "Ask the person to reconnect (request_human takeover) or stop and report.]",
  elevated_target: "[client: PLATFORM - that window runs as administrator; the agent runs with normal rights and cannot act on it. " +
    "Do not retry. Ask the person to do this step (request_human takeover) or stop and report.]",
};

/** What a constraint line in the task's process state says while a condition holds. */
export const CONSTRAINT: Record<Condition, string> = {
  a11y_lost: "platform: the accessibility bus was lost - read before acting; a person may need to restart the app or session",
  secure_desktop: "platform: a UAC prompt needs the person (the agent cannot reach the secure desktop)",
  locked: "platform: the session was locked - the person must unlock it",
  no_desktop: "platform: no interactive desktop (RDP) - the person must reconnect",
  elevated_target: "platform: an administrator window cannot be driven with normal rights - the person does that step",
};

/** The condition a tool result shows, or null. Only error / not-executed / unknown results are classified. */
export function classify(text: string, isError: boolean, status = ""): Condition | null {
  if (!isError && status !== "unknown" && status !== "blocked") return null;
  const t = String(text || "").slice(0, 4000);
  for (const [c, rx] of RULES) if (rx.test(t)) return c;
  return null;
}

export const READ_ONLY = new Set(["get_window_state", "list_windows", "list_apps", "inspect", "check_done", "set_plan", "gate_report", "request_human"]);

export class PlatformHealth {
  current: Condition | null = null;
  streak = 0;

  /** Feed one gate tool result; returns the note to append (or null). */
  onResult(tool: string, text: string, isError: boolean, status = "", byPerson = false): string | null {
    const c = classify(text, isError, status);
    if (c) {
      this.streak = c === this.current ? this.streak + 1 : 1;
      this.current = c;
      return NOTE[c];
    }
    // a read that worked: the desktop answers again
    if (tool.endsWith("get_window_state") && !isError) this.reset();
    // a person did the step that needed them
    else if (byPerson && this.current && this.current !== "a11y_lost") this.reset();
    return null;
  }

  reset() {
    this.current = null;
    this.streak = 0;
  }

  /** Why a GUI action is held right now (null: it may go to the gate). */
  check(tool: string, gateTools: Set<string>): string | null {
    if (!this.current || this.streak < HOLD_AFTER || !gateTools.has(tool) || READ_ONLY.has(tool)) return null;
    return `Held: ${CONSTRAINT[this.current]}. Read the window (get_window_state) to see whether it is back, or ask the person (request_human takeover), or stop and report.`;
  }
}
