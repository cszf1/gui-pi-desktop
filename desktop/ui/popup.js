// One shared component for the four result types the gate asks for (the shell builds `view`, gpd-core human::view):
//   approve   the gate-confirmed object, scope and consequence; Deny / Allow
//   clarify   the question, each option with how it changes what happens next; or free text; Don't answer
//   takeover  the minimal steps for the person; the agent's input is paused; Done never re-runs the action
//   help      what the agent tried; "Give a hint" (text) / "I did it for you"
// No answer before the bar runs out = timed out; closing the window = cancelled. Both are "no answer": not a refusal
// by the person, but the step is still not done.
// Only the first answer counts (the shell ignores any later one).
// Person metrics (gui-pi-human/2): the shell times how long the popup was up (wait_s). This page reports only what it
// can prove of the person's own activity: from the first input inside the popup (key, click, typing, scrolling,
// pointer movement) to the answer. No input before the answering click -> "answer_only"; none at all -> "none".
// Waiting is never reported as active time; a takeover's work happens in other windows and is not measured here.
const { invoke } = window.__TAURI__.core;
const q = JSON.parse(decodeURIComponent(location.hash.slice(1)));
const v = q.view || { type: "approve", message: q.message, buttons: [{ choice: "no", label: "Deny" }, { choice: "yes", label: "Allow" }] };
const $ = (id) => document.getElementById(id);
const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text !== undefined) e.textContent = text; return e; };
$("msg").textContent = v.message || q.message || "";
const facts = $("facts");
const fact = (label, value) => { if (!value) return; const r = el("div", "fact"); r.append(el("b", "", label + ": "), el("span", "", value)); facts.appendChild(r); };
const list = (label, items) => {
  if (!items || !items.length) return;
  facts.appendChild(el("div", "fact", label));
  const ol = el("ol"); items.forEach((s) => ol.appendChild(el("li", "", s))); facts.appendChild(ol);
};
const btns = $("btns");
const text = $("text");
let sent = false;
let firstInput = null;
let lastInput = null;
let moves = 0;
const evidence = (e) => {
  if (sent) return;
  if (e.type === "pointermove" && ++moves < 3) return;  // a pointer merely passing over is not evidence yet
  const t = Date.now();
  if (firstInput === null) firstInput = t;
  lastInput = t;
};
for (const ev of ["keydown", "pointerdown", "input", "wheel", "pointermove"]) document.addEventListener(ev, evidence, { capture: true, passive: true });
const send = (choice, t) => {
  if (sent) return;
  const now = Date.now();
  // the answering click itself is one pointerdown just before this call: it proves the answer, not time spent
  const before = firstInput !== null && now - firstInput > 300;
  const answered = choice !== "timeout";
  const active_ms = answered ? (before ? now - firstInput : null) : (firstInput !== null && lastInput > firstInput ? lastInput - firstInput : null);
  const ev = active_ms !== null ? "input" : firstInput !== null ? "answer_only" : "none";
  sent = true;
  document.querySelectorAll("button").forEach((b) => (b.disabled = true));
  invoke("popup_answer", { choice, text: t ?? null, activeMs: active_ms, evidence: ev, kind: q.kind || v.kind || null });
};
const button = (label, cls, fn, parent = btns, choice) => { const b = el("button", cls, label); b.onclick = fn; if (choice) b.dataset.choice = choice; parent.appendChild(b); return b; };

if (v.type === "approve") {
  fact("What", v.object); fact("Scope", v.scope); fact("If you allow", v.consequence); fact("Why the gate asks", v.reason);
  if ((v.missing || []).length) facts.appendChild(el("div", "muted", `The gate did not say: ${v.missing.join(", ")}. If you are not sure what this does, deny.`));
  if (v.only) facts.appendChild(el("div", "fact", v.only));
  button("Deny", "", () => send("no"), btns, "no");
  button("Allow", "primary", () => send("yes"), btns, "yes");
} else if (v.type === "clarify") {
  const opts = v.options || [];
  if (opts.length) {
    const box = el("div", "options");
    opts.forEach((o) => {
      const b = el("button", "option");
      b.append(el("b", "", o.label), el("div", "muted", o.effect || ""));
      b.onclick = () => send("text", o.value || o.label);
      box.appendChild(b);
    });
    facts.appendChild(box);
  }
  if (v.free_text !== false) {
    text.style.display = "block"; text.placeholder = opts.length ? "Or type another answer" : "Your answer"; text.focus();
    text.onkeydown = (e) => { if (e.key === "Enter" && text.value.trim()) send("text", text.value); };
    button("Don't answer", "", () => send("no"), btns, "no");
    button("Send", "primary", () => text.value.trim() && send("text", text.value), btns, "text");
  } else button("Don't answer", "", () => send("no"), btns, "no");
} else if (v.type === "takeover") {
  list("Please do this yourself:", v.steps);
  if (v.more_steps) facts.appendChild(el("div", "muted", `(${v.more_steps} more steps not shown; the gate sent more than the minimal ones)`));
  facts.appendChild(el("div", "muted", `${v.paused || ""} ${v.note || ""}`));
  button("I won't do it", "", () => send("no"), btns, "no");
  button("Done", "primary", () => send("yes"), btns, "yes");
} else {
  list(v.tried_source === "agent log" ? "What the agent tried (from its log):" : "What the agent tried:", v.tried);
  fact("Why it is stuck", v.reason);
  if (v.choices) facts.appendChild(el("div", "muted", v.choices));
  text.style.display = "block"; text.placeholder = "A hint for the agent";
  button("Give a hint", "", () => send("no", text.value), btns, "no");
  button("I did it for you", "primary", () => send("yes"), btns, "yes");
}
// the keyboard default (Enter / Space) is the refusing / not-doing button (view.default = "no"), never Allow / Done;
// a free-text question keeps the focus in its text box (Enter sends only a typed answer)
const defaultChoice = v.default || "no";
if (text.style.display !== "block") {
  const d = [...btns.querySelectorAll("button")].find((b) => b.dataset.choice === defaultChoice);
  if (d) d.focus();
}
// what happens when nobody answers (gate: no_answer). Not a refusal by the person; the step is still not done.
const NO_ANSWER = {
  approve: "if nobody answers, the step is not done",
  clarify: "if nobody answers, nothing is chosen and the step is not done",
  takeover: "if nobody answers, the step stays undone",
  help: "if nobody answers, the agent gets no help and does not go on with this step",
};
const total = Math.max(1, Number(q.seconds) || 120);
const t0 = Date.now();
const tick = () => {
  const left = Math.max(0, total - (Date.now() - t0) / 1000);
  $("bar").style.width = (100 * left / total) + "%";
  $("left").textContent = `${Math.ceil(left)} s left - ${NO_ANSWER[v.type] || NO_ANSWER.approve}`;
  if (left <= 0) send("timeout"); else if (!sent) setTimeout(tick, 500);
};
tick();

// size the window to its content (the shell clamps it to the screen); again when fonts / layout settle. Then check
// what the page really got: on macOS the requested size came out a title bar short (the countdown line was cut
// off and a scroll bar showed), so the shortfall is added and asked for once more. `seen` is logged by the shell.
let extra = 0;
const need = () => Math.ceil(document.querySelector(".pop").getBoundingClientRect().height) + 4;
let queue = Promise.resolve();
const fit = () => (queue = queue.then(fitOnce));  // one at a time: two overlapping fits would add the shortfall twice
const fitOnce = async () => {
  try {
    await invoke("popup_fit", { height: need() + extra, need: need(), seen: window.innerHeight });
    await new Promise((r) => setTimeout(r, 150));
    const short = need() - window.innerHeight;
    if (short > 1 && extra < 240) {
      extra += short;
      await invoke("popup_fit", { height: need() + extra, need: need(), seen: window.innerHeight });
    }
  } catch (_) { /* not in the shell */ }
};
fit(); window.addEventListener("load", fit); setTimeout(fit, 300); setTimeout(() => { queue = queue.then(() => invoke("popup_fit", { height: need() + extra, need: need(), seen: window.innerHeight }).catch(() => {})); }, 900);
