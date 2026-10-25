//! The ask-a-person service: the gui-pi extension connects, sends one request line, gets one reply line.
//!   request  {"token","id","kind","message","schema","timeout_s","deadline_ms","meta"}
//!   reply    {"id","action":"accept"|"decline"|"cancel","content":{...}}
//! The shell shows a native popup per request (one at a time) and maps the button to the kind's form.
//! A wrong token, an unknown kind, a closed popup or the deadline passing is never an approval.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
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
    Closed,
    Timeout,
}

/// Popup button -> the kind's ElicitResult.
pub fn to_elicit(kind: &str, o: &Outcome) -> Value {
    match (kind, o) {
        (_, Outcome::Closed) | (_, Outcome::Timeout) => json!({"action": "decline"}),
        ("approve", Outcome::Yes) => json!({"action": "accept", "content": {"decision": "allow"}}),
        ("approve", Outcome::No(_)) => json!({"action": "accept", "content": {"decision": "deny"}}),
        ("takeover", Outcome::Yes) => json!({"action": "accept", "content": {"done": true}}),
        ("takeover", Outcome::No(_)) => json!({"action": "accept", "content": {"done": false}}),
        ("ask_help", Outcome::Yes) => json!({"action": "accept", "content": {"choice": "did_it", "note": ""}}),
        ("ask_help", Outcome::No(n)) => json!({"action": "accept", "content": {"choice": "try_other", "note": n.chars().take(300).collect::<String>()}}),
        ("answer", Outcome::Text(t)) if !t.trim().is_empty() => json!({"action": "accept", "content": {"answer": t.chars().take(2000).collect::<String>()}}),
        _ => json!({"action": "decline"}),
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

/// Handle one connection's request line.
pub fn handle_line(line: &str, token: &str, popup: &Popup, serial: &Mutex<()>) -> Value {
    let req: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(_) => return json!({"id": "", "action": "decline"}),
    };
    let id = req.id.clone();
    if !ct_eq(&req.token, token) || !KINDS.contains(&req.kind.as_str()) {
        return json!({"id": id, "action": "decline"});
    }
    let _one_at_a_time = serial.lock().unwrap_or_else(|e| e.into_inner());
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    let left = seconds_left(&req, now_ms);
    if left < 1.0 {
        return json!({"id": id, "action": "decline"}); // waited in the queue past its deadline
    }
    let t0 = Instant::now();
    let mut o = popup(&req, left);
    if t0.elapsed() > Duration::from_secs_f64(left + 1.0) {
        o = Outcome::Timeout;
    }
    let mut r = to_elicit(&req.kind, &o);
    r["id"] = json!(id);
    r
}

fn serve_conn<S: std::io::Read + Write>(s: S, rd: impl std::io::Read, token: &str, popup: &Popup, serial: &Mutex<()>) {
    let mut line = String::new();
    if BufReader::new(rd).read_line(&mut line).is_ok() {
        let r = handle_line(line.trim(), token, popup, serial);
        let mut w = s;
        let _ = w.write_all(format!("{r}\n").as_bytes());
    }
}

/// Serve requests on a Unix socket (Linux / macOS); the socket file is 0600 in a 0700 directory.
#[cfg(unix)]
pub fn serve_unix(path: &std::path::Path, token: String, popup: Popup) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    let _ = std::fs::remove_file(path);
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
        std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700))?;
    }
    let l = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    let serial = Arc::new(Mutex::new(()));
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let (token, popup, serial) = (token.clone(), popup.clone(), serial.clone());
            std::thread::spawn(move || {
                let Ok(rd) = s.try_clone() else { return };
                serve_conn(s, rd, &token, &popup, &serial);
            });
        }
    });
    Ok(())
}

/// Serve on 127.0.0.1 (Windows; the extension connects to "tcp:127.0.0.1:PORT"). Returns the port.
pub fn serve_tcp(token: String, popup: Popup) -> std::io::Result<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = l.local_addr()?.port();
    let serial = Arc::new(Mutex::new(()));
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let (token, popup, serial) = (token.clone(), popup.clone(), serial.clone());
            std::thread::spawn(move || {
                let Ok(rd) = s.try_clone() else { return };
                serve_conn(s, rd, &token, &popup, &serial);
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
        assert_eq!(to_elicit("takeover", &Outcome::Closed)["action"], "decline");
        assert_eq!(to_elicit("approve", &Outcome::Text("allow".into()))["action"], "decline");
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
        serve_unix(&path, "tok".into(), popup).unwrap();
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
        let port = serve_tcp("tok".into(), popup).unwrap();
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
