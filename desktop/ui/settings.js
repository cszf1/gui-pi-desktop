const { invoke } = window.__TAURI__.core;
const MAIN_P = ["openai-completions", "openai-responses", "anthropic-messages", "google-generative-ai"];
const GATE_P = ["chat", "responses", "anthropic", "gemini", "systemone"];
let state = null;
const el = (tag, attrs = {}, kids = []) => { const e = document.createElement(tag); Object.assign(e, attrs); kids.forEach((k) => e.appendChild(k)); return e; };
const row = (label, input) => el("div", { className: "row" }, [el("label", { textContent: label }), input]);
const select = (opts, v, labels = {}) => { const s = el("select"); opts.forEach((o) => s.appendChild(el("option", { value: o, textContent: labels[o] || o, selected: o === v }))); return s; };

// Same rule as gpd-core config::check_base_url (the Rust side decides; this only gives early feedback).
function urlProblem(u, optional) {
  u = (u || "").trim();
  if (!u) return optional ? "" : "required";
  let x;
  try { x = new URL(u); } catch { return "not a URL"; }
  if (x.username || x.password) return "no user name or password in the URL";
  const h = x.hostname.replace(/^\[|\]$/g, "").toLowerCase();
  const local = h === "localhost" || h === "::1" || /^127\.\d+\.\d+\.\d+$/.test(h);
  if (x.protocol === "https:") return "";
  if (x.protocol === "http:" && local) return "";
  return x.protocol === "http:" ? "use https (plain http only for localhost)" : "must start with https://";
}

function modelForm(box, m, protocols, slot, opts = {}) {
  box.textContent = "";
  const hint = (inp, fn) => { const s = el("span", { className: "problems" }); const upd = () => (s.textContent = fn(inp.value)); inp.addEventListener("input", upd); upd(); return el("div", {}, [inp, s]); };
  const proto = select(opts.optional ? ["", ...protocols] : protocols, m.protocol || "", { "": "(same as main model)" }); proto.onchange = () => (m.protocol = proto.value);
  const url = el("input", { value: m.base_url || "", placeholder: opts.optional ? "(empty: main model's)" : "https://..." }); url.oninput = () => (m.base_url = url.value);
  const model = el("input", { value: m.model || "", placeholder: opts.optional ? "(empty: main model's)" : "" }); model.oninput = () => (m.model = model.value);
  const key = el("input", { type: "password", placeholder: state.keys[slot] + (opts.optional ? " (empty: main model's key)" : ""), autocomplete: "off" });
  // B6: the key goes to the keychain; edits not saved yet (URL, model name, ...) stay in the form
  const setKey = el("button", { textContent: "Save key" });
  setKey.onclick = async () => {
    const edits = state.settings;
    try { state = await invoke("set_key", { slot, value: key.value }); }
    catch (e) { res.textContent = "key not saved: " + String(e); return; }
    state.settings = edits; key.value = ""; render();
  };
  const test = el("button", { textContent: "Test connection" }); const res = el("span", { className: "muted" });
  test.onclick = async () => {
    // B5: save without rebuilding the form, so this result span stays on the page
    res.textContent = "save first, then testing...";
    try { await save(false); } catch { res.textContent = "fix the settings first"; return; }
    res.textContent = "testing...";
    test.disabled = true;
    try {
      const r = await invoke("test_model", { slot });
      res.textContent = r.ok ? `ok (${r.ms} ms)${r.note && r.note !== "ok" ? " - " + r.note : ""}` : `failed: ${r.error}`;
      res.className = r.ok ? "ok" : "fail";
    } catch (e) { res.textContent = "failed: " + String(e); res.className = "fail"; }
    finally { test.disabled = false; }
  };
  box.append(row("Protocol", proto), row("Base URL", hint(url, (v) => urlProblem(v, opts.optional))),
             row("Model name", hint(model, (v) => (!opts.optional && !v.trim() ? "required" : ""))), row("API key", el("div", {}, [key, setKey])));
  if (opts.isMain) {
    const th = select(["medium", "high", "xhigh"], m.thinking || "medium"); th.onchange = () => (m.thinking = th.value);
    const vis = el("input", { type: "checkbox", checked: m.vision !== false }); vis.onchange = () => (m.vision = vis.checked);
    box.append(row("Thinking", th), row("Reads images", vis));
  } else {
    const to = el("input", { type: "number", value: m.timeout_s || 60 }); to.oninput = () => (m.timeout_s = Number(to.value) || null);
    box.append(row("Timeout (s)", to));
  }
  box.append(row("", el("div", {}, [test, res])));
}

const emptyModel = () => ({ protocol: "", base_url: "", model: "", timeout_s: 120 });

function render() {
  const s = state.settings;
  const store = document.getElementById("store");
  store.textContent = state.store_notice;
  store.className = state.store === "keychain" ? "muted" : "notice warn";
  modelForm(document.getElementById("main"), s.main, MAIN_P, "main", { isMain: true });
  modelForm(document.getElementById("reviewer"), s.reviewer, GATE_P, "reviewer");
  const roles = document.getElementById("roles"); roles.textContent = "";
  for (const r of state.roles) {
    const cur = s.roles[r] || { mode: "off" };
    const mode = select(["off", "main", "reviewer", "custom"], cur.mode, { off: "Off", main: "Main model", reviewer: "Same as reviewer", custom: "Custom endpoint" });
    const sub = el("div", { className: "sub" });
    const draw = () => {
      sub.textContent = "";
      if (mode.value === "custom") modelForm(sub, s.roles[r].model, GATE_P, "role." + r, { optional: true });
    };
    mode.onchange = () => {
      s.roles[r] = mode.value === "custom" ? { mode: "custom", model: (s.roles[r] && s.roles[r].model) || emptyModel() } : { mode: mode.value };
      draw();
    };
    roles.append(row(r, mode), sub); draw();
  }
  if (Object.values(s.roles).some((x) => x.mode !== "off")) document.getElementById("advanced").open = true;
  document.getElementById("timeout").value = s.human_timeout_s;
  document.getElementById("updates").checked = s.update_check !== false;
  renderChecks(state.checks || {});
  document.getElementById("problems").textContent = state.problems.join("\n");
}
function renderChecks(c) {
  const box = document.getElementById("checks"); box.textContent = "";
  for (const x of c.system || []) box.appendChild(el("div", { className: x.level, textContent: `${x.level === "ok" ? "✓" : x.level === "warn" ? "!" : "✗"} ${x.message}` }));
  if (c.integrity) box.appendChild(el("div", { className: c.integrity.ok ? (c.integrity.unverified ? "warn" : "ok") : "fail", textContent: c.integrity.message }));
  if (c.update) box.appendChild(el("div", { className: "warn", textContent: c.update }));
  if (c.blocked) box.appendChild(el("div", { className: "fail", textContent: "Start is blocked: " + c.blocked }));
}
document.getElementById("recheck").onclick = async () => { renderChecks(await invoke("self_check")); };
// redraw=false (Test connection): keep the form as it is (its values are what was just saved) and update only the
// problem list, so a result written into the form is not thrown away.
async function save(redraw = true) {
  state.settings.human_timeout_s = Number(document.getElementById("timeout").value) || 120;
  state.settings.update_check = document.getElementById("updates").checked;
  try {
    const edits = state.settings;
    state = await invoke("save_settings", { settings: state.settings });
    document.getElementById("saved").textContent = "saved";
    if (redraw) render();
    else { state.settings = edits; document.getElementById("problems").textContent = state.problems.join("\n"); renderChecks(state.checks || {}); }
  }
  catch (e) { document.getElementById("saved").textContent = String(e); throw e; }
}
document.getElementById("save").onclick = () => save().catch(() => {});
invoke("get_settings").then((s) => { state = s; render(); });
