const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const log = document.getElementById("log");
const $ = (id) => document.getElementById(id);
const add = (kind, text, image, extra) => {
  const d = document.createElement("div"); d.className = "ev " + kind; d.textContent = (kind === "tool" ? "→ " : "") + text;
  if (image) { const i = document.createElement("img"); i.src = "data:image/jpeg;base64," + image; d.appendChild(i); }
  if (extra) d.appendChild(extra);
  log.appendChild(d); log.scrollTop = log.scrollHeight;
  return d;
};
const running = (on, label) => {
  $("stop").disabled = !on; $("start").disabled = on; $("status").textContent = label || (on ? "running" : "");
  $("msgrow").style.display = on ? "flex" : "none";
};
async function refresh() {
  const s = await invoke("get_settings");
  const probs = [...s.problems];
  if (s.checks && s.checks.blocked) probs.push(s.checks.blocked);
  $("problems").textContent = probs.length ? "Before you start, open Settings:\n" + probs.join("\n") : "";
  if (s.checks && s.checks.update && !shownUpdate) { shownUpdate = true; add("muted", s.checks.update); }
  if ($("stop").disabled) $("start").disabled = s.problems.length > 0;
}
async function start(t) {
  try { await invoke("start_task", { task: t }); running(true); }
  catch (e) { add("error", String(e)); }
}
$("settings").onclick = () => invoke("open_settings");
$("start").onclick = () => {
  const t = $("task").value.trim(); if (!t) return;
  log.textContent = ""; add("text", "Task: " + t); start(t);
};
$("stop").onclick = () => invoke("stop_task");
let shownUpdate = false;
let hadError = false;
// a later message from the person: the shell writes it to the task file (the gate's trusted channel), then to Pi
const sendMsg = async () => {
  const t = $("message").value.trim(); if (!t) return;
  try { await invoke("send_message", { text: t }); add("text", "You: " + t); $("message").value = ""; }
  catch (e) { add("error", String(e)); }
};
$("send").onclick = sendMsg;
$("message").onkeydown = (e) => { if (e.key === "Enter") sendMsg(); };
listen("agent", (e) => {
  const ev = e.payload;
  // a task the window did not start itself (--run, Restart) still shows as running (B10)
  if (ev.kind === "started") { hadError = false; running(true); if (!log.textContent) add("text", "Task: " + ev.text); refresh(); return; }
  if (ev.kind === "error") hadError = true;
  // B2: a failed model call is never shown as a normal finish
  if (ev.kind === "done") { $("status").textContent = hadError ? "failed" : ev.text === "aborted" ? "aborted" : "done"; return; }
  if (ev.kind === "stopped") {
    const failed = ev.status === "error";
    running(false, failed ? "failed" : ev.text);
    add(failed ? "error" : "muted", "Agent " + ev.text); refresh(); return;
  }
  if (ev.kind === "crashed") {
    running(false, "crashed");
    const b = document.createElement("button"); b.textContent = "Restart the task"; b.className = "primary";
    b.onclick = async () => { b.disabled = true; add("text", "Restarting the task"); try { await invoke("restart_task"); running(true); } catch (err) { add("error", String(err)); } };
    add("error", ev.text, null, document.createElement("br")).appendChild(b);
    refresh();
    return;
  }
  if (ev.kind === "gate_exit") { add("error", "The safety gate stopped: " + ev.text + "\nNo GUI action can run until the task is restarted."); return; }
  if (ev.kind === "ui_request") return;
  add(ev.status === "unknown" ? "gate unknown" : ev.kind, ev.text, ev.image);
});
// every person question ends in the timeline with its outcome; anything but a yes / an answer is a refusal
listen("human", (e) => {
  const p = e.payload;
  const noAnswer = ["timed out", "expired", "window closed", "cancelled", "unavailable"].includes(p.outcome);
  const why = p.outcome === "expired" ? " (never shown: another question was open)" : "";
  add("human" + (noAnswer ? " muted" : p.refused ? " refused" : ""), `asked the person (${p.kind}): ${noAnswer ? "no answer (" + p.outcome + ")" : p.outcome}${why}${noAnswer ? " - not a refusal; the step was not done" : p.refused ? " - the step was not done" : ""}`);
});
window.addEventListener("focus", refresh);
// B10: saved settings and the finished start-up checks update the "before you start" list right away
listen("settings_changed", refresh);
listen("checks_done", refresh);
refresh();
