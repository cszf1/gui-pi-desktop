//! Settings the user edits on the settings page. No secrets in here: keys live in the OS keychain (see keys.rs).
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The gate's optional model roles (GUI_GATE_ROLE_<NAME>_*), all off by default.
pub const ROLES: &[&str] = &["feasibility", "planner", "verifier", "judge", "search", "compact", "grounder"];
/// Protocols the main model can speak (Pi's `api` names).
pub const MAIN_PROTOCOLS: &[&str] = &["openai-completions", "openai-responses", "anthropic-messages", "google-generative-ai"];
/// Protocols the gate's reviewer / roles can speak (gui-gate GUI_GATE_REVIEWER_PROTOCOL).
pub const GATE_PROTOCOLS: &[&str] = &["chat", "responses", "anthropic", "gemini", "systemone"];
pub const MIN_HUMAN_TIMEOUT: u32 = 30;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ModelCfg {
    pub protocol: String,
    pub base_url: String,
    pub model: String,
    /// seconds (reviewer / roles)
    #[serde(default)]
    pub timeout_s: Option<u32>,
    /// main model only: thinking level (default medium) and whether it reads images
    #[serde(default)]
    pub thinking: Option<String>,
    #[serde(default)]
    pub vision: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum RoleCfg {
    #[default]
    Off,
    /// use the reviewer's endpoint (GUI_GATE_ROLE_<NAME>=reviewer)
    Reviewer,
    Custom { model: ModelCfg },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub main: ModelCfg,
    /// mandatory: there is no way to turn the reviewer off
    pub reviewer: ModelCfg,
    #[serde(default)]
    pub roles: BTreeMap<String, RoleCfg>,
    #[serde(default = "default_timeout")]
    pub human_timeout_s: u32,
    /// the 0.9.7+ gate features (plan, check_done, inspect, loop breaker...), on by default like the v0.9.9 bench
    #[serde(default = "yes")]
    pub gate_097: bool,
}

fn default_timeout() -> u32 {
    120
}
fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            main: ModelCfg { protocol: "openai-completions".into(), thinking: Some("medium".into()), vision: Some(true), ..Default::default() },
            reviewer: ModelCfg { protocol: "chat".into(), timeout_s: Some(60), ..Default::default() },
            roles: ROLES.iter().map(|r| (r.to_string(), RoleCfg::Off)).collect(),
            human_timeout_s: 120,
            gate_097: true,
        }
    }
}

/// Key slot names in the keychain.
pub fn slot_main() -> String {
    "main".into()
}
pub fn slot_reviewer() -> String {
    "reviewer".into()
}
pub fn slot_role(r: &str) -> String {
    format!("role.{r}")
}

fn check_model(what: &str, m: &ModelCfg, protocols: &[&str], has_key: bool, errs: &mut Vec<String>) {
    if !protocols.contains(&m.protocol.as_str()) {
        errs.push(format!("{what}: choose a protocol ({})", protocols.join(", ")));
    }
    let u = m.base_url.trim();
    if u.is_empty() {
        errs.push(format!("{what}: base URL is required"));
    } else if !(u.starts_with("https://") || u.starts_with("http://127.0.0.1") || u.starts_with("http://localhost")) {
        errs.push(format!("{what}: use https (plain http only for localhost)"));
    }
    if m.model.trim().is_empty() {
        errs.push(format!("{what}: model id is required"));
    }
    if !has_key {
        errs.push(format!("{what}: API key is not set"));
    }
}

impl Settings {
    /// Everything that must hold before a task may start. `has_key(slot)` asks the key store.
    pub fn validate(&self, has_key: &dyn Fn(&str) -> bool) -> Vec<String> {
        let mut errs = vec![];
        check_model("Main model", &self.main, MAIN_PROTOCOLS, has_key(&slot_main()), &mut errs);
        check_model("Reviewer model (required)", &self.reviewer, GATE_PROTOCOLS, has_key(&slot_reviewer()), &mut errs);
        if let Some(t) = &self.main.thinking {
            if !["medium", "high", "xhigh"].contains(&t.as_str()) {
                errs.push("Main model: thinking must be medium or higher".into());
            }
        }
        for (name, r) in &self.roles {
            if !ROLES.contains(&name.as_str()) {
                errs.push(format!("unknown role {name}"));
                continue;
            }
            if let RoleCfg::Custom { model } = r {
                check_model(&format!("Role {name}"), model, GATE_PROTOCOLS, has_key(&slot_role(name)), &mut errs);
            }
        }
        if self.human_timeout_s < MIN_HUMAN_TIMEOUT {
            errs.push(format!("Person timeout must be at least {MIN_HUMAN_TIMEOUT} s"));
        }
        errs
    }

    pub fn load(json: &str) -> Result<Settings, String> {
        let mut s: Settings = serde_json::from_str(json).map_err(|e| e.to_string())?;
        for r in ROLES {
            s.roles.entry(r.to_string()).or_default();
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled() -> Settings {
        let mut s = Settings::default();
        s.main.base_url = "https://api.example.com/v1".into();
        s.main.model = "m".into();
        s.reviewer.base_url = "https://rev.example.com/v1/chat/completions".into();
        s.reviewer.model = "r".into();
        s
    }

    #[test]
    fn reviewer_is_mandatory() {
        let mut s = filled();
        assert!(s.validate(&|_| true).is_empty(), "{:?}", s.validate(&|_| true));
        s.reviewer.base_url.clear();
        assert!(s.validate(&|_| true).iter().any(|e| e.contains("Reviewer")));
        let s = filled();
        assert!(s.validate(&|slot| slot != "reviewer").iter().any(|e| e.contains("Reviewer") && e.contains("key")));
    }

    #[test]
    fn roles_default_off_and_custom_needs_its_key() {
        let mut s = filled();
        assert_eq!(s.roles.len(), 7);
        assert!(s.roles.values().all(|r| *r == RoleCfg::Off));
        s.roles.insert("grounder".into(), RoleCfg::Custom { model: ModelCfg { protocol: "chat".into(), base_url: "https://g.example.com".into(), model: "g".into(), ..Default::default() } });
        assert!(s.validate(&|slot| slot != "role.grounder").iter().any(|e| e.contains("grounder")));
        assert!(s.validate(&|_| true).is_empty());
    }

    #[test]
    fn low_thinking_and_short_timeouts_are_refused() {
        let mut s = filled();
        s.main.thinking = Some("low".into());
        s.human_timeout_s = 5;
        let e = s.validate(&|_| true);
        assert!(e.iter().any(|x| x.contains("thinking")));
        assert!(e.iter().any(|x| x.contains("timeout")));
    }

    #[test]
    fn plain_http_only_for_localhost() {
        let mut s = filled();
        s.main.base_url = "http://api.example.com".into();
        assert!(s.validate(&|_| true).iter().any(|e| e.contains("https")));
        s.main.base_url = "http://127.0.0.1:8080/v1".into();
        assert!(s.validate(&|_| true).is_empty());
    }

    #[test]
    fn settings_file_has_no_key_field() {
        let j = serde_json::to_string(&filled()).unwrap();
        assert!(!j.to_lowercase().contains("key"), "{j}");
        let back = Settings::load(&j).unwrap();
        assert_eq!(back, filled());
    }
}
