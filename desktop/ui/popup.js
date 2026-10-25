// One shared component for the four result types the gate asks for (the shell builds `view`, gpd-core human::view):
//   approve   the gate-confirmed object, scope and consequence; Deny / Allow
//   clarify   the question, each option with how it changes what happens next; or free text; Don't answer
//   takeover  the minimal steps for the person; the agent's input is paused; Done never re-runs the action
//   help      what the agent tried; "Give a hint" (text) / "I did it for you"
// No answer before the bar runs out = timed out; closing the window = cancelled. Both are refusals.
// Only the first answer counts (the shell ignores any later one).
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
const send = (choice, t) => {
  if (sent) return;
  sent = true;
  document.querySelectorAll("button").forEach((b) => (b.disabled = true));
  invoke("popup_answer", { choice, text: t ?? null });
};
const button = (label, cls, fn, parent = btns) => { const b = el("button", cls, label); b.onclick = fn; parent.appendChild(b); return b; };

if (v.type === "approve") {
  fact("What", v.object); fact("Scope", v.scope); fact("If you allow", v.consequence); fact("Why the gate asks", v.reason);
  button("Deny", "", () => send("no"));
  button("Allow", "primary", () => send("yes"));
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
    button("Don't answer", "", () => send("no"));
    button("Send", "primary", () => text.value.trim() && send("text", text.value));
  } else button("Don't answer", "", () => send("no"));
} else if (v.type === "takeover") {
  list("Please do this yourself:", v.steps);
  facts.appendChild(el("div", "muted", `${v.paused || ""} ${v.note || ""}`));
  button("I won't do it", "", () => send("no"));
  button("Done", "primary", () => send("yes"));
} else {
  list("What the agent tried:", v.tried);
  fact("Why it is stuck", v.reason);
  text.style.display = "block"; text.placeholder = "A hint for the agent";
  button("Give a hint", "", () => send("no", text.value));
  button("I did it for you", "primary", () => send("yes"));
}
const total = Math.max(1, Number(q.seconds) || 120);
const t0 = Date.now();
const tick = () => {
  const left = Math.max(0, total - (Date.now() - t0) / 1000);
  $("bar").style.width = (100 * left / total) + "%";
  $("left").textContent = `${Math.ceil(left)} s left - no answer counts as "no"`;
  if (left <= 0) send("timeout"); else if (!sent) setTimeout(tick, 500);
};
tick();
