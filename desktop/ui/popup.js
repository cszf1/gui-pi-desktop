// One question from the gate. Buttons map to the gate's form in the shell (gpd-core human::to_elicit).
// No answer before the bar runs out = timed out; closing the window = cancelled. Both are refusals.
// Only the first answer counts (the shell ignores any later one).
const { invoke } = window.__TAURI__.core;
const q = JSON.parse(decodeURIComponent(location.hash.slice(1)));
document.getElementById("msg").textContent = q.message;
const btns = document.getElementById("btns");
const text = document.getElementById("text");
let sent = false;
const send = (choice, t) => {
  if (sent) return;
  sent = true;
  btns.querySelectorAll("button").forEach((b) => (b.disabled = true));
  invoke("popup_answer", { choice, text: t ?? null });
};
const button = (label, cls, fn) => { const b = document.createElement("button"); b.textContent = label; if (cls) b.className = cls; b.onclick = fn; btns.appendChild(b); return b; };
if (q.kind === "approve") {
  button("Deny", "", () => send("no"));
  button("Allow", "primary", () => send("yes"));
} else if (q.kind === "answer") {
  text.style.display = "block"; text.focus();
  text.onkeydown = (e) => { if (e.key === "Enter" && text.value.trim()) send("text", text.value); };
  button("Don't answer", "", () => send("no"));
  button("Send", "primary", () => text.value.trim() && send("text", text.value));
} else if (q.kind === "takeover") {
  button("No", "", () => send("no"));
  button("I did it, continue", "primary", () => send("yes"));
} else {
  text.style.display = "block"; text.placeholder = "A hint for the agent (optional)";
  button("Let it try another way", "", () => send("no", text.value));
  button("I did this step myself, continue", "primary", () => send("yes"));
}
const total = Math.max(1, Number(q.seconds) || 120);
const t0 = Date.now();
const tick = () => {
  const left = Math.max(0, total - (Date.now() - t0) / 1000);
  document.getElementById("bar").style.width = (100 * left / total) + "%";
  document.getElementById("left").textContent = `${Math.ceil(left)} s left - no answer counts as "no"`;
  if (left <= 0) send("timeout"); else if (!sent) setTimeout(tick, 500);
};
tick();
