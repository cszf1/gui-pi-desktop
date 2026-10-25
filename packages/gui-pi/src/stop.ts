// Stop from the desktop shell. The extension keeps one connection to the shell's person IPC open:
//   -> {"token","op":"watch_stop","id":"stop"}
//   <- {"stop":true}                       when the user presses Stop
//   -> {"ack":"stopped","reset":bool}      after: actions blocked, gate_reset sent (when the gate has it), gate killed
// The shell kills Pi and the gate's process group right after the ack (or after a short wait without one), so this
// is the orderly part only; nothing here can keep the task alive.
import { connect, type Socket } from "node:net";

export type OnStop = () => Promise<{ reset: boolean }>;

export function watchStop(path: string | undefined, token: string, onStop: OnStop): { close: () => void } {
  if (!path) return { close() {} };
  let sock: Socket | null = null;
  let done = false;
  let buf = "";
  try {
    const tcp = path.match(/^tcp:([^:]+):(\d+)$/);
    sock = tcp ? connect({ host: tcp[1], port: Number(tcp[2]) }) : connect(path);
  } catch {
    return { close() {} };
  }
  sock.on("error", () => {});
  sock.on("connect", () => sock!.write(JSON.stringify({ token, op: "watch_stop", id: "stop" }) + "\n"));
  sock.on("data", async (d) => {
    buf += String(d);
    const nl = buf.indexOf("\n");
    if (nl < 0 || done) return;
    let m: any = null;
    try {
      m = JSON.parse(buf.slice(0, nl));
    } catch {}
    buf = buf.slice(nl + 1);
    if (m?.stop !== true) return;
    done = true;
    let reset = false;
    try {
      reset = (await onStop()).reset;
    } catch {}
    try {
      sock!.end(JSON.stringify({ ack: "stopped", reset }) + "\n");
    } catch {}
  });
  // never keeps Pi alive on its own
  sock.unref();
  return { close: () => sock?.destroy() };
}
