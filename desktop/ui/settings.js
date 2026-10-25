const { invoke } = window.__TAURI__.core;
const MAIN_P = ["openai-completions", "openai-responses", "anthropic-messages", "google-generative-ai"];
const GATE_P = ["chat", "responses", "anthropic", "gemini", "systemone"];
let state = null;
const el = (tag, attrs = {}, kids = []) => { const e = document.createElement(tag); Object.assign(e, attrs); kids.forEach((k) => e.appendChild(k)); return e; };
const row = (label, input) => el("div", { className: "row" }, [el("label", { textContent: label }), input]);
const select = (opts, v) => { const s = el("select"); opts.forEach((o) => s.appendChild(el("option", { value: o, textContent: o, selected: o === v }))); return s; };

function modelForm(box, m, protocols, slot, isMain) {
  box.textContent = "";
  const proto = select(protocols, m.protocol); proto.onchange = () => (m.protocol = proto.value);
  const url = el("input", { value: m.base_url || "", placeholder: "https://..." }); url.oninput = () => (m.base_url = url.value);
  const model = el("input", { value: m.model || "" }); model.oninput = () => (m.model = model.value);
  const key = el("input", { type: "password", placeholder: state.keys[slot] }); 
  const setKey = el("button", { textContent: "Save key" }); setKey.onclick = async () => { state = await invoke("set_key", { slot, value: key.value }); key.value = ""; render(); };
  const test = el("button", { textContent: "Test connection" }); const res = el("span", { className: "muted" });
  test.onclick = async () => { res.textContent = "testing..."; const r = await invoke("test_model", { slot }); res.textContent = r.ok ? `ok (${r.ms} ms)` : `failed: ${r.error}`; };
  box.append(row("Protocol", proto), row("Base URL", url), row("Model id", model), row("API key", el("div", {}, [key, setKey])));
  if (isMain) {
    const th = select(["medium", "high", "xhigh"], m.thinking || "medium"); th.onchange = () => (m.thinking = th.value);
    const vis = el("input", { type: "checkbox", checked: m.vision !== false }); vis.onchange = () => (m.vision = vis.checked);
    box.append(row("Thinking", th), row("Reads images", vis));
  } else {
    const to = el("input", { type: "number", value: m.timeout_s || 60 }); to.oninput = () => (m.timeout_s = Number(to.value));
    box.append(row("Timeout (s)", to));
  }
  box.append(row("", el("div", {}, [test, res])));
}

function render() {
  const s = state.settings;
  document.getElementById("store").textContent = state.store === "keychain" ? "Keys are kept in the system keychain." : "No system keychain here: keys are kept in memory for this run only.";
  modelForm(document.getElementById("main"), s.main, MAIN_P, "main", true);
  modelForm(document.getElementById("reviewer"), s.reviewer, GATE_P, "reviewer", false);
  const roles = document.getElementById("roles"); roles.textContent = "";
  for (const r of state.roles) {
    const cur = s.roles[r] || { mode: "off" };
    const mode = select(["off", "reviewer", "custom"], cur.mode);
    const sub = el("div");
    const draw = () => {
      sub.textContent = "";
      if (mode.value === "custom") { if (!s.roles[r].model) s.roles[r] = { mode: "custom", model: { protocol: "chat", base_url: "", model: "", timeout_s: 120 } }; modelForm(sub, s.roles[r].model, GATE_P, "role." + r, false); }
    };
    mode.onchange = () => { s.roles[r] = mode.value === "custom" ? { mode: "custom", model: (s.roles[r] && s.roles[r].model) || { protocol: "chat", base_url: "", model: "", timeout_s: 120 } } : { mode: mode.value }; draw(); };
    roles.append(row(r, mode), sub); draw();
  }
  document.getElementById("timeout").value = s.human_timeout_s;
  document.getElementById("problems").textContent = state.problems.join("\n");
}
document.getElementById("save").onclick = async () => {
  state.settings.human_timeout_s = Number(document.getElementById("timeout").value) || 120;
  try { state = await invoke("save_settings", { settings: state.settings }); document.getElementById("saved").textContent = "saved"; render(); }
  catch (e) { document.getElementById("saved").textContent = String(e); }
};
invoke("get_settings").then((s) => { state = s; render(); });
