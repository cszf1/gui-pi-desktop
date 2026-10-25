//! How the shell starts the agent: `node pi --mode rpc` with the gui-pi extension, which starts gui-gate, which
//! starts the bundled cua-driver. Keys go ONLY into the child environment (never into a file or a log line); the
//! generated models.json names the environment variable, not the key.
use crate::config::{slot_main, slot_reviewer, slot_role, ModelCfg, RoleCfg, Settings, ROLES};
use crate::keys::KeyStore;
use serde_json::json;
use std::path::{Path, PathBuf};

pub const MAIN_KEY_ENV: &str = "GUI_PI_MAIN_KEY";
pub const PROVIDER: &str = "gui-main";
/// the gate tools Pi may call (Pi's own bash/read/edit/write are not in the list)
pub const PI_TOOLS: &[&str] = &["list_apps", "list_windows", "get_window_state", "launch_app", "bring_to_front", "click",
    "double_click", "right_click", "type_text", "press_key", "hotkey", "scroll", "set_value", "drag", "invoke_menu",
    "run_actions", "run_command", "request_human", "set_plan", "check_done", "gate_report", "inspect"];

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
    /// variables to set on top of the cleaned parent environment
    pub env: Vec<(String, String)>,
    /// variable names removed from the parent environment first
    pub env_remove_prefixes: Vec<&'static str>,
    /// files to write before start (path, content): no secrets in them
    pub files: Vec<(PathBuf, String)>,
}

impl Launch {
    /// For logs and the UI: the environment with every secret value masked.
    pub fn redacted_env(&self) -> Vec<(String, String)> {
        self.env.iter().map(|(k, v)| (k.clone(), if is_secret(k) { "***".into() } else { v.clone() })).collect()
    }
}

pub fn is_secret(k: &str) -> bool {
    k.ends_with("_KEY") || k.contains("TOKEN")
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
    // the gate: reviewer (mandatory) and roles; the user's own ~/.config/gui-gate/reviewer.env is never read
    gate_model_env("GUI_GATE_REVIEWER", &s.reviewer, keys.get(&slot_reviewer()), &mut env);
    env.push(("GUI_GATE_REVIEWER_ENV".into(), p.data.join("no-reviewer.env").display().to_string()));
    for r in ROLES {
        let up = r.to_uppercase();
        match s.roles.get(*r).cloned().unwrap_or_default() {
            RoleCfg::Off => {}
            RoleCfg::Reviewer => env.push((format!("GUI_GATE_ROLE_{up}"), "reviewer".into())),
            RoleCfg::Custom { model } => gate_model_env(&format!("GUI_GATE_ROLE_{up}"), &model, keys.get(&slot_role(r)), &mut env),
        }
    }
    if s.gate_097 {
        env.push(("GUI_GATE_097".into(), "1".into()));
    }
    env.push(("GUI_GATE_NO_UPDATE_CHECK".into(), "1".into()));
    // the extension
    env.push(("GUI_PI_GATE".into(), p.gate.display().to_string()));
    if let Some(d) = &p.driver {
        env.push(("GUI_PI_DRIVER".into(), d.display().to_string()));
    }
    env.push(("GUI_PI_TASK".into(), task.into()));
    env.push(("GUI_PI_HUMAN_TIMEOUT".into(), s.human_timeout_s.to_string()));
    env.push(("GUI_PI_GATE_LOG".into(), p.data.join("logs").join("gate.jsonl").display().to_string()));
    if let Some((path, token)) = human_ipc {
        env.push(("GUI_PI_HUMAN_IPC".into(), path.into()));
        env.push(("GUI_PI_HUMAN_TOKEN".into(), token.into()));
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
    Ok(Launch {
        program: p.node.clone(),
        args,
        env,
        env_remove_prefixes: vec!["GUI_GATE_", "GUI_PI_", "PI_"],
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
        let get = |n: &str| l.env.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
        assert_eq!(get("GUI_PI_MAIN_KEY").as_deref(), Some("sk-main-secret-0001"));
        assert_eq!(get("GUI_GATE_REVIEWER_KEY").as_deref(), Some("sk-rev-secret-0002"));
        assert_eq!(get("GUI_GATE_REVIEWER_PROTOCOL").as_deref(), Some("systemone"));
        assert_eq!(get("GUI_GATE_ROLE_GROUNDER").as_deref(), Some("reviewer"));
        assert!(get("GUI_GATE_ROLE_PLANNER").is_none() && get("GUI_GATE_ROLE_PLANNER_URL").is_none());
        assert_eq!(get("GUI_PI_TASK").as_deref(), Some("rename the file"));
        for (_, content) in &l.files {
            assert!(!content.contains("secret"), "{content}");
            assert!(content.contains("GUI_PI_MAIN_KEY"));
        }
        assert!(!l.args.iter().any(|a| a.contains("secret")));
        assert!(l.redacted_env().iter().all(|(_, v)| !v.contains("secret")));
        // only the gate's tools; Pi's own bash is not among them
        let tools = &l.args[l.args.iter().position(|a| a == "--tools").unwrap() + 1];
        assert!(tools.contains("click") && !tools.split(',').any(|t| t == "bash"));
        assert!(l.args.contains(&"--skill".to_string()));
        assert_eq!(l.args.last().unwrap(), "medium");
    }

    #[test]
    fn no_launch_without_reviewer_key() {
        let (s, k, p, _) = setup();
        k.delete("reviewer").unwrap();
        let e = build(&s, &k, &p, "x", None).unwrap_err();
        assert!(e.iter().any(|x| x.contains("Reviewer")));
    }
}
