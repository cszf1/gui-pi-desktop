//! The ask-a-person service: the gui-pi extension connects, sends one request line, gets one reply line.
//!   request  {"token","id","kind","message","schema","timeout_s","deadline_ms","meta"}
//!   reply    {"id","action":"accept"|"decline"|"cancel","content":{...},"reason"?}
//!   request  {"token","op":"gate_env"}  ->  {"env":{GUI_GATE_*...}}   once per task (one-shot), then {"env":{}}
//! The shell shows a native popup per request (one at a time) and maps the button to the kind's form.
//! A wrong token, an unknown kind, a closed popup, the app quitting or the deadline passing is never an approval;
//! every refusal carries its reason (declined / cancelled / timed_out / closed / unavailable). Each request id is
//! answered once: a repeated id is refused without a popup, and only the first click on a popup counts.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const KINDS: &[&str] = &["approve", "answer", "takeover", "ask_help"];

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct Request {
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub timeout_s: f64,
    #[serde(default)]
    pub deadline_ms: Option<i64>,
    #[serde(default)]
    pub meta: Value,
    /// "gate_env" for the one-shot hand-over of the gate's settings; empty for a person request
    #[serde(default)]
    pub op: String,
}

/// What the person did in the popup.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// approve: allow; takeover: done; ask_help: I did it
    Yes,
    /// approve: deny; takeover: no; ask_help: try another way (with an optional hint)
    No(String),
    /// answer: the text
    Text(String),
    /// the person closed the question window
    Closed,
    /// the deadline passed
    Timeout,
    /// the app quit, the task was stopped, or the agent process ended while the question was open
    Cancelled,
    /// no popup could be shown (no window, the service is shutting down)
    Unavailable,
}

/// The reason a refusal is recorded with, as the gate and the extension name it. None: the person answered.
pub fn reason(o: &Outcome) -> Option<&'static str> {
    match o {
        Outcome::Closed => Some("closed"),
        Outcome::Timeout => Some("timed_out"),
        Outcome::Cancelled => Some("cancelled"),
        Outcome::Unavailable => Some("unavailable"),
        _ => None,
    }
}

/// For the main window's timeline: what happened to a question. Everything but a yes / an answer is a refusal.
pub fn label(kind: &str, o: &Outcome) -> (&'static str, bool) {
    match (kind, o) {
        ("approve", Outcome::Yes) => ("allowed", false),
        ("approve", Outcome::No(_)) => ("declined", true),
        ("takeover", Outcome::Yes) | ("ask_help", Outcome::Yes) => ("done by the person", false),
        ("takeover", Outcome::No(_)) => ("declined", true),
        ("ask_help", Outcome::No(_)) => ("try another way", false),
        ("answer", Outcome::Text(t)) if !t.trim().is_empty() => ("answered", false),
        (_, Outcome::Closed) => ("window closed", true),
        (_, Outcome::Timeout) => ("timed out", true),
        (_, Outcome::Cancelled) => ("cancelled", true),
        (_, Outcome::Unavailable) => ("unavailable", true),
        _ => ("declined", true),
    }
}

fn refusal(action: &str, why: &str) -> Value {
    json!({"action": action, "reason": why})
}

/// Popup button -> the kind's ElicitResult. Closing the window or quitting is a cancel; a timeout or no popup
/// is a decline; both are refusals with their reason.
pub fn to_elicit(kind: &str, o: &Outcome) -> Value {
    match (kind, o) {
        (_, Outcome::Closed) => refusal("cancel", "closed"),
        (_, Outcome::Cancelled) => refusal("cancel", "cancelled"),
        (_, Outcome::Timeout) => refusal("decline", "timed_out"),
        (_, Outcome::Unavailable) => refusal("decline", "unavailable"),
        ("approve", Outcome::Yes) => json!({"action": "accept", "content": {"decision": "allow"}}),
        ("approve", Outcome::No(_)) => json!({"action": "accept", "content": {"decision": "deny"}}),
        ("takeover", Outcome::Yes) => json!({"action": "accept", "content": {"done": true}}),
        ("takeover", Outcome::No(_)) => json!({"action": "accept", "content": {"done": false}}),
        ("ask_help", Outcome::Yes) => json!({"action": "accept", "content": {"choice": "did_it", "note": ""}}),
        ("ask_help", Outcome::No(n)) => json!({"action": "accept", "content": {"choice": "try_other", "note": n.chars().take(300).collect::<String>()}}),
        ("answer", Outcome::Text(t)) if !t.trim().is_empty() => json!({"action": "accept", "content": {"answer": t.chars().take(2000).collect::<String>()}}),
        _ => refusal("decline", "declined"),
    }
}

/// Open questions, one answer each. `open` registers a question; the first `answer` for it wins, later ones are
/// ignored (a double click, a stale popup, a replayed message); `cancel_all` ends every open question as cancelled
/// (app quit, task stopped, agent process gone).
#[derive(Default)]
pub struct Pending {
    open: Mutex<HashMap<String, Sender<Outcome>>>,
    used: Mutex<HashSet<String>>,
}

impl Pending {
    /// None when this id was already used (a second question with the same id is never shown).
    pub fn open(&self, id: &str) -> Option<Receiver<Outcome>> {
        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        if !used.insert(id.to_string()) {
            return None;
        }
        let (tx, rx) = channel();
        self.open.lock().unwrap_or_else(|e| e.into_inner()).insert(id.to_string(), tx);
        Some(rx)
    }
    /// true only for the first answer to an open question.
    pub fn answer(&self, id: &str, o: Outcome) -> bool {
        let tx = self.open.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
        match tx {
            Some(tx) => tx.send(o).is_ok(),
            None => false,
        }
    }
    pub fn is_open(&self, id: &str) -> bool {
        self.open.lock().unwrap_or_else(|e| e.into_inner()).contains_key(id)
    }
    pub fn open_ids(&self) -> Vec<String> {
        self.open.lock().unwrap_or_else(|e| e.into_inner()).keys().cloned().collect()
    }
    /// End every open question as Cancelled; returns how many there were.
    pub fn cancel_all(&self) -> usize {
        let all: Vec<(String, Sender<Outcome>)> = self.open.lock().unwrap_or_else(|e| e.into_inner()).drain().collect();
        let n = all.len();
        for (_, tx) in all {
            let _ = tx.send(Outcome::Cancelled);
        }
        n
    }
    /// Wait for the answer (or `seconds`); the question is closed afterwards either way.
    pub fn wait(&self, id: &str, rx: Receiver<Outcome>, seconds: f64) -> Outcome {
        let o = rx.recv_timeout(Duration::from_secs_f64(seconds.max(0.0))).unwrap_or(Outcome::Timeout);
        self.open.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
        o
    }
}

/// Seconds the popup may stay up: the gate's deadline when given, else timeout_s, clamped to [0, 3600].
pub fn seconds_left(r: &Request, now_ms: i64) -> f64 {
    let by_deadline = r.deadline_ms.map(|d| (d - now_ms) as f64 / 1000.0);
    let t = by_deadline.unwrap_or(if r.timeout_s > 0.0 { r.timeout_s } else { 120.0 });
    t.clamp(0.0, 3600.0)
}

fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A fresh random token (std's randomly keyed hasher + time; 128 bits of hex).
pub fn new_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for i in 0..2u64 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(i);
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

/// Shows a popup and blocks until the person answers or `seconds` pass.
pub type Popup = Arc<dyn Fn(&Request, f64) -> Outcome + Send + Sync>;

/// The service behind the socket: token, popup, one question at a time, seen request ids, the gate's settings
/// for the current task (handed over once), and a closing flag (app quitting: every request is cancelled).
pub struct Service {
    token: String,
    popup: Popup,
    serial: Mutex<()>,
    seen: Mutex<HashSet<String>>,
    gate_env: Mutex<Option<Vec<(String, String)>>>,
    closing: AtomicBool,
}

impl Service {
    pub fn new(token: String, popup: Popup) -> Arc<Service> {
        Arc::new(Service { token, popup, serial: Mutex::new(()), seen: Mutex::new(HashSet::new()), gate_env: Mutex::new(None), closing: AtomicBool::new(false) })
    }
    /// Stage the gate's variables for the task about to start (replaces anything not yet collected).
    pub fn stage_gate_env(&self, env: Vec<(String, String)>) {
        *self.gate_env.lock().unwrap_or_else(|e| e.into_inner()) = Some(env);
    }
    /// Drop staged variables that were never collected (the task failed to start or ended).
    pub fn clear_gate_env(&self) {
        *self.gate_env.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    pub fn set_closing(&self) {
        self.closing.store(true, Ordering::SeqCst);
    }

    /// Handle one connection's request line.
    pub fn handle_line(&self, line: &str) -> Value {
        let req: Request = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(_) => return json!({"id": "", "action": "decline", "reason": "declined"}),
        };
        let id = req.id.clone();
        let mut deny = refusal("decline", "declined");
        deny["id"] = json!(id);
        if !ct_eq(&req.token, &self.token) {
            return if req.op.is_empty() { deny } else { json!({"env": {}}) };
        }
        if req.op == "gate_env" {
            let env = self.gate_env.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or_default();
            let m: serde_json::Map<String, Value> = env.into_iter().map(|(k, v)| (k, json!(v))).collect();
            return json!({"env": m});
        }
        if !req.op.is_empty() || !KINDS.contains(&req.kind.as_str()) {
            return deny;
        }
        if id.is_empty() || !self.seen.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone()) {
            return deny; // one answer per request id: a replay never shows a popup
        }
        let cancelled = || {
            let mut r = to_elicit(&req.kind, &Outcome::Cancelled);
            r["id"] = json!(id);
            r
        };
        if self.closing.load(Ordering::SeqCst) {
            return cancelled();
        }
        let _one_at_a_time = self.serial.lock().unwrap_or_else(|e| e.into_inner());
        if self.closing.load(Ordering::SeqCst) {
            return cancelled();
        }
        let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        let left = seconds_left(&req, now_ms);
        if left < 1.0 {
            let mut r = to_elicit(&req.kind, &Outcome::Timeout); // waited in the queue past its deadline
            r["id"] = json!(id);
            return r;
        }
        let t0 = Instant::now();
        let mut o = (self.popup)(&req, left);
        if t0.elapsed() > Duration::from_secs_f64(left + 1.0) && !matches!(o, Outcome::Cancelled) {
            o = Outcome::Timeout;
        }
        let mut r = to_elicit(&req.kind, &o);
        r["id"] = json!(id);
        r
    }
}

/// One-call form for callers that need no state between requests (tests, simple embedders).
pub fn handle_line(line: &str, token: &str, popup: &Popup, _serial: &Mutex<()>) -> Value {
    Service::new(token.to_string(), popup.clone()).handle_line(line)
}

fn serve_conn<S: std::io::Read + Write>(s: S, rd: impl std::io::Read, svc: &Service) {
    let mut line = String::new();
    if BufReader::new(rd).read_line(&mut line).is_ok() {
        let r = svc.handle_line(line.trim());
        let mut w = s;
        let _ = w.write_all(format!("{r}\n").as_bytes());
    }
}

/// Serve requests on a Unix socket (Linux / macOS); the socket file is 0600 in a 0700 directory.
#[cfg(unix)]
pub fn serve_unix(path: &std::path::Path, svc: Arc<Service>) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    let _ = std::fs::remove_file(path);
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
        std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700))?;
    }
    let l = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let svc = svc.clone();
            std::thread::spawn(move || {
                let Ok(rd) = s.try_clone() else { return };
                serve_conn(s, rd, &svc);
            });
        }
    });
    Ok(())
}

/// Serve on 127.0.0.1 (Windows; the extension connects to "tcp:127.0.0.1:PORT"). Returns the port.
pub fn serve_tcp(svc: Arc<Service>) -> std::io::Result<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = l.local_addr()?.port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let svc = svc.clone();
            std::thread::spawn(move || {
                let Ok(rd) = s.try_clone() else { return };
                serve_conn(s, rd, &svc);
            });
        }
    });
    Ok(port)
}

/// Test builds only (GUI_PI_HUMAN_AUTO): the popup is really shown, then "clicked" by a script after delay_ms.
/// Rules: {"delay_ms": 1500, "approve": ["yes","no"], "answer": ["q3.ods"], "takeover": ["yes"], "ask_help": ["no:use Ctrl+S"], "default": "timeout"}
pub fn auto_outcome(rules: &Value, kind: &str, counter: &Mutex<std::collections::HashMap<String, usize>>) -> Outcome {
    let pick = |v: &Value, i: usize| -> String {
        match v {
            Value::Array(a) if !a.is_empty() => a[i.min(a.len() - 1)].as_str().unwrap_or("timeout").to_string(),
            Value::String(s) => s.clone(),
            _ => "timeout".into(),
        }
    };
    let mut c = counter.lock().unwrap_or_else(|e| e.into_inner());
    let i = *c.get(kind).unwrap_or(&0);
    c.insert(kind.into(), i + 1);
    let v = rules.get(kind).or(rules.get("default")).cloned().unwrap_or(json!("timeout"));
    let a = pick(&v, i);
    if a == "yes" {
        Outcome::Yes
    } else if let Some(n) = a.strip_prefix("no") {
        Outcome::No(n.trim_start_matches(':').to_string())
    } else if a == "close" {
        Outcome::Closed
    } else if a == "timeout" {
        Outcome::Timeout
    } else {
        Outcome::Text(a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_per_kind() {
        assert_eq!(to_elicit("approve", &Outcome::Yes)["content"]["decision"], "allow");
        assert_eq!(to_elicit("approve", &Outcome::No(String::new()))["content"]["decision"], "deny");
        assert_eq!(to_elicit("takeover", &Outcome::Yes)["content"]["done"], true);
        assert_eq!(to_elicit("ask_help", &Outcome::No("use F12".into()))["content"]["note"], "use F12");
        assert_eq!(to_elicit("answer", &Outcome::Text("a.ods".into()))["content"]["answer"], "a.ods");
        assert_eq!(to_elicit("answer", &Outcome::Text("  ".into()))["action"], "decline");
        assert_eq!(to_elicit("approve", &Outcome::Timeout)["action"], "decline");
        assert_eq!(to_elicit("approve", &Outcome::Timeout)["reason"], "timed_out");
        assert_eq!((to_elicit("takeover", &Outcome::Closed)["action"].as_str(), to_elicit("takeover", &Outcome::Closed)["reason"].as_str()), (Some("cancel"), Some("closed")));
        assert_eq!(to_elicit("approve", &Outcome::Cancelled)["reason"], "cancelled");
        assert_eq!(to_elicit("approve", &Outcome::Unavailable)["reason"], "unavailable");
        assert_eq!(to_elicit("approve", &Outcome::Text("allow".into()))["action"], "decline");
        // the timeline: every non-answer is a refusal, with its reason
        for (o, want) in [(Outcome::No(String::new()), "declined"), (Outcome::Closed, "window closed"), (Outcome::Timeout, "timed out"),
                          (Outcome::Cancelled, "cancelled"), (Outcome::Unavailable, "unavailable")] {
            assert_eq!(label("approve", &o), (want, true));
        }
        assert_eq!(label("approve", &Outcome::Yes), ("allowed", false));
    }

    #[test]
    fn one_answer_per_question() {
        let p = Pending::default();
        let rx = p.open("q1").unwrap();
        assert!(p.open("q1").is_none(), "a repeated id is never opened again");
        assert!(p.answer("q1", Outcome::Yes));
        assert!(!p.answer("q1", Outcome::No(String::new())), "second click ignored");
        assert_eq!(p.wait("q1", rx, 1.0), Outcome::Yes);
        assert!(!p.answer("q1", Outcome::Yes));
        // closing the window after an answer changes nothing; closing before one is the answer
        let rx = p.open("q2").unwrap();
        assert!(p.answer("q2", Outcome::Closed));
        assert!(!p.answer("q2", Outcome::Yes));
        assert_eq!(p.wait("q2", rx, 1.0), Outcome::Closed);
    }

    #[test]
    fn quitting_cancels_open_questions() {
        let p = Arc::new(Pending::default());
        let rx = p.open("q").unwrap();
        let p2 = p.clone();
        let h = std::thread::spawn(move || p2.wait("q", rx, 10.0));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(p.cancel_all(), 1);
        assert_eq!(h.join().unwrap(), Outcome::Cancelled);
        assert!(p.open_ids().is_empty());
        let rx = p.open("late").unwrap();
        assert_eq!(p.wait("late", rx, 0.05), Outcome::Timeout);
    }

    #[test]
    fn duplicate_request_ids_and_closing_service() {
        let shown = Arc::new(Mutex::new(0));
        let s2 = shown.clone();
        let popup: Popup = Arc::new(move |_, _| { *s2.lock().unwrap() += 1; Outcome::Yes });
        let svc = Service::new("t".into(), popup);
        let line = r#"{"token":"t","id":"7","kind":"approve","timeout_s":10}"#;
        assert_eq!(svc.handle_line(line)["content"]["decision"], "allow");
        let again = svc.handle_line(line);
        assert_eq!(again["action"], "decline");
        assert_eq!(*shown.lock().unwrap(), 1);
        assert_eq!(svc.handle_line(r#"{"token":"t","id":"","kind":"approve","timeout_s":10}"#)["action"], "decline");
        svc.set_closing();
        let r = svc.handle_line(r#"{"token":"t","id":"8","kind":"approve","timeout_s":10}"#);
        assert_eq!((r["action"].as_str(), r["reason"].as_str()), (Some("cancel"), Some("cancelled")));
        assert_eq!(*shown.lock().unwrap(), 1);
    }

    #[test]
    fn gate_env_is_handed_over_once_with_the_token() {
        let popup: Popup = Arc::new(|_, _| Outcome::Yes);
        let svc = Service::new("t".into(), popup);
        svc.stage_gate_env(vec![("GUI_GATE_REVIEWER_KEY".into(), "sk-rev".into())]);
        assert_eq!(svc.handle_line(r#"{"token":"bad","op":"gate_env"}"#)["env"], json!({}));
        assert_eq!(svc.handle_line(r#"{"token":"t","op":"gate_env"}"#)["env"]["GUI_GATE_REVIEWER_KEY"], "sk-rev");
        assert_eq!(svc.handle_line(r#"{"token":"t","op":"gate_env"}"#)["env"], json!({}));
        assert_eq!(svc.handle_line(r#"{"token":"t","op":"other","id":"1","kind":"approve"}"#)["action"], "decline");
    }

    #[test]
    fn wrong_token_or_kind_never_shows_a_popup() {
        let shown = Arc::new(Mutex::new(0));
        let s2 = shown.clone();
        let popup: Popup = Arc::new(move |_, _| { *s2.lock().unwrap() += 1; Outcome::Yes });
        let serial = Mutex::new(());
        let r = handle_line(r#"{"token":"bad","id":"1","kind":"approve","timeout_s":10}"#, "good", &popup, &serial);
        assert_eq!(r["action"], "decline");
        let r = handle_line(r#"{"token":"good","id":"2","kind":"rm","timeout_s":10}"#, "good", &popup, &serial);
        assert_eq!(r["action"], "decline");
        assert_eq!(*shown.lock().unwrap(), 0);
        let r = handle_line(r#"{"token":"good","id":"3","kind":"approve","timeout_s":10}"#, "good", &popup, &serial);
        assert_eq!((r["id"].as_str(), r["content"]["decision"].as_str()), (Some("3"), Some("allow")));
    }

    #[test]
    fn past_deadline_is_declined_without_a_popup() {
        let popup: Popup = Arc::new(|_, _| Outcome::Yes);
        let r = handle_line(r#"{"token":"t","id":"1","kind":"approve","deadline_ms":1000}"#, "t", &popup, &Mutex::new(()));
        assert_eq!(r["action"], "decline");
    }

    #[cfg(unix)]
    #[test]
    fn unix_socket_round_trip() {
        use std::os::unix::net::UnixStream;
        let path = std::env::temp_dir().join(format!("gpd-h-{}/h.sock", std::process::id()));
        let popup: Popup = Arc::new(|r, _| if r.kind == "takeover" { Outcome::Yes } else { Outcome::Timeout });
        serve_unix(&path, Service::new("tok".into(), popup)).unwrap();
        let mut s = UnixStream::connect(&path).unwrap();
        s.write_all(b"{\"token\":\"tok\",\"id\":\"9\",\"kind\":\"takeover\",\"timeout_s\":5}\n").unwrap();
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["content"]["done"], true);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn tcp_round_trip() {
        use std::net::TcpStream;
        let popup: Popup = Arc::new(|_, _| Outcome::Text("q3.ods".into()));
        let port = serve_tcp(Service::new("tok".into(), popup)).unwrap();
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(b"{\"token\":\"tok\",\"id\":\"1\",\"kind\":\"answer\",\"timeout_s\":5}\n").unwrap();
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).unwrap();
        assert!(line.contains("q3.ods"));
    }

    #[test]
    fn auto_rules() {
        let r = json!({"approve": ["yes", "no"], "ask_help": "no:use Ctrl+S", "answer": ["q3.ods"]});
        let c = Mutex::new(Default::default());
        assert_eq!(auto_outcome(&r, "approve", &c), Outcome::Yes);
        assert_eq!(auto_outcome(&r, "approve", &c), Outcome::No(String::new()));
        assert_eq!(auto_outcome(&r, "ask_help", &c), Outcome::No("use Ctrl+S".into()));
        assert_eq!(auto_outcome(&r, "answer", &c), Outcome::Text("q3.ods".into()));
        assert_eq!(auto_outcome(&r, "takeover", &c), Outcome::Timeout);
    }

    #[test]
    fn tokens_differ() {
        assert_ne!(new_token(), new_token());
        assert_eq!(new_token().len(), 32);
    }
}
