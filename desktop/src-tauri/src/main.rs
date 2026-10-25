// gui-pi-desktop shell: settings (keys in the OS keychain), the agent process (Pi RPC), and the ask-a-person
// popups. All decisions about actions stay in gui-gate; this shell only shows the person what the gate asks.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gpd_core::config::{slot_main, slot_reviewer, slot_role, Settings, ROLES};
use gpd_core::human::{self, Outcome, Popup, Request};
use gpd_core::keys::{preview, KeyStore, MemoryStore, SERVICE};
use gpd_core::launch::{self, Paths};
use gpd_core::rpc;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

/// The OS keychain (Secret Service / Credential Manager / Keychain). No plain-text fallback.
struct KeyringStore;

impl KeyStore for KeyringStore {
    fn get(&self, slot: &str) -> Option<String> {
        keyring::Entry::new(SERVICE, slot).ok()?.get_password().ok()
    }
    fn set(&self, slot: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, slot).map_err(|e| e.to_string())?.set_password(value.trim()).map_err(|e| e.to_string())
    }
    fn delete(&self, slot: &str) -> Result<(), String> {
        match keyring::Entry::new(SERVICE, slot).map_err(|e| e.to_string())?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
    fn kind(&self) -> &'static str {
        "keychain"
    }
}

/// Is a keychain usable here? (Linux without a Secret Service daemon is the usual "no".)
fn keychain_works() -> bool {
    let e = match keyring::Entry::new(SERVICE, "__probe__") { Ok(e) => e, Err(_) => return false };
    if e.set_password("probe").is_err() {
        return false;
    }
    let _ = e.delete_credential();
    true
}

struct AppState {
    settings: Mutex<Settings>,
    keys: Mutex<Arc<dyn KeyStore>>,
    agent: Mutex<Option<Child>>,
    agent_in: Mutex<Option<std::process::ChildStdin>>,
    popups: Mutex<HashMap<String, Sender<Outcome>>>,
    ipc: Mutex<Option<(String, String)>>,
}

fn config_dir(app: &AppHandle) -> PathBuf {
    app.path().app_config_dir().unwrap_or_else(|_| PathBuf::from(".gui-pi-desktop"))
}
fn data_dir(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from(".gui-pi-desktop-data"))
}

fn private_dir(p: &PathBuf) {
    let _ = std::fs::create_dir_all(p);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700));
    }
}

/// Bundled resources (node, pi, extension, prompts, skills, gate, driver). GUI_PI_RESOURCES overrides for dev.
fn paths(app: &AppHandle) -> Paths {
    let res = std::env::var("GUI_PI_RESOURCES").map(PathBuf::from)
        .unwrap_or_else(|_| app.path().resource_dir().map(|r| r.join("resources")).unwrap_or_else(|_| PathBuf::from("resources")));
    let exe = |n: &str| if cfg!(windows) { format!("{n}.exe") } else { n.to_string() };
    let driver = res.join("cua-driver").join(exe("cua-driver"));
    Paths {
        node: res.join("node").join("bin").join(exe("node")),
        pi_cli: res.join("pi").join("node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js"),
        extension: res.join("gui-pi").join("index.ts"),
        skills: res.join("skills"),
        system_prompt: res.join("prompts").join("system.md"),
        gate: res.join(exe("gui-gate")),
        driver: if driver.exists() { Some(driver) } else { None },
        data: data_dir(app),
    }
}

// ------------------------------------------------------------------ settings commands
#[tauri::command]
fn get_settings(state: tauri::State<AppState>) -> Value {
    let s = state.settings.lock().unwrap().clone();
    let keys = state.keys.lock().unwrap().clone();
    let mut slots: Vec<String> = vec![slot_main(), slot_reviewer()];
    slots.extend(ROLES.iter().map(|r| slot_role(r)));
    let status: HashMap<String, String> = slots.iter().map(|k| (k.clone(), keys.get(k).map(|v| preview(&v)).unwrap_or_else(|| "not set".into()))).collect();
    let errs = s.validate(&|slot| keys.has(slot));
    json!({"settings": s, "keys": status, "store": keys.kind(), "problems": errs, "roles": ROLES})
}

#[tauri::command]
fn save_settings(app: AppHandle, state: tauri::State<AppState>, settings: Settings) -> Result<Value, String> {
    let dir = config_dir(&app);
    private_dir(&dir);
    std::fs::write(dir.join("config.json"), serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    *state.settings.lock().unwrap() = settings;
    Ok(get_settings(state))
}

#[tauri::command]
fn set_key(state: tauri::State<AppState>, slot: String, value: String) -> Result<Value, String> {
    let ok_slot = slot == slot_main() || slot == slot_reviewer() || ROLES.iter().any(|r| slot == slot_role(r));
    if !ok_slot {
        return Err("unknown key slot".into());
    }
    let keys = state.keys.lock().unwrap().clone();
    if value.trim().is_empty() { keys.delete(&slot)? } else { keys.set(&slot, &value)? }
    Ok(get_settings(state))
}

/// "This run only": keys kept in memory when the machine has no keychain.
#[tauri::command]
fn use_memory_keys(state: tauri::State<AppState>) -> Value {
    *state.keys.lock().unwrap() = Arc::new(MemoryStore::default());
    get_settings(state)
}

/// Send one tiny request to a configured model and report the result (no key is ever returned).
#[tauri::command]
fn test_model(state: tauri::State<AppState>, slot: String) -> Value {
    let s = state.settings.lock().unwrap().clone();
    let keys = state.keys.lock().unwrap().clone();
    let (m, gate_proto) = if slot == slot_main() {
        (s.main.clone(), false)
    } else if slot == slot_reviewer() {
        (s.reviewer.clone(), true)
    } else if let Some(r) = ROLES.iter().find(|r| slot == slot_role(r)) {
        match s.roles.get(*r) { Some(gpd_core::config::RoleCfg::Custom { model }) => (model.clone(), true), _ => return json!({"ok": false, "error": "role is not a custom endpoint"}) }
    } else {
        return json!({"ok": false, "error": "unknown slot"});
    };
    let Some(key) = keys.get(&slot) else { return json!({"ok": false, "error": "no key set"}) };
    let t0 = std::time::Instant::now();
    let r = probe(&m, &key, gate_proto);
    json!({"ok": r.is_ok(), "ms": t0.elapsed().as_millis() as u64, "error": r.err()})
}

fn probe(m: &gpd_core::config::ModelCfg, key: &str, gate_proto: bool) -> Result<(), String> {
    let base = m.base_url.trim().trim_end_matches('/');
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).build();
    let proto = m.protocol.as_str();
    let res = match proto {
        "anthropic-messages" | "anthropic" => agent.post(&if base.ends_with("/messages") { base.to_string() } else { format!("{base}/messages") })
            .set("x-api-key", key).set("anthropic-version", "2023-06-01")
            .send_json(json!({"model": m.model, "max_tokens": 8, "messages": [{"role": "user", "content": "ping"}]})),
        "google-generative-ai" | "gemini" => agent.post(&format!("{base}/models/{}:generateContent", m.model)).set("x-goog-api-key", key)
            .send_json(json!({"contents": [{"parts": [{"text": "ping"}]}]})),
        "openai-responses" | "responses" => agent.post(&if base.ends_with("/responses") { base.to_string() } else { format!("{base}/responses") })
            .set("Authorization", &format!("Bearer {key}")).send_json(json!({"model": m.model, "input": "ping", "max_output_tokens": 16})),
        _ => {
            let url = if base.ends_with("/chat/completions") || (gate_proto && proto == "systemone") { base.to_string() } else { format!("{base}/chat/completions") };
            agent.post(&url).set("Authorization", &format!("Bearer {key}"))
                .send_json(json!({"model": m.model, "max_tokens": 8, "messages": [{"role": "user", "content": "ping"}]}))
        }
    };
    match res {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(c, r)) => Err(format!("HTTP {c}: {}", r.into_string().unwrap_or_default().chars().take(200).collect::<String>())),
        Err(e) => Err(e.to_string()),
    }
}

// ------------------------------------------------------------------ popups
/// Show one popup window and block until the person answers, closes it, or `seconds` pass.
fn show_popup(app: &AppHandle, req: &Request, seconds: f64) -> Outcome {
    let state = app.state::<AppState>();
    let pid = format!("popup-{}", req.id.chars().filter(|c| c.is_ascii_alphanumeric()).take(24).collect::<String>());
    let (tx, rx) = channel::<Outcome>();
    state.popups.lock().unwrap().insert(pid.clone(), tx.clone());
    let payload = json!({"kind": req.kind, "message": req.message, "seconds": seconds, "meta": req.meta});
    let url = format!("popup.html#{}", urlencode(&payload.to_string()));
    let takeover = req.kind == "takeover" || req.kind == "ask_help";
    let built = WebviewWindowBuilder::new(app, &pid, WebviewUrl::App(url.into()))
        .title(match req.kind.as_str() { "approve" => "Approve this step?", "answer" => "A question for you", "takeover" => "Your turn", _ => "The agent is stuck" })
        .inner_size(460.0, if takeover { 230.0 } else { 260.0 })
        .always_on_top(true).focused(true).resizable(false)
        .build();
    let win = match built {
        Ok(w) => w,
        Err(_) => { state.popups.lock().unwrap().remove(&pid); return Outcome::Closed; }
    };
    if takeover {
        // out of the way: the person works in the app behind it
        if let Ok(Some(m)) = win.current_monitor() {
            let sz = m.size();
            let _ = win.set_position(tauri::PhysicalPosition::new(sz.width as i32 - 500, 40));
        }
    }
    let _ = win.request_user_attention(Some(tauri::UserAttentionType::Critical));
    let tx_close = tx.clone();
    win.on_window_event(move |e| if let tauri::WindowEvent::Destroyed = e { let _ = tx_close.send(Outcome::Closed); });
    #[cfg(feature = "test-hooks")]
    if let Ok(rules) = std::env::var("GUI_PI_HUMAN_AUTO") {
        // test builds: the popup is on screen; a script "clicks" it after delay_ms
        let rules: Value = serde_json::from_str(&std::fs::read_to_string(&rules).unwrap_or(rules)).unwrap_or(json!({}));
        let delay = rules.get("delay_ms").and_then(|d| d.as_u64()).unwrap_or(1500);
        let kind = req.kind.clone();
        let tx2 = tx.clone();
        std::thread::spawn(move || {
            static COUNTER: std::sync::OnceLock<Mutex<HashMap<String, usize>>> = std::sync::OnceLock::new();
            std::thread::sleep(Duration::from_millis(delay));
            let o = human::auto_outcome(&rules, &kind, COUNTER.get_or_init(Default::default));
            if o != Outcome::Timeout { let _ = tx2.send(o); }
        });
    }
    let out = rx.recv_timeout(Duration::from_secs_f64(seconds)).unwrap_or(Outcome::Timeout);
    state.popups.lock().unwrap().remove(&pid);
    let _ = win.destroy();
    let _ = app.emit("human", json!({"kind": req.kind, "message": req.message, "outcome": format!("{out:?}")}));
    out
}

fn urlencode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// The popup's buttons call this.
#[tauri::command]
fn popup_answer(window: tauri::WebviewWindow, state: tauri::State<AppState>, choice: String, text: Option<String>) {
    let o = match choice.as_str() {
        "yes" => Outcome::Yes,
        "no" => Outcome::No(text.unwrap_or_default()),
        "text" => Outcome::Text(text.unwrap_or_default()),
        _ => Outcome::Closed,
    };
    if let Some(tx) = state.popups.lock().unwrap().get(window.label()) {
        let _ = tx.send(o);
    }
}

fn start_human_service(app: &AppHandle) -> Result<(String, String), String> {
    let token = human::new_token();
    let a = app.clone();
    let popup: Popup = Arc::new(move |req, secs| show_popup(&a, req, secs));
    #[cfg(unix)]
    {
        let base = std::env::var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|_| data_dir(app).join("run"));
        let path = base.join("gui-pi-desktop").join(format!("{}.sock", std::process::id()));
        human::serve_unix(&path, token.clone(), popup).map_err(|e| e.to_string())?;
        Ok((path.display().to_string(), token))
    }
    #[cfg(not(unix))]
    {
        let port = human::serve_tcp(token.clone(), popup).map_err(|e| e.to_string())?;
        Ok((format!("tcp:127.0.0.1:{port}"), token))
    }
}

// ------------------------------------------------------------------ the agent
#[tauri::command]
fn start_task(app: AppHandle, state: tauri::State<AppState>, task: String) -> Result<(), String> {
    if state.agent.lock().unwrap().is_some() {
        return Err("a task is already running".into());
    }
    let s = state.settings.lock().unwrap().clone();
    let keys = state.keys.lock().unwrap().clone();
    let ipc = state.ipc.lock().unwrap().clone();
    let p = paths(&app);
    private_dir(&p.data);
    private_dir(&p.data.join("logs"));
    let l = launch::build(&s, keys.as_ref(), &p, &task, ipc.as_ref().map(|(a, b)| (a.as_str(), b.as_str()))).map_err(|e| e.join("\n"))?;
    for (f, c) in &l.files {
        if let Some(d) = f.parent() { private_dir(&d.to_path_buf()); }
        std::fs::write(f, c).map_err(|e| e.to_string())?;
    }
    let mut cmd = Command::new(&l.program);
    cmd.args(&l.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).current_dir(&p.data);
    for (k, _) in std::env::vars() {
        if l.env_remove_prefixes.iter().any(|pfx| k.starts_with(pfx)) { cmd.env_remove(&k); }
    }
    cmd.envs(l.env.iter().map(|(k, v)| (k, v)));
    let mut child = cmd.spawn().map_err(|e| format!("cannot start the agent: {e}"))?;
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let log_path = p.data.join("logs").join("agent.jsonl");
    let a = app.clone();
    std::thread::spawn(move || {
        let mut log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).ok();
        let mut buf = String::new();
        let mut chunk = [0u8; 65536];
        loop {
            let n = match stdout.read(&mut chunk) { Ok(0) | Err(_) => break, Ok(n) => n };
            buf.push_str(&String::from_utf8_lossy(&chunk[..n]));
            for line in rpc::split_records(&mut buf) {
                if let Some(f) = log.as_mut() { let _ = writeln!(f, "{line}"); }
                if let Some(ev) = rpc::ui_event(&line) {
                    if std::env::var("GUI_PI_HEADLESS").is_ok() { println!("{}", serde_json::to_string(&ev).unwrap_or_default()); }
                    let done = ev.kind == "done";
                    let _ = a.emit("agent", &ev);
                    if done { finish(&a, 0); }
                }
            }
        }
        finish(&a, 1);
    });
    std::thread::spawn(move || {
        let mut s = String::new();
        let mut r = std::io::BufReader::new(stderr);
        let _ = r.read_to_string(&mut s);
        if !s.trim().is_empty() { eprintln!("{}", s.chars().rev().take(4000).collect::<String>().chars().rev().collect::<String>()); }
    });
    writeln!(stdin, "{}", rpc::prompt("task-1", &task)).map_err(|e| e.to_string())?;
    *state.agent_in.lock().unwrap() = Some(stdin);
    *state.agent.lock().unwrap() = Some(child);
    Ok(())
}

fn finish(app: &AppHandle, code: i32) {
    let state = app.state::<AppState>();
    let child = state.agent.lock().unwrap().take();
    *state.agent_in.lock().unwrap() = None; // closing stdin ends Pi; Pi's extension ends gui-gate, which ends the driver
    if let Some(mut c) = child {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if let Ok(Some(_)) = c.try_wait() { break; }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = c.kill();
        let _ = app.emit("agent", json!({"kind": "stopped", "text": ""}));
        if std::env::var("GUI_PI_HEADLESS").is_ok() { app.exit(code); }
    }
}

#[tauri::command]
fn stop_task(app: AppHandle, state: tauri::State<AppState>) {
    if let Some(w) = state.agent_in.lock().unwrap().as_mut() {
        let _ = writeln!(w, "{}", rpc::abort());
    }
    finish(&app, 2);
}

#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("settings") { let _ = w.set_focus(); return Ok(()); }
    WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("settings.html".into())).title("Settings").inner_size(760.0, 820.0)
        .build().map(|_| ()).map_err(|e| e.to_string())
}

fn main() {
    // `gui-pi-desktop --run "task"`: run one task with the saved settings, popups still shown, events on stdout
    let argv: Vec<String> = std::env::args().collect();
    let run_task = argv.iter().position(|a| a == "--run").and_then(|i| argv.get(i + 1).cloned());
    if run_task.is_some() { std::env::set_var("GUI_PI_HEADLESS", "1"); }
    tauri::Builder::default()
        .setup(move |app| {
            let h = app.handle().clone();
            let cfg = config_dir(&h).join("config.json");
            let settings = std::fs::read_to_string(&cfg).ok().and_then(|j| Settings::load(&j).ok()).unwrap_or_default();
            let keys: Arc<dyn KeyStore> = if keychain_works() { Arc::new(KeyringStore) } else { Arc::new(MemoryStore::default()) };
            app.manage(AppState { settings: Mutex::new(settings), keys: Mutex::new(keys), agent: Mutex::new(None), agent_in: Mutex::new(None),
                                  popups: Mutex::new(HashMap::new()), ipc: Mutex::new(None) });
            match start_human_service(&h) {
                Ok(x) => *h.state::<AppState>().ipc.lock().unwrap() = Some(x),
                Err(e) => eprintln!("ask-a-person service failed: {e} (risky steps will be refused)"),
            }
            if let Some(t) = run_task.clone() {
                let h2 = h.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(500));
                    if let Err(e) = start_task(h2.clone(), h2.state::<AppState>(), t) { eprintln!("{e}"); h2.exit(3); }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_settings, save_settings, set_key, use_memory_keys, test_model, popup_answer, start_task, stop_task, open_settings])
        .run(tauri::generate_context!())
        .expect("error while running gui-pi-desktop");
}
