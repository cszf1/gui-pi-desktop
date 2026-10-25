// gui-pi-desktop shell: settings (keys in the OS keychain, or this run's memory), the agent process (Pi RPC,
// supervised), and the ask-a-person popups. All decisions about actions stay in gui-gate; this shell only shows the
// person what the gate asks and passes the answer back. All testable logic lives in gpd-core.
//
// Coordinates: the shell never converts agent coordinates. The gate / driver report window-relative logical
// coordinates for each screenshot and map x/y themselves; the only screen maths here is placing the shell's own
// popup windows (physical pixels, scaled by the monitor's scale factor).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gpd_core::config::{all_slots, slot_main, slot_reviewer, slot_role, EffRole, Settings, ROLES};
use gpd_core::human::{self, Outcome, Pending, Popup, Request, Service};
use gpd_core::keys::{preview, store_notice, KeyStore, MemoryStore, SERVICE};
use gpd_core::launch::{self, Paths};
use gpd_core::proc::{self, Agent, AgentEvent, ExitReason, OnEvent};
use gpd_core::{probe, rpc};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
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
    agent: Mutex<Option<Arc<Agent>>>,
    last_task: Mutex<Option<String>>,
    /// open person questions, keyed by popup window label; one answer each
    pending: Arc<Pending>,
    service: Mutex<Option<Arc<Service>>>,
    ipc: Mutex<Option<(String, String)>>,
}

static POPUP_SEQ: AtomicU64 = AtomicU64::new(1);

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

fn headless() -> bool {
    std::env::var("GUI_PI_HEADLESS").is_ok()
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

fn secrets(keys: &dyn KeyStore) -> Vec<String> {
    all_slots().iter().filter_map(|s| keys.get(s)).collect()
}

// ------------------------------------------------------------------ settings commands
#[tauri::command]
fn get_settings(state: tauri::State<AppState>) -> Value {
    let s = state.settings.lock().unwrap().clone();
    let keys = state.keys.lock().unwrap().clone();
    let status: HashMap<String, String> = all_slots().iter().map(|k| (k.clone(), keys.get(k).map(|v| preview(&v)).unwrap_or_else(|| "not set".into()))).collect();
    let errs = s.validate(&|slot| keys.has(slot));
    json!({"settings": s, "keys": status, "store": keys.kind(), "store_notice": store_notice(keys.kind()), "problems": errs, "roles": ROLES})
}

#[tauri::command]
fn save_settings(app: AppHandle, state: tauri::State<AppState>, settings: Settings) -> Result<Value, String> {
    let keys = state.keys.lock().unwrap().clone();
    let dir = config_dir(&app);
    private_dir(&dir);
    settings.save_to(&dir, &secrets(keys.as_ref()))?;
    *state.settings.lock().unwrap() = settings;
    Ok(get_settings(state))
}

#[tauri::command]
fn set_key(state: tauri::State<AppState>, slot: String, value: String) -> Result<Value, String> {
    if !all_slots().contains(&slot) {
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

/// Send one tiny request to a configured model through the Rust side and report ok / latency / a masked error.
/// The key is read from the store here and is never returned to the webview.
#[tauri::command]
async fn test_model(state: tauri::State<'_, AppState>, slot: String) -> Result<Value, String> {
    let s = state.settings.lock().unwrap().clone();
    let keys = state.keys.lock().unwrap().clone();
    let (m, key_slot, gate) = if slot == slot_main() {
        (s.main.clone(), slot_main(), false)
    } else if slot == slot_reviewer() {
        (s.reviewer.clone(), slot_reviewer(), true)
    } else if let Some(r) = ROLES.iter().find(|r| slot == slot_role(r)) {
        match s.effective_role(r, &|k| keys.has(k)) {
            EffRole::Model { model, key_slot } => (model, key_slot, true),
            EffRole::Reviewer => (s.reviewer.clone(), slot_reviewer(), true),
            EffRole::Off => return Ok(json!({"ok": false, "error": "this role is off"})),
        }
    } else {
        return Ok(json!({"ok": false, "error": "unknown slot"}));
    };
    let Some(key) = keys.get(&key_slot) else { return Ok(json!({"ok": false, "error": "no key set"})) };
    let out = tauri::async_runtime::spawn_blocking(move || {
        let pr = match probe::request(&m, &key, gate) {
            Ok(p) => p,
            Err(e) => return json!({"ok": false, "error": e}),
        };
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).build();
        let mut req = agent.post(&pr.url);
        for (k, v) in &pr.headers {
            req = req.set(k, v);
        }
        let t0 = std::time::Instant::now();
        let (status, body, err): (Option<u16>, String, Option<String>) = match req.send_json(pr.body.clone()) {
            Ok(r) => (Some(r.status()), String::new(), None),
            Err(ureq::Error::Status(c, r)) => (Some(c), r.into_string().unwrap_or_default(), None),
            Err(e) => (None, String::new(), Some(e.to_string())),
        };
        let ms = t0.elapsed().as_millis() as u64;
        match probe::verdict(status, &body, err.as_deref(), &key, m.protocol == "systemone") {
            Ok(msg) => json!({"ok": true, "ms": ms, "note": msg}),
            Err(e) => json!({"ok": false, "ms": ms, "error": e}),
        }
    }).await.map_err(|e| e.to_string())?;
    Ok(out)
}

// ------------------------------------------------------------------ popups
/// Show one popup window and block until the person answers, closes it, the app quits, or `seconds` pass.
fn show_popup(app: &AppHandle, req: &Request, seconds: f64) -> Outcome {
    let state = app.state::<AppState>();
    let pending = state.pending.clone();
    let label = format!("popup-{}-{}", POPUP_SEQ.fetch_add(1, Ordering::SeqCst),
                        req.id.chars().filter(|c| c.is_ascii_alphanumeric()).take(16).collect::<String>());
    let Some(rx) = pending.open(&label) else { return Outcome::Unavailable };
    let payload = json!({"kind": req.kind, "message": req.message, "seconds": seconds, "meta": req.meta});
    let url = format!("popup.html#{}", urlencode(&payload.to_string()));
    let takeover = req.kind == "takeover" || req.kind == "ask_help";
    let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title(match req.kind.as_str() { "approve" => "Approve this step?", "answer" => "A question for you", "takeover" => "Your turn", _ => "The agent is stuck" })
        .inner_size(460.0, if takeover { 230.0 } else { 260.0 })
        .always_on_top(true).focused(true).resizable(false)
        .build();
    let win = match built {
        Ok(w) => Some(w),
        Err(_) => {
            pending.answer(&label, Outcome::Unavailable);
            None
        }
    };
    if let Some(win) = &win {
        if takeover {
            // out of the way: the person works in the app behind it (physical pixels, HiDPI-scaled)
            if let Ok(Some(m)) = win.current_monitor() {
                let sf = m.scale_factor();
                let (pos, sz) = (m.position(), m.size());
                let x = pos.x + sz.width as i32 - ((460.0 + 40.0) * sf) as i32;
                let y = pos.y + (40.0 * sf) as i32;
                let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
            }
        }
        let _ = win.request_user_attention(Some(tauri::UserAttentionType::Critical));
        // closing the window is the answer "cancel" (ignored when a button already answered)
        let (p2, l2) = (pending.clone(), label.clone());
        win.on_window_event(move |e| match e {
            tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed => {
                p2.answer(&l2, Outcome::Closed);
            }
            _ => {}
        });
    }
    #[cfg(feature = "test-hooks")]
    if let Ok(rules) = std::env::var("GUI_PI_HUMAN_AUTO") {
        // test builds: the popup is on screen; a script "clicks" it after delay_ms
        let rules: Value = serde_json::from_str(&std::fs::read_to_string(&rules).unwrap_or(rules)).unwrap_or(json!({}));
        let delay = rules.get("delay_ms").and_then(|d| d.as_u64()).unwrap_or(1500);
        let kind = req.kind.clone();
        let (p3, l3) = (pending.clone(), label.clone());
        std::thread::spawn(move || {
            static COUNTER: std::sync::OnceLock<Mutex<HashMap<String, usize>>> = std::sync::OnceLock::new();
            std::thread::sleep(Duration::from_millis(delay));
            let o = human::auto_outcome(&rules, &kind, COUNTER.get_or_init(Default::default));
            if o != Outcome::Timeout {
                p3.answer(&l3, o);
            }
        });
    }
    let out = pending.wait(&label, rx, seconds);
    if let Some(win) = win {
        let _ = win.destroy();
    }
    let (word, refused) = human::label(&req.kind, &out);
    let _ = app.emit("human", json!({"kind": req.kind, "message": req.message, "outcome": word, "refused": refused,
                                     "reason": human::reason(&out)}));
    if headless() {
        println!("{}", json!({"kind": "human", "text": format!("{} -> {word}", req.kind), "refused": refused}));
    }
    out
}

fn urlencode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// The popup's buttons call this. Only the first answer for a popup counts.
#[tauri::command]
fn popup_answer(window: tauri::WebviewWindow, state: tauri::State<AppState>, choice: String, text: Option<String>) -> bool {
    let o = match choice.as_str() {
        "yes" => Outcome::Yes,
        "no" => Outcome::No(text.unwrap_or_default()),
        "text" => Outcome::Text(text.unwrap_or_default()),
        "timeout" => Outcome::Timeout,
        _ => Outcome::Closed,
    };
    state.pending.answer(window.label(), o)
}

fn start_human_service(app: &AppHandle) -> Result<(Arc<Service>, (String, String)), String> {
    let token = human::new_token();
    let a = app.clone();
    let popup: Popup = Arc::new(move |req, secs| show_popup(&a, req, secs));
    let svc = Service::new(token.clone(), popup);
    #[cfg(unix)]
    {
        let base = std::env::var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|_| data_dir(app).join("run"));
        let path = base.join("gui-pi-desktop").join(format!("{}.sock", std::process::id()));
        human::serve_unix(&path, svc.clone()).map_err(|e| e.to_string())?;
        Ok((svc, (path.display().to_string(), token)))
    }
    #[cfg(not(unix))]
    {
        let port = human::serve_tcp(svc.clone()).map_err(|e| e.to_string())?;
        Ok((svc, (format!("tcp:127.0.0.1:{port}"), token)))
    }
}

// ------------------------------------------------------------------ the agent
fn emit_ui(app: &AppHandle, ev: &rpc::UiEvent) {
    if headless() {
        println!("{}", serde_json::to_string(ev).unwrap_or_default());
    }
    let _ = app.emit("agent", ev);
}

fn current_agent(app: &AppHandle) -> Option<Arc<Agent>> {
    app.state::<AppState>().agent.lock().unwrap().clone()
}

fn on_agent_event(app: &AppHandle, e: AgentEvent, log: &Mutex<Option<std::fs::File>>) {
    match e {
        AgentEvent::Line(line) => {
            if let Some(f) = log.lock().unwrap().as_mut() {
                let _ = writeln!(f, "{line}");
            }
            let Some(ev) = rpc::ui_event(&line) else { return };
            if ev.kind == "ui_request" {
                // person questions come through the ask-a-person IPC; a Pi dialog is answered "cancelled" (= refusal)
                if let (Some(id), Some(a)) = (ev.id.as_deref(), current_agent(app)) {
                    let _ = a.send(&rpc::ui_cancel(id));
                }
                return;
            }
            let done = ev.kind == "done";
            emit_ui(app, &ev);
            if done {
                // the run settled: close Pi's stdin (Pi ends the extension, which ends gui-gate and the driver)
                if let Some(a) = current_agent(app) {
                    a.stop(false);
                }
            }
        }
        AgentEvent::Stderr(line) => {
            if let Some(ev) = rpc::stderr_event(&line) {
                emit_ui(app, &ev);
            }
            eprintln!("{line}");
        }
        AgentEvent::Exited { code, reason, stderr_tail } => {
            let state = app.state::<AppState>();
            let n = state.pending.cancel_all();
            if let Some(svc) = state.service.lock().unwrap().as_ref() {
                svc.clear_gate_env();
            }
            *state.agent.lock().unwrap() = None;
            emit_ui(app, &proc::exit_event(code, reason, &stderr_tail, n));
            if headless() {
                app.exit(match reason { ExitReason::Finished => 0, ExitReason::Stopped => 2, _ => 1 });
            }
        }
    }
}

#[tauri::command]
fn start_task(app: AppHandle, state: tauri::State<AppState>, task: String) -> Result<(), String> {
    if state.agent.lock().unwrap().as_ref().map(|a| !a.has_exited()).unwrap_or(false) {
        return Err("a task is already running".into());
    }
    let s = state.settings.lock().unwrap().clone();
    let keys = state.keys.lock().unwrap().clone();
    let ipc = state.ipc.lock().unwrap().clone().ok_or("the ask-a-person service is not running, so no task can start (see the log)")?;
    let svc = state.service.lock().unwrap().clone().ok_or("the ask-a-person service is not running")?;
    let p = paths(&app);
    private_dir(&p.data);
    private_dir(&p.data.join("logs"));
    let l = launch::build(&s, keys.as_ref(), &p, &task, Some((ipc.0.as_str(), ipc.1.as_str()))).map_err(|e| e.join("\n"))?;
    for (f, c) in &l.files {
        if let Some(d) = f.parent() { private_dir(&d.to_path_buf()); }
        std::fs::write(f, c).map_err(|e| e.to_string())?;
    }
    // the gate's settings and keys wait in the service; the extension collects them once, Pi's environment never has them
    svc.stage_gate_env(l.gate_env.clone());
    let env = launch::child_env(std::env::vars(), &l);
    let log = Arc::new(Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(p.data.join("logs").join("agent.jsonl")).ok()));
    let a = app.clone();
    let on: OnEvent = Arc::new(move |e| on_agent_event(&a, e, &log));
    let agent = match proc::spawn(&l.program, &l.args, &env, Some(p.data.as_path()), on) {
        Ok(a) => Arc::new(a),
        Err(e) => {
            svc.clear_gate_env();
            return Err(format!("cannot start the agent: {e}"));
        }
    };
    *state.agent.lock().unwrap() = Some(agent.clone());
    *state.last_task.lock().unwrap() = Some(task.clone());
    agent.send(&rpc::prompt("task-1", &task)).map_err(|e| format!("the agent did not accept the task: {e}"))?;
    Ok(())
}

/// Start the last task again (after a crash).
#[tauri::command]
fn restart_task(app: AppHandle, state: tauri::State<AppState>) -> Result<(), String> {
    let t = state.last_task.lock().unwrap().clone().ok_or("no task to restart")?;
    start_task(app, state, t)
}

/// Stop: cancel any open question, ask Pi to abort, then end the process.
#[tauri::command]
fn stop_task(state: tauri::State<AppState>) {
    state.pending.cancel_all();
    if let Some(a) = state.agent.lock().unwrap().clone() {
        let _ = a.send(&rpc::abort());
        a.stop(true);
    }
}

#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("settings") { let _ = w.set_focus(); return Ok(()); }
    WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("settings.html".into())).title("Settings").inner_size(760.0, 860.0)
        .build().map(|_| ()).map_err(|e| e.to_string())
}

/// App quitting: no new question is shown, every open one is cancelled, the agent is stopped.
fn shutdown(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Some(svc) = state.service.lock().unwrap().as_ref() {
        svc.set_closing();
        svc.clear_gate_env();
    }
    state.pending.cancel_all();
    let agent = state.agent.lock().unwrap().clone();
    if let Some(a) = agent {
        let _ = a.send(&rpc::abort());
        a.stop(true);
    }
}

fn main() {
    // `gui-pi-desktop --run "task"`: run one task with the saved settings, popups still shown, events on stdout
    let argv: Vec<String> = std::env::args().collect();
    let run_task = argv.iter().position(|a| a == "--run").and_then(|i| argv.get(i + 1).cloned());
    if run_task.is_some() { std::env::set_var("GUI_PI_HEADLESS", "1"); }
    let app = tauri::Builder::default()
        .setup(move |app| {
            let h = app.handle().clone();
            let cfg = config_dir(&h).join("config.json");
            let settings = std::fs::read_to_string(&cfg).ok().and_then(|j| Settings::load(&j).ok()).unwrap_or_default();
            let keys: Arc<dyn KeyStore> = if keychain_works() { Arc::new(KeyringStore) } else { Arc::new(MemoryStore::default()) };
            app.manage(AppState { settings: Mutex::new(settings), keys: Mutex::new(keys), agent: Mutex::new(None), last_task: Mutex::new(None),
                                  pending: Arc::new(Pending::default()), service: Mutex::new(None), ipc: Mutex::new(None) });
            match start_human_service(&h) {
                Ok((svc, x)) => {
                    let st = h.state::<AppState>();
                    *st.service.lock().unwrap() = Some(svc);
                    *st.ipc.lock().unwrap() = Some(x);
                }
                Err(e) => eprintln!("ask-a-person service failed: {e} (no task can start)"),
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
        .on_window_event(|window, event| {
            // closing the main window quits the app (open questions are cancelled in shutdown)
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    shutdown(window.app_handle());
                    window.app_handle().exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![get_settings, save_settings, set_key, use_memory_keys, test_model, popup_answer,
                                                 start_task, restart_task, stop_task, open_settings])
        .build(tauri::generate_context!())
        .expect("error while building gui-pi-desktop");
    app.run(|h, event| match event {
        tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit => shutdown(h),
        _ => {}
    });
}
