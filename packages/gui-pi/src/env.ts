// What the gate process gets: an explicit environment, never Pi's model key or the shell's person token.
//
// The desktop shell keeps the gate's own settings and keys (reviewer, roles: GUI_GATE_*) OUT of Pi's environment:
// it hands them over once through its ask-a-person IPC ({"op":"gate_env"}, one-shot, token-checked), and this
// extension passes them only to the gate child. Pi's model key (GUI_PI_MAIN_KEY) stays in Pi and never reaches the
// gate. GUI_GATE_ALLOW_CONFIG / --allow-config (the gate's trusted-launcher config tool) is never passed on.
import { connect } from "node:net";

/** Names a child may inherit (what a desktop session, locale and proxies need); mirrors gui-gate proc.rs. */
export const BASE_ENV_NAMES = [
  "PATH", "HOME", "USER", "LOGNAME", "SHELL", "PWD", "TERM", "COLORTERM", "LANG", "LANGUAGE", "TZ", "TMPDIR",
  "DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY", "DBUS_SESSION_BUS_ADDRESS", "AT_SPI_BUS_ADDRESS", "NO_AT_BRIDGE",
  "DESKTOP_SESSION", "SESSION_MANAGER",
  "http_proxy", "https_proxy", "no_proxy", "all_proxy", "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "ALL_PROXY",
  "SYSTEMROOT", "SystemRoot", "SYSTEMDRIVE", "SystemDrive", "WINDIR", "windir", "COMSPEC", "ComSpec", "PATHEXT",
  "TEMP", "TMP", "USERPROFILE", "USERNAME", "USERDOMAIN", "APPDATA", "LOCALAPPDATA", "PROGRAMDATA", "ProgramData",
  "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "CommonProgramFiles", "PUBLIC", "HOMEDRIVE", "HOMEPATH",
  "NUMBER_OF_PROCESSORS", "PROCESSOR_ARCHITECTURE", "OS", "__CF_USER_TEXT_ENCODING",
];
export const BASE_ENV_PREFIXES = ["LC_", "XDG_", "GTK_", "GDK_", "QT_", "CUA_"];
/** Never given to the gate, whatever the source. */
export const NEVER_TO_GATE = ["GUI_PI_MAIN_KEY", "GUI_PI_HUMAN_TOKEN", "GUI_GATE_ALLOW_CONFIG", "GUI_GATE_TASK", "GUI_GATE_TASK_FILE"];
/** Gate arguments the extension never passes (trusted-launcher / bench only). */
export const FORBIDDEN_GATE_ARGS = ["--allow-config"];
/** Task arguments only the user's channel sets (GUI_PI_TASK_FILE / GUI_PI_TASK). */
export const TASK_ARGS = ["--task", "--task-file"];

const isNever = (k: string) => NEVER_TO_GATE.includes(k) || k.startsWith("GUI_PI_");

/**
 * The gate child's environment: the allowlisted session variables from Pi's environment, the GUI_GATE_* settings
 * Pi's environment carries (plain `pi -e` runs and tests), then what the shell handed over (`fromShell`).
 */
export function gateEnv(env: NodeJS.ProcessEnv, fromShell: Record<string, string> = {}): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(env)) {
    if (v === undefined || isNever(k)) continue;
    if (BASE_ENV_NAMES.includes(k) || BASE_ENV_PREFIXES.some((p) => k.startsWith(p)) || k.startsWith("GUI_GATE_")) out[k] = v;
  }
  for (const [k, v] of Object.entries(fromShell || {})) {
    if (typeof v !== "string" || isNever(k) || !k.startsWith("GUI_GATE_")) continue;
    out[k] = v;
  }
  delete out.GUI_GATE_ALLOW_CONFIG;
  delete out.GUI_GATE_TASK;
  delete out.GUI_GATE_TASK_FILE;
  out.GUI_GATE_NO_UPDATE_CHECK = "1";
  return out;
}

export function gateArgs(env: NodeJS.ProcessEnv): string[] {
  const a = ["--human", "elicit", "--human-timeout", String(Number(env.GUI_PI_HUMAN_TIMEOUT) || 120)];
  a.push("--log", env.GUI_PI_GATE_LOG || "none");
  if (env.GUI_PI_DRIVER) a.push("--driver", env.GUI_PI_DRIVER);
  // the trusted task channel: the shell writes the user's own words to GUI_PI_TASK_FILE (0600, its private runtime
  // dir) and the gate reads (and re-reads) it. The extension only passes the path on; it never writes the file.
  if (env.GUI_PI_TASK_FILE) a.push("--task-file", env.GUI_PI_TASK_FILE);
  else if (env.GUI_PI_TASK) a.push("--task", env.GUI_PI_TASK);
  if (env.GUI_PI_GATE_ARGS) {
    try {
      const extra = JSON.parse(env.GUI_PI_GATE_ARGS);
      if (Array.isArray(extra)) {
        const xs = extra.map(String);
        for (let i = 0; i < xs.length; i++) {
          const k = xs[i].split("=")[0];
          if (FORBIDDEN_GATE_ARGS.includes(k)) continue;
          // the task comes from the user's channel only, never from extra arguments
          if (TASK_ARGS.includes(k)) {
            if (!xs[i].includes("=")) i++;
            continue;
          }
          a.push(xs[i]);
        }
      }
    } catch {}
  }
  return a;
}

/** One-shot fetch of the gate's settings from the shell's IPC; {} when there is no shell or it says no. */
export function fetchGateEnv(path: string | undefined, token: string, timeoutMs = 5000): Promise<Record<string, string>> {
  if (!path) return Promise.resolve({});
  return new Promise((resolve) => {
    let done = false;
    let buf = "";
    const finish = (v: Record<string, string>) => {
      if (done) return;
      done = true;
      clearTimeout(t);
      sock.destroy();
      resolve(v);
    };
    const t = setTimeout(() => finish({}), timeoutMs);
    const tcp = path.match(/^tcp:([^:]+):(\d+)$/);
    const sock = tcp ? connect({ host: tcp[1], port: Number(tcp[2]) }) : connect(path);
    sock.on("error", () => finish({}));
    sock.on("connect", () => sock.write(JSON.stringify({ token, op: "gate_env", id: "gate-env" }) + "\n"));
    sock.on("data", (d) => {
      buf += String(d);
      const nl = buf.indexOf("\n");
      if (nl < 0) return;
      try {
        const r = JSON.parse(buf.slice(0, nl));
        const env = r && typeof r.env === "object" && r.env ? r.env : {};
        const out: Record<string, string> = {};
        for (const [k, v] of Object.entries(env)) if (typeof v === "string") out[k] = v;
        finish(out);
      } catch {
        finish({});
      }
    });
    sock.on("close", () => finish({}));
  });
}
