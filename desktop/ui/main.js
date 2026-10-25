const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const log = document.getElementById("log");
const add = (kind, text, image) => {
  const d = document.createElement("div"); d.className = "ev " + kind; d.textContent = (kind === "tool" ? "→ " : "") + text;
  if (image) { const i = document.createElement("img"); i.src = "data:image/jpeg;base64," + image; d.appendChild(i); }
  log.appendChild(d); log.scrollTop = log.scrollHeight;
};
async function refresh() {
  const s = await invoke("get_settings");
  document.getElementById("problems").textContent = s.problems.length ? "Before you start, open Settings:\n" + s.problems.join("\n") : "";
  document.getElementById("start").disabled = s.problems.length > 0;
}
document.getElementById("settings").onclick = () => invoke("open_settings");
document.getElementById("start").onclick = async () => {
  const t = document.getElementById("task").value.trim(); if (!t) return;
  log.textContent = ""; add("text", "Task: " + t);
  try { await invoke("start_task", { task: t }); document.getElementById("stop").disabled = false; document.getElementById("start").disabled = true; document.getElementById("status").textContent = "running"; }
  catch (e) { add("error", String(e)); }
};
document.getElementById("stop").onclick = () => invoke("stop_task");
listen("agent", (e) => {
  const ev = e.payload;
  if (ev.kind === "done" || ev.kind === "stopped") { document.getElementById("stop").disabled = true; document.getElementById("status").textContent = ev.kind; refresh(); return; }
  if (ev.kind !== "ui_request") add(ev.kind, ev.text, ev.image);
});
listen("human", (e) => add("human", `asked the person (${e.payload.kind}): ${e.payload.outcome}`));
window.addEventListener("focus", refresh);
refresh();
