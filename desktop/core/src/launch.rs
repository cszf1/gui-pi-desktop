//! How the shell starts the agent: `node pi --mode rpc` with the gui-pi extension, which starts gui-gate, which
//! starts the bundled cua-driver. Keys go ONLY into the environment of the process that needs them (never into a
//! file, an argument or a log line); the generated models.json names the environment variable, not the key.
//!
//! Who gets what:
//!   Pi (node)  the allowlisted session variables + PI_CODING_AGENT_DIR + GUI_PI_* (its own model key GUI_PI_MAIN_KEY,
//!              the person IPC path and token). No GUI_GATE_* variable at all.
//!   gui-gate   GUI_GATE_* (reviewer, roles and their keys), handed over ONCE through the person IPC
//!              (`gate_env`, token-checked, one-shot) and passed by the extension to the gate child only. Never
//!              Pi's model key, never the person token, never GUI_GATE_ALLOW_CONFIG / --allow-config.
//! Without a person IPC (dev runs only) the gate variables fall back to Pi's environment (`gate_env_in_pi`).
use crate::config::{slot_main, slot_reviewer, EffRole, ModelCfg, Settings, ROLES};
use crate::keys::KeyStore;
use serde_json::json;
use std::path::{Path, PathBuf};

pub const MAIN_KEY_ENV: &str = "GUI_PI_MAIN_KEY";
pub const PROVIDER: &str = "gui-main";
/// the tools Pi may call: the gate's GUI tools + the extension's own (note_fact, pin_evidence, recall_image,
/// load_skill). Pi's own bash/read/edit/write are not in the list; gate_task / gate_confirm / gate_config and the
/// client-only gate_next_step / gate_reset are never offered.
pub const PI_TOOLS: &[&str] = &["list_apps", "list_windows", "get_window_state", "launch_app", "bring_to_front", "click",
    "double_click", "right_click", "type_text", "press_key", "hotkey", "scroll", "set_value", "drag", "invoke_menu",
    "run_actions", "run_command", "request_human", "set_plan", "check_done", "gate_report", "inspect",
    "note_fact", "pin_evidence", "recall_image", "load_skill"];
/// Never set by the desktop launcher, never inherited from the shell's own environment.
pub const FORBIDDEN_ENV: &[&str] = &["GUI_GATE_ALLOW_CONFIG", "GUI_PI_GATE_ARGS"];
pub const FORBIDDEN_ARGS: &[&str] = &["--allow-config"];

/// Session variables a child may inherit from the shell (mirrors gui-gate's proc.rs allowlist).
pub const BASE_ENV_NAMES: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "SHELL", "PWD", "TERM", "COLORTERM", "LANG", "LANGUAGE", "TZ", "TMPDIR",
    "DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY", "DBUS_SESSION_BUS_ADDRESS", "AT_SPI_BUS_ADDRESS", "NO_AT_BRIDGE",
    "DESKTOP_SESSION", "SESSION_MANAGER",
    "http_proxy", "https_proxy", "no_proxy", "all_proxy", "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "ALL_PROXY",
    "SYSTEMROOT", "SystemRoot", "SYSTEMDRIVE", "SystemDrive", "WINDIR", "windir", "COMSPEC", "ComSpec", "PATHEXT",
    "TEMP", "TMP", "USERPROFILE", "USERNAME", "USERDOMAIN", "APPDATA", "LOCALAPPDATA", "PROGRAMDATA", "ProgramData",
    "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "CommonProgramFiles", "PUBLIC", "HOMEDRIVE", "HOMEPATH",
    "NUMBER_OF_PROCESSORS", "PROCESSOR_ARCHITECTURE", "OS", "__CF_USER_TEXT_ENCODING",
];
pub const BASE_ENV_PREFIXES: &[&str] = &["LC_", "XDG_", "GTK_", "GDK_", "QT_", "CUA_"];

#[derive(Clone, Debug)]
pub struct Paths {
    pub node: PathBuf,
    pub pi_cli: PathBuf,
    pub extension: PathBuf,
    pub skills: PathBuf,
    pub system_prompt: PathBuf,
    pub gate: PathBuf,
    pub driver: Option<PathBuf>,
    /// private app data dir (0700): Pi agent dir, sessions, logs
    pub data: PathBuf,
}

#[derive(Debug)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Pi's own variables, set on top of the allowlisted parent environment (see `child_env`)
    pub env: Vec<(String, String)>,
    /// the gate's variables, handed to the extension once over the person IPC (empty when they had to go into `env`)
    pub gate_env: Vec<(String, String)>,
    /// true when there was no person IPC and the gate's variables are in Pi's environment (dev runs only)
    pub gate_env_in_pi: bool,
    /// files to write before start (path, content): no secrets in them
    pub files: Vec<(PathBuf, String)>,
}

impl Launch {
    /// For logs and the UI: the environment with every secret value masked.
    pub fn redacted_env(&self) -> Vec<(String, String)> {
        self.env.iter().chain(self.gate_env.iter()).map(|(k, v)| (k.clone(), if is_secret(k) { "***".into() } else { v.clone() })).collect()
    }
}

pub fn is_secret(k: &str) -> bool {
    let u = k.to_ascii_uppercase(); // Windows names come in any case
    u.ends_with("_KEY") || u.contains("TOKEN")
}

/// Masks every key value the shell holds in what it logs or shows (the agent's RPC lines, stderr, UI events).
#[derive(Clone, Default)]
pub struct Redactor {
    values: Vec<String>,
}

impl Redactor {
    pub fn new(values: impl IntoIterator<Item = String>) -> Redactor {
        let mut v: Vec<String> = values.into_iter().map(|s| s.trim().to_string()).filter(|s| s.len() >= 8).collect();
        v.sort_by_key(|s| std::cmp::Reverse(s.len()));
        v.dedup();
        Redactor { values: v }
    }
    /// From a key store and a launch: every slot's key + every secret-looking variable the launch sets.
    pub fn for_launch(keys: &dyn KeyStore, l: &Launch) -> Redactor {
        let mut v: Vec<String> = crate::config::all_slots().iter().filter_map(|s| keys.get(s)).collect();
        v.extend(l.env.iter().chain(l.gate_env.iter()).filter(|(k, _)| is_secret(k)).map(|(_, v)| v.clone()));
        Redactor::new(v)
    }
    pub fn line(&self, s: &str) -> String {
        let mut out = s.to_string();
        for v in &self.values {
            if out.contains(v.as_str()) {
                out = out.replace(v.as_str(), "***");
            }
        }
        out
    }
}

/// The complete environment of the Pi process: the allowlisted variables of `parent` (never a GUI_GATE_*, GUI_PI_*,
/// PI_* or other secret-looking variable of the shell's own environment), then the launch's own variables.
pub fn child_env(parent: impl IntoIterator<Item = (String, String)>, l: &Launch) -> Vec<(String, String)> {
    child_env_for(parent, l, cfg!(windows))
}

/// Windows spells variables as it likes ("Path", "SystemRoot", "windir") and treats names without case: there the
/// allowlist is compared without case, or node, the gate and the driver would start without PATH.
pub fn allowed_name(k: &str, windows: bool) -> bool {
    let named = if windows { BASE_ENV_NAMES.iter().any(|n| n.eq_ignore_ascii_case(k)) } else { BASE_ENV_NAMES.contains(&k) };
    named || BASE_ENV_PREFIXES.iter().any(|p| k.starts_with(p))
}

pub fn child_env_for(parent: impl IntoIterator<Item = (String, String)>, l: &Launch, windows: bool) -> Vec<(String, String)> {
    let same = |a: &str, b: &str| if windows { a.eq_ignore_ascii_case(b) } else { a == b };
    let mut out: Vec<(String, String)> = parent.into_iter()
        .filter(|(k, _)| allowed_name(k, windows))
        .filter(|(k, _)| !is_secret(k) && !FORBIDDEN_ENV.contains(&k.as_str()))
        .collect();
    for (k, v) in &l.env {
        if FORBIDDEN_ENV.contains(&k.as_str()) {
            continue;
        }
        out.retain(|(x, _)| !same(x, k));
        out.push((k.clone(), v.clone()));
    }
    out
}

/// models.json `apiKey`: Pi 1.4.0 resolves "$NAME" / "${NAME}" from its environment (docs/models-configuration.md,
/// core/resolve-config-value.js); a bare name is a literal key (0.3.0 sent "GUI_PI_MAIN_KEY" itself -> 401).
pub fn pi_api_key_ref() -> String {
    format!("${MAIN_KEY_ENV}")
}

/// Variables an AppImage's runtime and hooks add for the app itself (its bundled libraries, GTK/GIO modules, Python).
const APPIMAGE_ONLY_ENV: &[&str] = &["LD_LIBRARY_PATH", "LD_PRELOAD", "PYTHONHOME", "PYTHONPATH", "PERLLIB", "GSETTINGS_SCHEMA_DIR",
    "GTK_PATH", "GTK_EXE_PREFIX", "GTK_DATA_PREFIX", "GTK_IM_MODULE_FILE", "GDK_PIXBUF_MODULE_FILE", "GDK_PIXBUF_MODULEDIR",
    "GIO_MODULE_DIR", "GIO_EXTRA_MODULES", "QT_PLUGIN_PATH", "APPDIR", "APPIMAGE", "ARGV0", "OWD"];

/// The session environment as the desktop gave it, for every system tool the shell runs (self-check) and every child
/// (Pi -> extension -> gui-gate -> driver, python dialogs). Inside an AppImage ($APPDIR set) the runtime points library,
/// module and data search paths into the mounted image: a system `dbus-send` then loads the image's older libdbus
/// and fails (0.3.0: DBus / AT-SPI wrongly reported broken). Those variables are dropped and $APPDIR entries are
/// removed from path lists. Outside an AppImage nothing changes.
pub fn system_env(vars: impl IntoIterator<Item = (String, String)>) -> Vec<(String, String)> {
    let vars: Vec<(String, String)> = vars.into_iter().collect();
    let appdir = vars.iter().find(|(k, _)| k == "APPDIR").map(|(_, v)| v.trim_end_matches('/').to_string()).filter(|v| v.len() > 1);
    let Some(appdir) = appdir else { return vars };
    let inside = |p: &str| p == appdir || p.starts_with(&format!("{appdir}/"));
    vars.into_iter().filter_map(|(k, v)| {
        if APPIMAGE_ONLY_ENV.contains(&k.as_str()) {
            return None;
        }
        if !v.contains(&appdir) {
            return Some((k, v));
        }
        let kept: Vec<&str> = v.split(':').filter(|p| !p.is_empty() && !inside(p)).collect();
        if kept.is_empty() { None } else { Some((k, kept.join(":"))) }
    }).collect()
}

fn pi_models_json(m: &ModelCfg) -> String {
    let vision = m.vision.unwrap_or(true);
    json!({"providers": {PROVIDER: {
        "baseUrl": m.base_url.trim(), "api": m.protocol, "apiKey": pi_api_key_ref(),
        "models": [{"id": m.model.trim(), "reasoning": true, "input": if vision { json!(["text", "image"]) } else { json!(["text"]) },
                    "contextWindow": 128000, "maxTokens": 16384}]}}}).to_string()
}

fn gate_model_env(prefix: &str, m: &ModelCfg, key: Option<String>, env: &mut Vec<(String, String)>) {
    env.push((format!("{prefix}_URL"), m.base_url.trim().into()));
    env.push((format!("{prefix}_MODEL"), m.model.trim().into()));
    env.push((format!("{prefix}_PROTOCOL"), m.protocol.clone()));
    if let Some(t) = m.timeout_s {
        env.push((format!("{prefix}_TIMEOUT"), t.to_string()));
    }
    if let Some(k) = key {
        env.push((format!("{prefix}_KEY"), k));
    }
}

/// The gate's variables: reviewer (mandatory), roles with inheritance, feature switches.
pub fn gate_env(s: &Settings, keys: &dyn KeyStore, p: &Paths) -> Vec<(String, String)> {
    let mut env = vec![];
    // the user's own ~/.config/gui-gate/reviewer.env is never read
    gate_model_env("GUI_GATE_REVIEWER", &s.reviewer, keys.get(&slot_reviewer()), &mut env);
    env.push(("GUI_GATE_REVIEWER_ENV".into(), p.data.join("no-reviewer.env").display().to_string()));
    for r in ROLES {
        let up = r.to_uppercase();
        match s.effective_role(r, &|slot| keys.has(slot)) {
            EffRole::Off => {}
            EffRole::Reviewer => env.push((format!("GUI_GATE_ROLE_{up}"), "reviewer".into())),
            EffRole::Model { model, key_slot } => gate_model_env(&format!("GUI_GATE_ROLE_{up}"), &model, keys.get(&key_slot), &mut env),
        }
    }
    if s.gate_097 {
        env.push(("GUI_GATE_097".into(), "1".into()));
    }
    env.push(("GUI_GATE_NO_UPDATE_CHECK".into(), "1".into()));
    env.retain(|(k, _)| !FORBIDDEN_ENV.contains(&k.as_str()));
    env
}

/// Build the launch. `task` is the user's request verbatim; `task_file` the trusted task channel the shell writes
/// (task::TaskFile) and the gate reads with --task-file (the gate treats it as the operator's own words).
pub fn build(s: &Settings, keys: &dyn KeyStore, p: &Paths, task: &str, task_file: Option<&Path>, human_ipc: Option<(&str, &str)>) -> Result<Launch, Vec<String>> {
    let errs = s.validate(&|slot| keys.has(slot));
    if !errs.is_empty() {
        return Err(errs);
    }
    let agent_dir = p.data.join("pi-agent");
    let mut env: Vec<(String, String)> = vec![];
    env.push(("PI_CODING_AGENT_DIR".into(), agent_dir.display().to_string()));
    env.push((MAIN_KEY_ENV.into(), keys.get(&slot_main()).unwrap_or_default()));
    // the extension
    env.push(("GUI_PI_GATE".into(), p.gate.display().to_string()));
    if let Some(d) = &p.driver {
        env.push(("GUI_PI_DRIVER".into(), d.display().to_string()));
    }
    env.push(("GUI_PI_TASK".into(), task.into()));
    if let Some(tf) = task_file {
        env.push(("GUI_PI_TASK_FILE".into(), tf.display().to_string()));
    }
    env.push(("GUI_PI_SKILLS".into(), p.skills.display().to_string()));
    env.push(("GUI_PI_HUMAN_TIMEOUT".into(), s.human_timeout_s.to_string()));
    env.push(("GUI_PI_GATE_LOG".into(), p.data.join("logs").join("gate.jsonl").display().to_string()));
    let mut genv = gate_env(s, keys, p);
    let gate_env_in_pi = human_ipc.is_none();
    if let Some((path, token)) = human_ipc {
        env.push(("GUI_PI_HUMAN_IPC".into(), path.into()));
        env.push(("GUI_PI_HUMAN_TOKEN".into(), token.into()));
    } else {
        env.append(&mut genv);
    }
    let prompt = std::fs::read_to_string(&p.system_prompt).map_err(|e| vec![format!("system prompt: {e}")])?;
    let mut args = vec![p.pi_cli.display().to_string(), "--mode".into(), "rpc".into(), "--no-skills".into()];
    for sk in skill_dirs(&p.skills) {
        args.push("--skill".into());
        args.push(sk.display().to_string());
    }
    args.extend(["--extension".into(), p.extension.display().to_string(), "--tools".into(), PI_TOOLS.join(","),
                 "--system-prompt".into(), prompt,
                 "--model".into(), format!("{PROVIDER}/{}", s.main.model.trim()),
                 "--thinking".into(), s.main.thinking.clone().unwrap_or_else(|| "medium".into())]);
    args.retain(|a| !FORBIDDEN_ARGS.iter().any(|f| a.starts_with(f)));
    Ok(Launch {
        program: p.node.clone(),
        args,
        env,
        gate_env: genv,
        gate_env_in_pi,
        files: vec![(agent_dir.join("models.json"), pi_models_json(&s.main))],
    })
}

fn skill_dirs(root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(root).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.join("SKILL.md").is_file()).collect()).unwrap_or_default();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_variable_names_ignore_case() {
        let l = Launch { program: "node".into(), args: vec![], env: vec![("GUI_PI_X".into(), "1".into())], gate_env: vec![], gate_env_in_pi: false, files: vec![] };
        let parent = vec![("Path".to_string(), "C:\\Windows\\system32".to_string()), ("SystemRoot".to_string(), "C:\\Windows".to_string()),
                          ("windir".to_string(), "C:\\Windows".to_string()), ("OpenAI_Api_Key".to_string(), "sk-x".to_string()),
                          ("Gui_Gate_Allow_Config".to_string(), "1".to_string())];
        let w = super::child_env_for(parent.clone(), &l, true);
        let names: Vec<&str> = w.iter().map(|(k, _)| k.as_str()).collect();
        assert!(names.contains(&"Path") && names.contains(&"SystemRoot") && names.contains(&"windir"), "{names:?}");
        assert!(!names.iter().any(|n| n.to_ascii_uppercase().contains("KEY") || n.to_ascii_uppercase().contains("ALLOW")), "{names:?}");
        let u = super::child_env_for(parent, &l, false);
        assert!(!u.iter().any(|(k, _)| k == "Path"), "unix names stay exact");
    }

    use super::*;
    use crate::keys::MemoryStore;
    use crate::config::{ModelCfg, RoleCfg};

    fn setup() -> (Settings, MemoryStore, Paths, PathBuf) {
        let dir = std::env::temp_dir().join(format!("gpd-launch-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("skills/gui-basics")).unwrap();
        std::fs::write(dir.join("skills/gui-basics/SKILL.md"), "---\nname: gui-basics\n---\n").unwrap();
        std::fs::write(dir.join("system.md"), "You operate apps.").unwrap();
        let mut s = Settings::default();
        s.main.base_url = "https://api.example.com/v1".into();
        s.main.model = "deepseek-v4.1-flash".into();
        s.reviewer.base_url = "https://rev.example.com/v1/chat/completions".into();
        s.reviewer.model = "decision-model-preview".into();
        s.reviewer.protocol = "systemone".into();
        s.roles.insert("grounder".into(), RoleCfg::Reviewer);
        let k = MemoryStore::default();
        k.set("main", "sk-main-secret-0001").unwrap();
        k.set("reviewer", "sk-rev-secret-0002").unwrap();
        let p = Paths { node: "node".into(), pi_cli: dir.join("pi/cli.js"), extension: dir.join("gui-pi/index.ts"), skills: dir.join("skills"),
                        system_prompt: dir.join("system.md"), gate: dir.join("gui-gate"), driver: Some(dir.join("cua-driver")), data: dir.join("data") };
        (s, k, p, dir)
    }

    #[test]
    fn keys_only_in_env_never_in_files_or_args() {
        let (s, k, p, _) = setup();
        let l = build(&s, &k, &p, "rename the file", None, Some(("/run/x.sock", "tok"))).unwrap();
        let pi = |n: &str| l.env.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
        let gate = |n: &str| l.gate_env.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
        assert_eq!(pi("GUI_PI_MAIN_KEY").as_deref(), Some("sk-main-secret-0001"));
        assert_eq!(gate("GUI_GATE_REVIEWER_KEY").as_deref(), Some("sk-rev-secret-0002"));
        assert_eq!(gate("GUI_GATE_REVIEWER_PROTOCOL").as_deref(), Some("systemone"));
        assert_eq!(gate("GUI_GATE_ROLE_GROUNDER").as_deref(), Some("reviewer"));
        assert!(gate("GUI_GATE_ROLE_PLANNER").is_none() && gate("GUI_GATE_ROLE_PLANNER_URL").is_none());
        assert_eq!(pi("GUI_PI_TASK").as_deref(), Some("rename the file"));
        for (_, content) in &l.files {
            assert!(!content.contains("secret"), "{content}");
        }
        // B1: the key is an environment reference Pi resolves ("$NAME"), never the bare name (a literal key -> 401)
        let models = l.files.iter().find(|(f, _)| f.ends_with("models.json")).map(|(_, c)| c.clone()).expect("models.json");
        let mj: serde_json::Value = serde_json::from_str(&models).unwrap();
        let key = mj["providers"].as_object().unwrap().values().next().unwrap()["apiKey"].as_str().unwrap().to_string();
        assert_eq!(key, "$GUI_PI_MAIN_KEY");
        assert_eq!(key, pi_api_key_ref());
        if let Ok(out) = std::env::var("GPD_DUMP_MODELS_JSON") {
            // CI: scripts/check-pi-key.mjs resolves this file with the bundled Pi's own resolver
            std::fs::write(out, &models).unwrap();
        }
        assert!(!l.args.iter().any(|a| a.contains("secret")));
        assert!(l.redacted_env().iter().all(|(_, v)| !v.contains("secret")));
        // only the gate's tools (+ note_fact); Pi's own bash and the gate's authorisation tools are not among them
        let tools = &l.args[l.args.iter().position(|a| a == "--tools").unwrap() + 1];
        assert!(tools.contains("click") && tools.contains("note_fact"));
        assert!(!tools.split(',').any(|t| ["bash", "gate_task", "gate_confirm", "gate_config"].contains(&t)));
        assert!(l.args.contains(&"--skill".to_string()));
        assert_eq!(l.args.last().unwrap(), "medium");
    }

    #[test]
    fn pi_never_sees_the_gate_keys_and_the_gate_never_sees_pi_s() {
        let (mut s, k, p, _) = setup();
        s.roles.insert("planner".into(), RoleCfg::Custom { model: ModelCfg { base_url: "https://p.example.com".into(), ..Default::default() } });
        k.set("role.planner", "sk-planner-secret-0003").unwrap();
        let l = build(&s, &k, &p, "x", None, Some(("/run/x.sock", "tok"))).unwrap();
        assert!(!l.gate_env_in_pi);
        assert!(l.env.iter().all(|(k, _)| !k.starts_with("GUI_GATE_")), "{:?}", l.redacted_env());
        let pi_env = child_env(vec![("PATH".into(), "/usr/bin".into()), ("GUI_GATE_REVIEWER_KEY".into(), "sk-parent".into()),
                                    ("OPENAI_API_KEY".into(), "sk-parent2".into()), ("DISPLAY".into(), ":0".into())], &l);
        let joined = format!("{pi_env:?}");
        for bad in ["sk-rev-secret-0002", "sk-planner-secret-0003", "sk-parent", "GUI_GATE_"] {
            assert!(!joined.contains(bad), "{bad} in Pi's environment");
        }
        assert!(joined.contains("DISPLAY") && joined.contains("sk-main-secret-0001"));
        let g = format!("{:?}", l.gate_env);
        assert!(g.contains("sk-planner-secret-0003") && g.contains("GUI_GATE_ROLE_PLANNER_URL"));
        assert!(!g.contains("sk-main-secret-0001") && !g.contains("GUI_PI_") && !g.contains("tok"));
        // a role that inherits the main model gets the main key in the GATE's variables (it needs it), still not Pi's others
        s.roles.insert("judge".into(), RoleCfg::Main);
        let l = build(&s, &k, &p, "x", None, Some(("/run/x.sock", "tok"))).unwrap();
        assert!(l.gate_env.iter().any(|(k, v)| k == "GUI_GATE_ROLE_JUDGE_KEY" && v == "sk-main-secret-0001"));
    }

    #[test]
    fn the_launcher_never_allows_gate_config() {
        let (s, k, p, _) = setup();
        for ipc in [Some(("/run/x.sock", "tok")), None] {
            let l = build(&s, &k, &p, "x --allow-config", None, ipc).unwrap();
            let all: Vec<&(String, String)> = l.env.iter().chain(l.gate_env.iter()).collect();
            assert!(all.iter().all(|(k, _)| k != "GUI_GATE_ALLOW_CONFIG" && k != "GUI_PI_GATE_ARGS"));
            assert!(all.iter().all(|(_, v)| !v.contains("--allow-config") || v == "x --allow-config"));
            assert!(!l.args.iter().any(|a| a.starts_with("--allow-config")));
            let parent = vec![("GUI_GATE_ALLOW_CONFIG".to_string(), "1".to_string()), ("GUI_PI_GATE_ARGS".to_string(), "[\"--allow-config\"]".to_string())];
            assert!(child_env(parent, &l).iter().all(|(k, _)| k != "GUI_GATE_ALLOW_CONFIG" && k != "GUI_PI_GATE_ARGS"));
        }
        // without a person IPC (dev runs) the gate variables travel in Pi's environment, and the launch says so
        let l = build(&s, &k, &p, "x", None, None).unwrap();
        assert!(l.gate_env_in_pi && l.gate_env.is_empty() && l.env.iter().any(|(k, _)| k == "GUI_GATE_REVIEWER_KEY"));
    }
    /// Item 11, shell side, end to end with fake keys in the keychain mock: Pi (a fake child that dumps its
    /// environment and echoes keys on stdout/stderr) gets only its own key; the gate's keys are only handed over
    /// through the person IPC, once, with the token; whatever the children print reaches the log and the UI masked.
    #[cfg(unix)]
    #[test]
    fn fake_keys_reach_only_the_intended_child_and_never_a_log() {
        use crate::human::{Outcome, Popup, Service};
        use crate::proc::{self, AgentEvent, OnEvent};
        use std::sync::{mpsc::channel, Arc, Mutex};
        let (mut s, k, p, dir) = setup();
        s.roles.insert("planner".into(), RoleCfg::Custom { model: ModelCfg { base_url: "https://p.example.com".into(), ..Default::default() } });
        k.set("role.planner", "sk-fake-planner-key-777").unwrap();
        let tf = dir.join("task.txt");
        let l = build(&s, &k, &p, "rename the file", Some(&tf), Some(("/run/x.sock", "tok-ipc-secret-123"))).unwrap();
        assert!(l.env.iter().any(|(k, v)| k == "GUI_PI_TASK_FILE" && v == &tf.display().to_string()));
        let red = Redactor::for_launch(&k, &l);
        // the shell's own environment holds gate-looking secrets too: none may be inherited
        let parent = vec![("PATH".to_string(), "/usr/bin:/bin".to_string()), ("GUI_GATE_REVIEWER_KEY".to_string(), "sk-parent-gate-key-1".to_string()),
                          ("ANTHROPIC_API_KEY".to_string(), "sk-parent-other-key-2".to_string())];
        let env = child_env(parent, &l);
        let dump = dir.join("pi-env.txt");
        let script = format!("env > {}; echo \"{{\\\"type\\\":\\\"message_end\\\",\\\"message\\\":{{\\\"role\\\":\\\"assistant\\\",\\\"content\\\":[{{\\\"type\\\":\\\"text\\\",\\\"text\\\":\\\"my key is $GUI_PI_MAIN_KEY\\\"}}]}}}}\"; echo \"oops $GUI_PI_MAIN_KEY $GUI_PI_HUMAN_TOKEN\" >&2; exit 0", dump.display());
        let log = Arc::new(Mutex::new(String::new()));
        let shown = Arc::new(Mutex::new(Vec::<String>::new()));
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let (l2, s2, r2) = (log.clone(), shown.clone(), red.clone());
        let on: OnEvent = Arc::new(move |e| {
            // what the shell does with each event: log the line masked, show the UI text masked
            match &e {
                AgentEvent::Line(x) => {
                    l2.lock().unwrap().push_str(&r2.line(x));
                    if let Some(ev) = crate::rpc::ui_event(x) {
                        s2.lock().unwrap().push(r2.line(&ev.text));
                    }
                }
                AgentEvent::Stderr(x) => l2.lock().unwrap().push_str(&r2.line(x)),
                _ => {}
            }
            let _ = tx.lock().unwrap().send(e);
        });
        let _a = proc::spawn(std::path::Path::new("/bin/sh"), &["-c".into(), script], &env, None, on).unwrap();
        loop {
            if let AgentEvent::Exited { .. } = rx.recv_timeout(std::time::Duration::from_secs(15)).unwrap() {
                break;
            }
        }
        let pi_env = std::fs::read_to_string(&dump).unwrap();
        assert!(pi_env.contains("GUI_PI_MAIN_KEY=sk-main-secret-0001"));
        for bad in ["sk-rev-secret-0002", "sk-fake-planner-key-777", "sk-parent-gate-key-1", "sk-parent-other-key-2", "GUI_GATE_"] {
            assert!(!pi_env.contains(bad), "{bad} in Pi's environment");
        }
        // the gate's keys: only through the IPC hand-over, once, with the token
        let popup: Popup = Arc::new(|_, _| Outcome::Yes);
        let svc = Service::new("tok-ipc-secret-123".into(), popup);
        svc.stage_gate_env(l.gate_env.clone());
        assert_eq!(svc.handle_line(r#"{"token":"wrong","op":"gate_env"}"#)["env"], serde_json::json!({}));
        let got = svc.handle_line(r#"{"token":"tok-ipc-secret-123","op":"gate_env"}"#)["env"].clone();
        assert_eq!(got["GUI_GATE_REVIEWER_KEY"], "sk-rev-secret-0002");
        assert_eq!(got["GUI_GATE_ROLE_PLANNER_KEY"], "sk-fake-planner-key-777");
        assert!(!got.to_string().contains("sk-main-secret-0001") && !got.to_string().contains("tok-ipc-secret-123"));
        assert_eq!(svc.handle_line(r#"{"token":"tok-ipc-secret-123","op":"gate_env"}"#)["env"], serde_json::json!({}));
        // logs and the timeline: masked
        let log = log.lock().unwrap().clone();
        assert!(log.contains("my key is ***") && log.contains("oops *** ***"), "{log}");
        for key in ["sk-main-secret-0001", "sk-rev-secret-0002", "sk-fake-planner-key-777", "tok-ipc-secret-123"] {
            assert!(!log.contains(key), "{key} in the log");
            assert!(shown.lock().unwrap().iter().all(|t| !t.contains(key)), "{key} shown");
        }
        assert!(l.redacted_env().iter().all(|(_, v)| !v.contains("secret") && !v.contains("sk-fake")));
    }

    #[test]
    fn no_launch_without_reviewer_key() {
        let (s, k, p, _) = setup();
        k.delete("reviewer").unwrap();
        let e = build(&s, &k, &p, "x", None, None).unwrap_err();
        assert!(e.iter().any(|x| x.contains("Reviewer")));
    }

    #[test]
    fn appimage_paths_never_reach_system_tools_or_children() {
        let kv = |v: &[(&str, &str)]| v.iter().map(|(k, x)| (k.to_string(), x.to_string())).collect::<Vec<_>>();
        // not an AppImage: unchanged
        let plain = kv(&[("PATH", "/usr/bin"), ("LD_LIBRARY_PATH", "/opt/x/lib")]);
        assert_eq!(system_env(plain.clone()), plain);
        let e = system_env(kv(&[("APPDIR", "/tmp/.mount_gui-piX"), ("APPIMAGE", "/home/u/gui-pi.AppImage"),
            ("LD_LIBRARY_PATH", "/tmp/.mount_gui-piX/usr/lib:/tmp/.mount_gui-piX/usr/lib/x86_64-linux-gnu"),
            ("PATH", "/tmp/.mount_gui-piX/usr/bin:/usr/local/bin:/usr/bin"), ("XDG_DATA_DIRS", "/tmp/.mount_gui-piX/usr/share:/usr/share"),
            ("GTK_PATH", "/tmp/.mount_gui-piX/usr/lib/gtk-3.0"), ("GDK_PIXBUF_MODULE_FILE", "/tmp/.mount_gui-piX/x.cache"),
            ("DISPLAY", ":1"), ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus")]));
        let get = |k: &str| e.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone());
        assert_eq!(get("LD_LIBRARY_PATH"), None);
        assert_eq!(get("GTK_PATH"), None);
        assert_eq!(get("GDK_PIXBUF_MODULE_FILE"), None);
        assert_eq!(get("APPDIR"), None);
        assert_eq!(get("PATH").as_deref(), Some("/usr/local/bin:/usr/bin"));
        assert_eq!(get("XDG_DATA_DIRS").as_deref(), Some("/usr/share"));
        assert_eq!(get("DISPLAY").as_deref(), Some(":1"));
        assert_eq!(get("DBUS_SESSION_BUS_ADDRESS").as_deref(), Some("unix:path=/run/user/1000/bus"));
    }
}
