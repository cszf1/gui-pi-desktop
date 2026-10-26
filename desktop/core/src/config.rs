//! Settings the user edits on the settings page. No secrets in here: keys live in the OS keychain (see keys.rs).
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The gate's optional model roles (GUI_GATE_ROLE_<NAME>_*), all off by default.
pub const ROLES: &[&str] = &["feasibility", "planner", "verifier", "judge", "search", "compact", "grounder"];
/// Roles whose in-gate version stays OFF even when configured, unless the user opts in (`in_gate_roles`): they are
/// candidates to move into the client, and no net benefit of the gate's version is shown yet (review 2026-10-26 v2 §7).
pub const CLIENT_SIDE_ROLES: &[&str] = &["planner", "search", "compact"];
/// Protocols the main model can speak (Pi's `api` names).
pub const MAIN_PROTOCOLS: &[&str] = &["openai-completions", "openai-responses", "anthropic-messages", "google-generative-ai"];
/// Protocols the gate's reviewer / roles can speak (gui-gate GUI_GATE_REVIEWER_PROTOCOL).
pub const GATE_PROTOCOLS: &[&str] = &["chat", "responses", "anthropic", "gemini", "systemone"];
pub const MIN_HUMAN_TIMEOUT: u32 = 30;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
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

/// One optional gate role (Settings > Advanced). Off by default; when on, every field left empty inherits the
/// main model (endpoint, model id, key).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum RoleCfg {
    #[default]
    Off,
    /// the main model's endpoint, model and key
    Main,
    /// use the reviewer's endpoint (GUI_GATE_ROLE_<NAME>=reviewer)
    Reviewer,
    /// its own endpoint; empty fields (and a missing key) inherit the main model
    Custom { model: ModelCfg },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
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
    /// look up the latest release at start and show an "update available" notice (never installs anything)
    #[serde(default = "yes")]
    pub update_check: bool,
    /// planner / search / compact: run the gate's own version (opt-in; empty = all three off in the gate)
    #[serde(default)]
    pub in_gate_roles: Vec<String>,
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
            update_check: true,
            in_gate_roles: vec![],
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
pub fn all_slots() -> Vec<String> {
    let mut v = vec![slot_main(), slot_reviewer()];
    v.extend(ROLES.iter().map(|r| slot_role(r)));
    v
}

/// The main model's protocol in the gate's words (a role inheriting the main model speaks the same API).
pub fn gate_protocol_of_main(p: &str) -> &'static str {
    match p {
        "openai-responses" => "responses",
        "anthropic-messages" => "anthropic",
        "google-generative-ai" => "gemini",
        _ => "chat",
    }
}

/// https only; plain http only for this machine (localhost, 127.0.0.0/8, [::1]). No credentials in the URL.
pub fn check_base_url(u: &str) -> Result<(), String> {
    let u = u.trim();
    if u.is_empty() {
        return Err("base URL is required".into());
    }
    if u.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("base URL must not contain spaces".into());
    }
    let (scheme, rest) = u.split_once("://").ok_or("base URL must start with https://")?;
    let scheme = scheme.to_ascii_lowercase();
    let authority = rest.split(|c| c == '/' || c == '?' || c == '#').next().unwrap_or("");
    if authority.contains('@') {
        return Err("base URL must not contain a user name or password (the key goes in the key field)".into());
    }
    let host = if let Some(h) = authority.strip_prefix('[') {
        h.split(']').next().unwrap_or("").to_ascii_lowercase()
    } else {
        authority.rsplit_once(':').map(|(h, port)| if port.chars().all(|c| c.is_ascii_digit()) { h } else { authority }).unwrap_or(authority).to_ascii_lowercase()
    };
    if host.is_empty() {
        return Err("base URL has no host".into());
    }
    let local = host == "localhost" || host == "::1" || (host.starts_with("127.") && host.split('.').count() == 4 && host.split('.').all(|p| p.parse::<u8>().is_ok()));
    match scheme.as_str() {
        "https" => Ok(()),
        "http" if local => Ok(()),
        "http" => Err("use https (plain http only for localhost)".into()),
        _ => Err("base URL must start with https://".into()),
    }
}

fn check_model(what: &str, m: &ModelCfg, protocols: &[&str], has_key: bool, errs: &mut Vec<String>) {
    if !protocols.contains(&m.protocol.as_str()) {
        errs.push(format!("{what}: choose a protocol ({})", protocols.join(", ")));
    }
    if let Err(e) = check_base_url(&m.base_url) {
        errs.push(format!("{what}: {e}"));
    }
    if m.model.trim().is_empty() {
        errs.push(format!("{what}: model name is required"));
    }
    if !has_key {
        errs.push(format!("{what}: API key is not set"));
    }
}

/// What a role really uses once inheritance is applied: endpoint + which key slot holds its key.
#[derive(Clone, Debug, PartialEq)]
pub enum EffRole {
    Off,
    Reviewer,
    Model { model: ModelCfg, key_slot: String },
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
                // every field may be left empty (inherits the main model); what is filled in must be valid
                if !model.protocol.is_empty() && !GATE_PROTOCOLS.contains(&model.protocol.as_str()) {
                    errs.push(format!("Advanced / role {name}: choose a protocol ({})", GATE_PROTOCOLS.join(", ")));
                }
                if !model.base_url.trim().is_empty() {
                    if let Err(e) = check_base_url(&model.base_url) {
                        errs.push(format!("Advanced / role {name}: {e}"));
                    }
                }
            }
        }
        for r in &self.in_gate_roles {
            if !CLIENT_SIDE_ROLES.contains(&r.as_str()) {
                errs.push(format!("Advanced: only {} can be switched to the gate's own version ({r} cannot)", CLIENT_SIDE_ROLES.join(", ")));
            }
        }
        if self.human_timeout_s < MIN_HUMAN_TIMEOUT {
            errs.push(format!("Person timeout must be at least {MIN_HUMAN_TIMEOUT} s"));
        }
        errs
    }

    /// Whether the gate may run this role. Precedence (highest first): the role switched Off -> off, whatever group
    /// switch (GUI_GATE_097, GUI_GATE_ROLES_ENABLE) or configured endpoint says; planner / search / compact -> off
    /// unless opted in (`in_gate_roles`); otherwise on when configured.
    pub fn role_enabled(&self, name: &str) -> bool {
        match self.roles.get(name) {
            None | Some(RoleCfg::Off) => false,
            Some(_) => !CLIENT_SIDE_ROLES.contains(&name) || self.in_gate_roles.iter().any(|r| r == name),
        }
    }

    /// A role with inheritance applied. `has_key(slot)` tells whether a custom role has its own key.
    pub fn effective_role(&self, name: &str, has_key: &dyn Fn(&str) -> bool) -> EffRole {
        let inherit = || ModelCfg {
            protocol: gate_protocol_of_main(&self.main.protocol).into(),
            base_url: self.main.base_url.trim().into(),
            model: self.main.model.trim().into(),
            timeout_s: None,
            thinking: None,
            vision: None,
        };
        match self.roles.get(name).cloned().unwrap_or_default() {
            RoleCfg::Off => EffRole::Off,
            RoleCfg::Reviewer => EffRole::Reviewer,
            RoleCfg::Main => EffRole::Model { model: inherit(), key_slot: slot_main() },
            RoleCfg::Custom { model } => {
                let base = inherit();
                let own_url = !model.base_url.trim().is_empty();
                let m = ModelCfg {
                    // an own endpoint without an own protocol defaults to chat, never the main model's protocol
                    protocol: if !model.protocol.is_empty() { model.protocol.clone() } else if own_url { "chat".into() } else { base.protocol },
                    base_url: if own_url { model.base_url.trim().into() } else { base.base_url },
                    model: if model.model.trim().is_empty() { base.model } else { model.model.trim().into() },
                    timeout_s: model.timeout_s,
                    thinking: None,
                    vision: None,
                };
                let key_slot = if has_key(&slot_role(name)) { slot_role(name) } else { slot_main() };
                EffRole::Model { model: m, key_slot }
            }
        }
    }

    pub fn load(json: &str) -> Result<Settings, String> {
        let mut s: Settings = serde_json::from_str(json).map_err(|e| e.to_string())?;
        for r in ROLES {
            s.roles.entry(r.to_string()).or_default();
        }
        Ok(s)
    }

    /// The settings file's content. Refuses when any stored key value would end up in it (a key pasted into a
    /// URL or model field): keys live in the keychain (or this run's memory) only.
    pub fn to_file_json(&self, secrets: &[String]) -> Result<String, String> {
        let j = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        for k in secrets {
            let k = k.trim();
            if k.len() >= 6 && j.contains(k) {
                return Err("an API key was typed into a settings field; put keys only in the key fields".into());
            }
        }
        Ok(j)
    }

    /// Write `config.json` into `dir` (0700 dir, 0600 file on unix). Never contains key material.
    pub fn save_to(&self, dir: &std::path::Path, secrets: &[String]) -> Result<std::path::PathBuf, String> {
        let j = self.to_file_json(secrets)?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let p = dir.join("config.json");
        let tmp = dir.join("config.json.tmp");
        std::fs::write(&tmp, j).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
        Ok(p)
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
    fn roles_default_off_optional_and_inherit_the_main_model() {
        let mut s = filled();
        assert_eq!(s.roles.len(), 7);
        assert!(s.roles.values().all(|r| *r == RoleCfg::Off));
        assert_eq!(s.effective_role("planner", &|_| true), EffRole::Off);
        // an empty custom role is valid and is the main model, with the main key
        s.roles.insert("grounder".into(), RoleCfg::Custom { model: ModelCfg::default() });
        assert!(s.validate(&|slot| slot != "role.grounder").is_empty());
        match s.effective_role("grounder", &|slot| slot != "role.grounder") {
            EffRole::Model { model, key_slot } => {
                assert_eq!((model.base_url.as_str(), model.model.as_str(), model.protocol.as_str()), ("https://api.example.com/v1", "m", "chat"));
                assert_eq!(key_slot, "main");
            }
            e => panic!("{e:?}"),
        }
        // own endpoint, own key, model id inherited
        s.roles.insert("grounder".into(), RoleCfg::Custom { model: ModelCfg { base_url: "https://g.example.com".into(), ..Default::default() } });
        match s.effective_role("grounder", &|_| true) {
            EffRole::Model { model, key_slot } => {
                assert_eq!((model.base_url.as_str(), model.model.as_str(), model.protocol.as_str()), ("https://g.example.com", "m", "chat"));
                assert_eq!(key_slot, "role.grounder");
            }
            e => panic!("{e:?}"),
        }
        s.roles.insert("judge".into(), RoleCfg::Main);
        s.main.protocol = "anthropic-messages".into();
        assert!(matches!(s.effective_role("judge", &|_| true), EffRole::Model { ref model, .. } if model.protocol == "anthropic"));
        s.roles.insert("grounder".into(), RoleCfg::Custom { model: ModelCfg { base_url: "http://g.example.com".into(), ..Default::default() } });
        assert!(s.validate(&|_| true).iter().any(|e| e.contains("Advanced / role grounder") && e.contains("https")));
    }

    #[test]
    fn explicit_off_wins_and_client_side_roles_are_off_in_the_gate_by_default() {
        let mut s = filled();
        for r in ROLES {
            assert!(!s.role_enabled(r), "{r}: off by default");
        }
        for r in ROLES {
            s.roles.insert(r.to_string(), RoleCfg::Main);
        }
        for r in ROLES {
            assert_eq!(s.role_enabled(r), !CLIENT_SIDE_ROLES.contains(r), "{r}");
        }
        s.in_gate_roles = vec!["compact".into()];
        assert!(s.role_enabled("compact") && !s.role_enabled("planner") && !s.role_enabled("search"));
        // explicit off beats the opt-in and the configured endpoint
        s.roles.insert("compact".into(), RoleCfg::Off);
        assert!(!s.role_enabled("compact"));
        s.roles.insert("verifier".into(), RoleCfg::Off);
        assert!(!s.role_enabled("verifier"));
        assert!(s.validate(&|_| true).is_empty());
        s.in_gate_roles = vec!["verifier".into()];
        assert!(s.validate(&|_| true).iter().any(|e| e.contains("verifier cannot")));
        // older settings files (no in_gate_roles) still load
        let mut v = serde_json::to_value(filled()).unwrap();
        v.as_object_mut().unwrap().remove("in_gate_roles");
        assert_eq!(Settings::load(&v.to_string()).unwrap().in_gate_roles, Vec::<String>::new());
    }

    #[test]
    fn model_names_are_required() {
        let mut s = filled();
        s.main.model = "  ".into();
        s.reviewer.model.clear();
        let e = s.validate(&|_| true);
        assert!(e.iter().any(|x| x.starts_with("Main model") && x.contains("model name")));
        assert!(e.iter().any(|x| x.starts_with("Reviewer") && x.contains("model name")));
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
        for ok in ["https://api.example.com/v1", "http://localhost:11434/v1", "http://[::1]:8080", "HTTPS://X.example.com", "http://127.1.2.3/v1"] {
            assert!(check_base_url(ok).is_ok(), "{ok}");
        }
        for bad in ["", "api.example.com", "ftp://x.example.com", "http://localhost.evil.com/v1", "http://127.0.0.1.evil.com",
                    "http://localhost@evil.com", "https://user:pw@api.example.com", "https://", "https://a b.com", "file:///etc/passwd",
                    "http://10.0.0.5:8000"] {
            assert!(check_base_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn reviewer_cannot_be_switched_off_by_a_settings_file() {
        // there is no field to turn it off: an "enabled": false is an unknown field and the file is refused
        let mut v = serde_json::to_value(filled()).unwrap();
        v["reviewer_enabled"] = serde_json::json!(false);
        assert!(Settings::load(&v.to_string()).is_err());
        let mut v = serde_json::to_value(filled()).unwrap();
        v["reviewer"]["enabled"] = serde_json::json!(false);
        assert!(Settings::load(&v.to_string()).is_err());
    }

    #[test]
    fn settings_files_never_contain_key_material() {
        use crate::keys::{KeyStore, MemoryStore};
        let keys = MemoryStore::default();
        keys.set("main", "sk-main-0123456789abcdef").unwrap();
        keys.set("reviewer", "sk-rev-0123456789abcdef").unwrap();
        keys.set("role.grounder", "sk-role-0123456789abcd").unwrap();
        let secrets: Vec<String> = all_slots().iter().filter_map(|s| keys.get(s)).collect();
        let dir = std::env::temp_dir().join(format!("gpd-cfg-{}", std::process::id()));
        let mut s = filled();
        s.roles.insert("grounder".into(), RoleCfg::Custom { model: ModelCfg { base_url: "https://g.example.com".into(), ..Default::default() } });
        let p = s.save_to(&dir, &secrets).unwrap();
        // a key pasted into a normal field is refused, and nothing is written
        let mut bad = s.clone();
        bad.reviewer.model = "sk-rev-0123456789abcdef".into();
        assert!(bad.save_to(&dir, &secrets).is_err());
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let c = std::fs::read_to_string(e.path()).unwrap();
            for k in &secrets {
                assert!(!c.contains(k.as_str()), "{} contains a key", e.path().display());
            }
            assert!(!c.to_lowercase().contains("api_key") && !c.to_lowercase().contains("\"key\""));
        }
        assert_eq!(Settings::load(&std::fs::read_to_string(&p).unwrap()).unwrap(), s);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_file_has_no_key_field() {
        let j = serde_json::to_string(&filled()).unwrap();
        assert!(!j.to_lowercase().contains("key"), "{j}");
        let back = Settings::load(&j).unwrap();
        assert_eq!(back, filled());
    }
}
