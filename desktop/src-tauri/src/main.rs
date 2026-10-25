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
use gpd_core::launch::{self, Paths, Redactor};
use gpd_core::proc::{self, Agent, AgentEvent, ExitReason, OnEvent};
use gpd_core::task::{self, TaskFile, UserWords};
use gpd_core::{integrity, probe, rpc, selfcheck};
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
    /// the trusted task channel of the running task (only the person's words are ever written to it)
    task: Mutex<Option<TaskFile>>,
    /// masks every key value in what the shell logs or shows
    redactor: Mutex<Redactor>,
    /// startup checks (Linux self-check, bundle integrity) and the update notice, for the settings page
    checks: Mutex<Value>,
    /// the first error of the running task (a failed model call): the task ends as "error", not "finished" (B2)
    run_error: Mutex<Option<String>>,
}

static TASK_SEQ: AtomicU64 = AtomicU64::new(1);
static MSG_SEQ: AtomicU64 = AtomicU64::new(1);
/// How long Stop waits for the extension's ack (actions blocked, gate_reset, gate killed) before killing anyway.
const STOP_ACK_WAIT: Duration = Duration::from_millis(2000);
const CHECKING: &str = "Still checking this computer and the bundled components; try again in a moment.";

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

/// The app's private runtime dir (sockets, task files): $XDG_RUNTIME_DIR/gui-pi-desktop, else <data>/run.
fn runtime_dir(app: &AppHandle) -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|_| data_dir(app).join("run")).join("gui-pi-desktop")
}

fn resources_dir(app: &AppHandle) -> PathBuf {
    std::env::var("GUI_PI_RESOURCES").map(PathBuf::from)
        .unwrap_or_else(|_| app.path().resource_dir().map(|r| r.join("resources")).unwrap_or_else(|_| PathBuf::from("resources")))
}

fn headless() -> bool {
    std::env::var("GUI_PI_HEADLESS").is_ok()
}

/// Bundled resources (node, pi, extension, prompts, skills, gate, driver). GUI_PI_RESOURCES overrides for dev.
fn paths(app: &AppHandle) -> Paths {
    let res = resources_dir(app);
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
    let checks = state.checks.lock().unwrap().clone();
    json!({"settings": s, "keys": status, "store": keys.kind(), "store_notice": store_notice(keys.kind()), "problems": errs, "roles": ROLES,
           "checks": checks})
}

#[tauri::command]
fn save_settings(app: AppHandle, state: tauri::State<AppState>, settings: Settings) -> Result<Value, String> {
    let keys = state.keys.lock().unwrap().clone();
    let dir = config_dir(&app);
    private_dir(&dir);
    settings.save_to(&dir, &secrets(keys.as_ref()))?;
    *state.settings.lock().unwrap() = settings;
    let _ = app.emit("settings_changed", ()); // the main window re-reads its "before you start" list (B10)
    Ok(get_settings(state))
}

#[tauri::command]
fn set_key(app: AppHandle, state: tauri::State<AppState>, slot: String, value: String) -> Result<Value, String> {
    if !all_slots().contains(&slot) {
        return Err("unknown key slot".into());
    }
    let keys = state.keys.lock().unwrap().clone();
    if value.trim().is_empty() { keys.delete(&slot)? } else { keys.set(&slot, &value)? }
    let _ = app.emit("settings_changed", ());
    Ok(get_settings(state))
}

/// "This run only": keys kept in memory when the machine has no keychain.
#[tauri::command]
fn use_memory_keys(app: AppHandle, state: tauri::State<AppState>) -> Value {
    *state.keys.lock().unwrap() = Arc::new(MemoryStore::default());
    let _ = app.emit("settings_changed", ());
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
    // one shared component for the four result types (approve / clarify / takeover / help)
    let payload = json!({"kind": req.kind, "message": req.message, "seconds": seconds, "view": human::view(req)});
    let url = format!("popup.html#{}", urlencode(&payload.to_string()));
    let ty = human::type_of(&req.kind);
    let takeover = ty == "takeover" || ty == "help";
    let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title(match ty { "approve" => "Approve this step?", "clarify" => "A question for you", "takeover" => "Your turn", _ => "The agent is stuck" })
        // a first guess only: the popup measures its own content and calls popup_fit (round 1: the takeover popup
        // at a fixed 300 px cut off its countdown line)
        .inner_size(480.0, if takeover { 420.0 } else { 380.0 })
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
                let x = pos.x + sz.width as i32 - ((480.0 + 40.0) * sf) as i32;
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

/// The popup asks for the height its content needs (CSS px); clamped to 160 px .. the monitor's height less 80 px.
/// Width stays 480. A takeover / help popup stays in the top right corner.
#[tauri::command]
fn popup_fit(window: tauri::WebviewWindow, height: f64) -> bool {
    if !window.label().starts_with("popup-") || !height.is_finite() {
        return false;
    }
    let max = window.current_monitor().ok().flatten()
        .map(|m| m.size().height as f64 / m.scale_factor() - 80.0).unwrap_or(720.0).max(200.0);
    window.set_size(tauri::LogicalSize::new(480.0, height.clamp(160.0, max))).is_ok()
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
    // per-question and per-task person metrics (gui-pi-human/1) for bench/human_pilot.py; nothing more is shown
    let logs = data_dir(app).join("logs");
    private_dir(&logs);
    let hl = Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(logs.join("human.jsonl")).ok());
    svc.set_log(Box::new(move |v| {
        if let Some(f) = hl.lock().unwrap().as_mut() {
            let _ = writeln!(f, "{v}");
        }
    }));
    // a question that expired in the queue never had a popup: it still shows in the timeline (B8)
    let a2 = app.clone();
    svc.set_notify(Box::new(move |v| {
        let _ = a2.emit("human", v.clone());
        if headless() {
            println!("{}", json!({"kind": "human", "text": format!("{} -> {}", v["kind"].as_str().unwrap_or("?"), v["outcome"].as_str().unwrap_or("?")),
                                  "refused": true}));
        }
    }));
    #[cfg(unix)]
    {
        let path = runtime_dir(app).join(format!("{}.sock", std::process::id()));
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
    let red = app.state::<AppState>().redactor.lock().unwrap().clone();
    let mut ev = ev.clone();
    ev.text = red.line(&ev.text);
    if headless() {
        println!("{}", serde_json::to_string(&ev).unwrap_or_default());
    }
    let _ = app.emit("agent", &ev);
}

fn current_agent(app: &AppHandle) -> Option<Arc<Agent>> {
    app.state::<AppState>().agent.lock().unwrap().clone()
}

fn on_agent_event(app: &AppHandle, e: AgentEvent, log: &Mutex<Option<std::fs::File>>) {
    match e {
        AgentEvent::Line(line) => {
            if let Some(f) = log.lock().unwrap().as_mut() {
                let red = app.state::<AppState>().redactor.lock().unwrap().clone();
                let _ = writeln!(f, "{}", red.line(&line));
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
            if ev.kind == "error" {
                let red = app.state::<AppState>().redactor.lock().unwrap().clone();
                app.state::<AppState>().run_error.lock().unwrap().get_or_insert(red.line(&ev.text));
            }
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
            let red = app.state::<AppState>().redactor.lock().unwrap().clone();
            eprintln!("{}", red.line(&line));
        }
        AgentEvent::Exited { code, reason, stderr_tail } => {
            let state = app.state::<AppState>();
            let n = state.pending.cancel_all();
            if let Some(svc) = state.service.lock().unwrap().as_ref() {
                svc.clear_gate_env();
                svc.end_task(match reason { ExitReason::Finished => "finished", ExitReason::Stopped => "stopped",
                                            ExitReason::Crashed => "crashed", ExitReason::Disconnected => "disconnected" });
            }
            if let Some(t) = state.task.lock().unwrap().take() {
                t.remove();
            }
            *state.agent.lock().unwrap() = None;
            let err = state.run_error.lock().unwrap().take();
            emit_ui(app, &proc::exit_event_after(code, reason, &stderr_tail, n, err.as_deref()));
            if headless() {
                app.exit(match reason { ExitReason::Finished if err.is_some() => 4, ExitReason::Finished => 0, ExitReason::Stopped => 2, _ => 1 });
            }
        }
    }
}

#[tauri::command]
fn start_task(app: AppHandle, state: tauri::State<AppState>, task: String) -> Result<(), String> {
    if state.agent.lock().unwrap().as_ref().map(|a| !a.has_exited()).unwrap_or(false) {
        return Err("a task is already running".into());
    }
    // the person's own words: the only thing that ever goes into the task file
    let words = UserWords::from_ui(&task).ok_or("type what the agent should do")?;
    let blocked = state.checks.lock().unwrap().get("blocked").and_then(|b| b.as_str()).map(String::from);
    if let Some(why) = blocked {
        return Err(why);
    }
    let s = state.settings.lock().unwrap().clone();
    let keys = state.keys.lock().unwrap().clone();
    let ipc = state.ipc.lock().unwrap().clone().ok_or("the ask-a-person service is not running, so no task can start (see the log)")?;
    let svc = state.service.lock().unwrap().clone().ok_or("the ask-a-person service is not running")?;
    let p = paths(&app);
    private_dir(&p.data);
    private_dir(&p.data.join("logs"));
    let task_id = format!("{}-{}", std::process::id(), TASK_SEQ.fetch_add(1, Ordering::SeqCst));
    let tf = TaskFile::create(&runtime_dir(&app), &task_id, &words).map_err(|e| format!("cannot write the task file: {e}"))?;
    let l = match launch::build(&s, keys.as_ref(), &p, words.as_str(), Some(&tf.path), Some((ipc.0.as_str(), ipc.1.as_str()))) {
        Ok(l) => l,
        Err(e) => {
            tf.remove();
            return Err(e.join("\n"));
        }
    };
    *state.redactor.lock().unwrap() = Redactor::for_launch(keys.as_ref(), &l);
    svc.begin_task(&task_id);
    for (f, c) in &l.files {
        if let Some(d) = f.parent() { private_dir(&d.to_path_buf()); }
        std::fs::write(f, c).map_err(|e| e.to_string())?;
    }
    // the gate's settings and keys wait in the service; the extension collects them once, Pi's environment never has them
    svc.stage_gate_env(l.gate_env.clone());
    // the session's environment without an AppImage's bundled library / module paths (B4)
    let env = launch::child_env(launch::system_env(std::env::vars()), &l);
    let log = Arc::new(Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(p.data.join("logs").join("agent.jsonl")).ok()));
    let a = app.clone();
    let on: OnEvent = Arc::new(move |e| on_agent_event(&a, e, &log));
    let agent = match proc::spawn(&l.program, &l.args, &env, Some(p.data.as_path()), on) {
        Ok(a) => Arc::new(a),
        Err(e) => {
            svc.clear_gate_env();
            svc.end_task("not_started");
            tf.remove();
            return Err(format!("cannot start the agent: {e}"));
        }
    };
    *state.agent.lock().unwrap() = Some(agent.clone());
    *state.task.lock().unwrap() = Some(tf);
    *state.last_task.lock().unwrap() = Some(words.as_str().to_string());
    *state.run_error.lock().unwrap() = None;
    agent.send(&rpc::prompt("task-1", words.as_str())).map_err(|e| format!("the agent did not accept the task: {e}"))?;
    // the main window learns of a task it did not start itself (--run, Restart) and shows it as running (B10)
    let _ = app.emit("agent", rpc::UiEvent::new("started", words.as_str()));
    Ok(())
}

/// A later message from the person while the task runs: into the task file first (the gate re-reads it), then to
/// Pi as a steering prompt. Only this command and start_task write the task file.
#[tauri::command]
fn send_message(state: tauri::State<AppState>, text: String) -> Result<(), String> {
    let words = UserWords::from_ui(&text).ok_or("empty message")?;
    let agent = state.agent.lock().unwrap().clone().filter(|a| !a.has_exited()).ok_or("no task is running")?;
    {
        let mut t = state.task.lock().unwrap();
        let tf = t.as_mut().ok_or("no task is running")?;
        tf.add(&words).map_err(|e| format!("cannot update the task file: {e}"))?;
    }
    let id = format!("msg-{}", MSG_SEQ.fetch_add(1, Ordering::SeqCst));
    agent.send(&task::steer_line(&id, &words)).map_err(|e| format!("the agent did not take the message: {e}"))
}

/// Start the last task again (after a crash).
#[tauri::command]
fn restart_task(app: AppHandle, state: tauri::State<AppState>) -> Result<(), String> {
    let t = state.last_task.lock().unwrap().clone().ok_or("no task to restart")?;
    start_task(app, state, t)
}

/// Stop: cancel any open question; the extension blocks every action, sends gate_reset (when the gate has it) and
/// kills the gate; then Pi and everything it started are killed. Nothing is sent after this; a new task starts
/// fresh processes.
#[tauri::command]
fn stop_task(state: tauri::State<AppState>) {
    stop_now(&state);
}

fn stop_now(state: &AppState) {
    state.pending.cancel_all();
    let agent = state.agent.lock().unwrap().clone();
    if let Some(a) = agent {
        let svc = state.service.lock().unwrap().clone();
        if let Some(svc) = svc {
            let _ = svc.signal_stop(STOP_ACK_WAIT);
        }
        a.kill_tree();
    }
}

/// Startup: Linux self-check, bundle integrity against the shipped SHA256SUMS (a mismatch blocks Start), and - when
/// allowed in the settings - an "update available" notice from the latest public release. Nothing is installed.
fn startup_checks(app: &AppHandle, keychain: bool, update_check: bool) {
    let checks = selfcheck::evaluate(&selfcheck::gather(keychain));
    let integ = integrity::verify(&resources_dir(app));
    let mut blocked = None;
    if integ.blocks_start() {
        blocked = Some(integ.message());
    } else if !selfcheck::can_start(&checks) {
        blocked = Some(format!("This desktop session cannot run the agent: {}",
            checks.iter().filter(|c| c.level == "fail").map(|c| c.message.clone()).collect::<Vec<_>>().join(" ")));
    }
    let v = json!({"system": checks, "integrity": {"ok": !integ.blocks_start(), "unverified": integ.unverified, "message": integ.message()},
                   "blocked": blocked, "update": Value::Null, "version": env!("CARGO_PKG_VERSION")});
    *app.state::<AppState>().checks.lock().unwrap() = v;
    let _ = app.emit("checks_done", ()); // the main window drops "Still checking..." (B10)
    if headless() {
        for c in &checks {
            eprintln!("[self-check] {}: {} - {}", c.id, c.level, c.message);
        }
        eprintln!("[self-check] bundle: {}", integ.message());
    }
    if update_check {
        let a = app.clone();
        std::thread::spawn(move || {
            let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).build();
            let Ok(r) = agent.get(integrity::LATEST_RELEASE_API).set("User-Agent", "gui-pi-desktop").set("Accept", "application/vnd.github+json").call() else { return };
            let Ok(body) = r.into_string() else { return };
            if let Some(n) = integrity::update_notice(env!("CARGO_PKG_VERSION"), &body) {
                a.state::<AppState>().checks.lock().unwrap()["update"] = json!(n);
                emit_ui(&a, &rpc::UiEvent::new("notice", n));
            }
        });
    }
}

#[tauri::command]
fn self_check(app: AppHandle, state: tauri::State<AppState>) -> Value {
    let keychain = state.keys.lock().unwrap().kind() == "keychain";
    let update = state.checks.lock().unwrap().get("update").cloned().unwrap_or(Value::Null);
    startup_checks(&app, keychain, false);
    state.checks.lock().unwrap()["update"] = update;
    state.checks.lock().unwrap().clone()
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
    stop_now(&state);
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
            let keychain = keys.kind() == "keychain";
            let update_check = settings.update_check;
            app.manage(AppState { settings: Mutex::new(settings), keys: Mutex::new(keys), agent: Mutex::new(None), last_task: Mutex::new(None),
                                  pending: Arc::new(Pending::default()), service: Mutex::new(None), ipc: Mutex::new(None),
                                  task: Mutex::new(None), redactor: Mutex::new(Redactor::default()), run_error: Mutex::new(None),
                                  checks: Mutex::new(json!({"pending": true, "blocked": CHECKING})) });
            // hashing the bundle takes a moment: off the UI thread; Start waits for it
            let h3 = h.clone();
            std::thread::spawn(move || startup_checks(&h3, keychain, update_check));
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
                    for _ in 0..600 {
                        if h2.state::<AppState>().checks.lock().unwrap().get("pending").is_none() { break; }
                        std::thread::sleep(Duration::from_millis(100));
                    }
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
        .invoke_handler(tauri::generate_handler![get_settings, save_settings, set_key, use_memory_keys, test_model, popup_answer, popup_fit,
                                                 start_task, send_message, restart_task, stop_task, open_settings, self_check])
        .build(tauri::generate_context!())
        .expect("error while building gui-pi-desktop");
    app.run(|h, event| match event {
        tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit => shutdown(h),
        _ => {}
    });
}
