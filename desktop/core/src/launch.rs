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
/// the tools Pi may call: the gate's GUI tools + the extension's note_fact (Pi's own bash/read/edit/write are not in
/// the list; gate_task / gate_confirm / gate_config are never offered)
pub const PI_TOOLS: &[&str] = &["list_apps", "list_windows", "get_window_state", "launch_app", "bring_to_front", "click",
    "double_click", "right_click", "type_text", "press_key", "hotkey", "scroll", "set_value", "drag", "invoke_menu",
    "run_actions", "run_command", "request_human", "set_plan", "check_done", "gate_report", "inspect", "note_fact"];
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
    k.ends_with("_KEY") || k.contains("TOKEN")
}

/// The complete environment of the Pi process: the allowlisted variables of `parent` (never a GUI_GATE_*, GUI_PI_*,
/// PI_* or other secret-looking variable of the shell's own environment), then the launch's own variables.
pub fn child_env(parent: impl IntoIterator<Item = (String, String)>, l: &Launch) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = parent.into_iter()
        .filter(|(k, _)| BASE_ENV_NAMES.contains(&k.as_str()) || BASE_ENV_PREFIXES.iter().any(|p| k.starts_with(p)))
        .filter(|(k, _)| !is_secret(k) && !FORBIDDEN_ENV.contains(&k.as_str()))
        .collect();
    for (k, v) in &l.env {
        if FORBIDDEN_ENV.contains(&k.as_str()) {
            continue;
        }
        out.retain(|(x, _)| x != k);
        out.push((k.clone(), v.clone()));
    }
    out
}

fn pi_models_json(m: &ModelCfg) -> String {
    let vision = m.vision.unwrap_or(true);
    json!({"providers": {PROVIDER: {
        "baseUrl": m.base_url.trim(), "api": m.protocol, "apiKey": MAIN_KEY_ENV,
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

/// Build the launch. `task` is the user's request verbatim (the gate treats it as the operator's own words).
pub fn build(s: &Settings, keys: &dyn KeyStore, p: &Paths, task: &str, human_ipc: Option<(&str, &str)>) -> Result<Launch, Vec<String>> {
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
        let l = build(&s, &k, &p, "rename the file", Some(("/run/x.sock", "tok"))).unwrap();
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
            assert!(content.contains("GUI_PI_MAIN_KEY"));
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
        let l = build(&s, &k, &p, "x", Some(("/run/x.sock", "tok"))).unwrap();
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
        let l = build(&s, &k, &p, "x", Some(("/run/x.sock", "tok"))).unwrap();
        assert!(l.gate_env.iter().any(|(k, v)| k == "GUI_GATE_ROLE_JUDGE_KEY" && v == "sk-main-secret-0001"));
    }

    #[test]
    fn the_launcher_never_allows_gate_config() {
        let (s, k, p, _) = setup();
        for ipc in [Some(("/run/x.sock", "tok")), None] {
            let l = build(&s, &k, &p, "x --allow-config", ipc).unwrap();
            let all: Vec<&(String, String)> = l.env.iter().chain(l.gate_env.iter()).collect();
            assert!(all.iter().all(|(k, _)| k != "GUI_GATE_ALLOW_CONFIG" && k != "GUI_PI_GATE_ARGS"));
            assert!(all.iter().all(|(_, v)| !v.contains("--allow-config") || v == "x --allow-config"));
            assert!(!l.args.iter().any(|a| a.starts_with("--allow-config")));
            let parent = vec![("GUI_GATE_ALLOW_CONFIG".to_string(), "1".to_string()), ("GUI_PI_GATE_ARGS".to_string(), "[\"--allow-config\"]".to_string())];
            assert!(child_env(parent, &l).iter().all(|(k, _)| k != "GUI_GATE_ALLOW_CONFIG" && k != "GUI_PI_GATE_ARGS"));
        }
        // without a person IPC (dev runs) the gate variables travel in Pi's environment, and the launch says so
        let l = build(&s, &k, &p, "x", None).unwrap();
        assert!(l.gate_env_in_pi && l.gate_env.is_empty() && l.env.iter().any(|(k, _)| k == "GUI_GATE_REVIEWER_KEY"));
    }
    #[test]
    fn no_launch_without_reviewer_key() {
        let (s, k, p, _) = setup();
        k.delete("reviewer").unwrap();
        let e = build(&s, &k, &p, "x", None).unwrap_err();
        assert!(e.iter().any(|x| x.contains("Reviewer")));
    }
}
