//! The ask-a-person service: the gui-pi extension connects, sends one request line, gets one reply line.
//!   request  {"token","id","kind","message","schema","timeout_s","deadline_ms","meta"}
//!   reply    {"id","action":"accept"|"decline"|"cancel","content":{...},"reason"?}
//!   request  {"token","op":"gate_env"}  ->  {"env":{GUI_GATE_*...}}   once per task (one-shot), then {"env":{}}
//!   request  {"token","op":"watch_stop"}  ->  (held open) {"stop":true} when the user presses Stop
//!            -> the extension answers {"ack":"stopped","reset":bool} after blocking actions, gate_reset, killing the gate
//! The shell shows a native popup per request (one at a time) and maps the button to the kind's form.
//! Four strict result types share one popup component (`view`): approve {decision}, clarify {answer} (the gate's
//! "answer" form), takeover {done}, help {choice, note} (the gate's "ask_help" form). Per task the service records
//! what asking cost the person (`Metrics`) and writes it to the session log (`human.jsonl`) for bench/human_pilot.py.
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

pub const KINDS: &[&str] = &["approve", "answer", "takeover", "ask_help", "clarify", "help"];

/// The gate's form for a kind (clarify uses the answer form, help the ask_help form).
pub fn form_of(kind: &str) -> &'static str {
    match kind {
        "approve" => "approve",
        "answer" | "clarify" => "answer",
        "takeover" => "takeover",
        "ask_help" | "help" => "ask_help",
        _ => "",
    }
}

/// The result type the person sees: approve | clarify | takeover | help.
pub fn type_of(kind: &str) -> &'static str {
    match form_of(kind) {
        "approve" => "approve",
        "answer" => "clarify",
        "takeover" => "takeover",
        "ask_help" => "help",
        _ => "",
    }
}

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
    /// never shown: the question waited in the queue (another popup was open) until its deadline passed
    Expired,
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
        Outcome::Expired => Some("expired"),
        Outcome::Cancelled => Some("cancelled"),
        Outcome::Unavailable => Some("unavailable"),
        _ => None,
    }
}

/// For the main window's timeline: what happened to a question. Everything but a yes / an answer is a refusal.
pub fn label(kind: &str, o: &Outcome) -> (&'static str, bool) {
    match (form_of(kind), o) {
        ("approve", Outcome::Yes) => ("allowed", false),
        ("approve", Outcome::No(_)) => ("declined", true),
        ("takeover", Outcome::Yes) | ("ask_help", Outcome::Yes) => ("done by the person", false),
        ("takeover", Outcome::No(_)) => ("declined", true),
        ("ask_help", Outcome::No(_)) => ("try another way", false),
        ("answer", Outcome::Text(t)) if !t.trim().is_empty() => ("answered", false),
        (_, Outcome::Closed) => ("window closed", true),
        (_, Outcome::Timeout) => ("timed out", true),
        (_, Outcome::Expired) => ("expired", true),
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
    match (form_of(kind), o) {
        (_, Outcome::Closed) => refusal("cancel", "closed"),
        (_, Outcome::Cancelled) => refusal("cancel", "cancelled"),
        (_, Outcome::Timeout) => refusal("decline", "timed_out"),
        (_, Outcome::Expired) => refusal("decline", "expired"),
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

fn strs(v: Option<&Value>, max: usize) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a.iter().filter_map(|x| match x {
            Value::String(s) => Some(s.clone()),
            Value::Object(o) => o.get("text").or(o.get("label")).or(o.get("method")).or(o.get("step")).and_then(|t| t.as_str()).map(String::from),
            _ => None,
        }).filter(|s| !s.trim().is_empty()).take(max).map(|s| s.chars().take(200).collect()).collect(),
        Some(Value::String(s)) if !s.trim().is_empty() => vec![s.chars().take(200).collect()],
        _ => vec![],
    }
}

fn s_of(m: &Value, keys: &[&str]) -> String {
    for k in keys {
        match m.get(*k) {
            Some(Value::String(s)) if !s.trim().is_empty() => return s.chars().take(300).collect(),
            Some(v @ Value::Array(_)) => {
                let l = strs(Some(v), 6);
                if !l.is_empty() {
                    return l.join(", ");
                }
            }
            _ => {}
        }
    }
    String::new()
}

/// What the one shared popup component shows, per result type, from the gate's request (message + `_meta`).
/// Field names the gate may use are read tolerantly; what is missing is left empty, never invented.
///   approve   object (what exactly), scope (how much / where), consequence (what happens on Allow)
///   clarify   the question, options {label, value, effect: how this answer changes what happens next}, free text
///   takeover  the minimal steps the person must do; the agent's input is paused; Done never re-runs the action
///   help      what the agent tried; "I did it for you" or "Give a hint"
pub fn view(req: &Request) -> Value {
    let m = if req.meta.is_object() { req.meta.clone() } else { json!({}) };
    let ty = type_of(&req.kind);
    let effect = m.get("effect").cloned().unwrap_or(Value::Null);
    let mut v = json!({"type": ty, "kind": req.kind, "message": req.message.chars().take(2000).collect::<String>()});
    match ty {
        "approve" => {
            let object = {
                let o = s_of(&m, &["object", "target"]);
                if o.is_empty() { s_of(&effect, &["resources"]) } else { o }
            };
            let scope = {
                let o = s_of(&m, &["scope"]);
                if !o.is_empty() { o } else {
                    let r = strs(effect.get("resources"), 20);
                    let app = s_of(&effect, &["app", "window"]);
                    let mut parts = vec![];
                    if r.len() > 1 { parts.push(format!("{} items", r.len())); }
                    if !app.is_empty() { parts.push(format!("in {app}")); }
                    parts.join(" ")
                }
            };
            let consequence = {
                let o = s_of(&m, &["consequence"]);
                if !o.is_empty() { o } else {
                    let hz = s_of(&effect, &["hazard"]);
                    let tool = s_of(&m, &["tool"]);
                    if hz.is_empty() && tool.is_empty() { String::new() } else {
                        format!("Allow runs this {}{} step now.", tool.replace('_', " "), if hz.is_empty() { String::new() } else { format!(" ({hz})") })
                    }
                }
            };
            v["object"] = json!(object);
            v["scope"] = json!(scope);
            v["consequence"] = json!(consequence);
            v["reason"] = json!(s_of(&m, &["reason"]));
            v["buttons"] = json!([{"choice": "no", "label": "Deny"}, {"choice": "yes", "label": "Allow"}]);
        }
        "clarify" => {
            let opts: Vec<Value> = match m.get("options") {
                Some(Value::Array(a)) => a.iter().take(8).filter_map(|o| match o {
                    Value::String(s) => Some(json!({"label": s, "value": s, "effect": ""})),
                    Value::Object(x) => {
                        let label = x.get("label").or(x.get("text")).or(x.get("value")).and_then(|t| t.as_str())?;
                        let value = x.get("value").and_then(|t| t.as_str()).unwrap_or(label);
                        let eff = x.get("effect").or(x.get("consequence")).or(x.get("then")).and_then(|t| t.as_str()).unwrap_or("");
                        Some(json!({"label": label.chars().take(120).collect::<String>(), "value": value.chars().take(500).collect::<String>(),
                                    "effect": eff.chars().take(240).collect::<String>()}))
                    }
                    _ => None,
                }).collect(),
                _ => vec![],
            };
            v["free_text"] = json!(opts.is_empty() || m.get("allow_other").and_then(|b| b.as_bool()).unwrap_or(true));
            v["options"] = json!(opts);
            v["buttons"] = json!([{"choice": "no", "label": "Don't answer"}]);
        }
        "takeover" => {
            v["steps"] = json!(strs(m.get("steps").or(m.get("todo")), 8));
            v["paused"] = json!("The agent's input is paused until you answer.");
            v["note"] = json!("Done tells the agent you did it; it will not repeat the step.");
            v["buttons"] = json!([{"choice": "no", "label": "I won't do it"}, {"choice": "yes", "label": "Done"}]);
        }
        "help" => {
            v["tried"] = json!(strs(m.get("tried").or(m.get("tried_methods")).or(m.get("attempts")), 6));
            v["reason"] = json!(s_of(&m, &["reason"]));
            v["hint"] = json!(true);
            v["buttons"] = json!([{"choice": "no", "label": "Give a hint"}, {"choice": "yes", "label": "I did it for you"}]);
        }
        _ => {}
    }
    v
}

/// One question as the pilot log records it.
#[derive(Clone, Debug, Serialize)]
pub struct QuestionRec {
    pub ty: String,
    pub outcome: String,
    pub refused: bool,
    pub secs: f64,
    pub repeated: bool,
    pub takeover_steps: usize,
}

/// Per task: what asking cost the person. Nothing of this is shown beyond the brief timeline entry per question.
///   person_active_s  popup up until the person answered (read, decide, do the takeover / help step)
///   person_wait_s    task wall time with no question open (the person only watches the agent)
///   agent_blocked_s  every second a question was open, answered or not
pub struct Metrics {
    pub task_id: String,
    started: Instant,
    seen: HashSet<String>,
    pub recs: Vec<QuestionRec>,
}

impl Metrics {
    pub fn new(task_id: &str) -> Metrics {
        Metrics { task_id: task_id.into(), started: Instant::now(), seen: HashSet::new(), recs: vec![] }
    }
    fn key(ty: &str, msg: &str) -> String {
        let m: String = msg.to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect();
        format!("{ty}:{}", m.split_whitespace().collect::<Vec<_>>().join(" "))
    }
    pub fn record(&mut self, kind: &str, message: &str, meta: &Value, o: &Outcome, secs: f64) -> QuestionRec {
        let ty = type_of(kind).to_string();
        let repeated = !self.seen.insert(Metrics::key(&ty, message));
        let (word, refused) = label(kind, o);
        let takeover_steps = if ty == "takeover" { strs(meta.get("steps").or(meta.get("todo")), 50).len().max(1) } else { 0 };
        let r = QuestionRec { ty, outcome: word.into(), refused, secs: (secs * 10.0).round() / 10.0, repeated, takeover_steps };
        self.recs.push(r.clone());
        r
    }
    pub fn summary(&self, end: &str) -> Value {
        let total = self.started.elapsed().as_secs_f64();
        let open: f64 = self.recs.iter().map(|r| r.secs).sum();
        let active: f64 = self.recs.iter().filter(|r| !matches!(r.outcome.as_str(), "timed out" | "expired" | "cancelled" | "unavailable")).map(|r| r.secs).sum();
        let count = |w: &str| self.recs.iter().filter(|r| r.outcome == w).count();
        let mut by_type = serde_json::Map::new();
        for t in ["approve", "clarify", "takeover", "help"] {
            by_type.insert(t.into(), json!(self.recs.iter().filter(|r| r.ty == t).count()));
        }
        let r1 = |x: f64| (x * 10.0).round() / 10.0;
        json!({"ev": "human_summary", "schema": "gui-pi-human/1", "task": self.task_id, "end": end,
               "asks": self.recs.len(), "by_type": by_type,
               "repeated": self.recs.iter().filter(|r| r.repeated).count(),
               "repeated_ratio": if self.recs.is_empty() { 0.0 } else { r1(self.recs.iter().filter(|r| r.repeated).count() as f64 / self.recs.len() as f64 * 100.0) / 100.0 },
               "person_active_s": r1(active), "person_wait_s": r1((total - open).max(0.0)), "agent_blocked_s": r1(open),
               "takeover_steps": self.recs.iter().map(|r| r.takeover_steps).sum::<usize>(),
               "cancelled": count("cancelled") + count("window closed"), "timed_out": count("timed out"), "expired": count("expired"),
               "declined": count("declined"), "unavailable": count("unavailable"), "task_s": r1(total)})
    }
}

pub type LogSink = Box<dyn Fn(&Value) + Send + Sync>;
/// One Stop request to a watching extension; it answers Some(reset) after its ack, None without one.
type StopReq = Sender<Option<bool>>;

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
    metrics: Mutex<Option<Metrics>>,
    log: Mutex<Option<LogSink>>,
    /// the main window's timeline, for questions that never reached a popup (expired in the queue)
    notify: Mutex<Option<LogSink>>,
    watchers: Mutex<Vec<Sender<StopReq>>>,
}

impl Service {
    pub fn new(token: String, popup: Popup) -> Arc<Service> {
        Arc::new(Service { token, popup, serial: Mutex::new(()), seen: Mutex::new(HashSet::new()), gate_env: Mutex::new(None),
                           closing: AtomicBool::new(false), metrics: Mutex::new(None), log: Mutex::new(None), notify: Mutex::new(None),
                           watchers: Mutex::new(vec![]) })
    }
    /// Where per-question and per-task records go (the shell: logs/human.jsonl).
    pub fn set_log(&self, sink: LogSink) {
        *self.log.lock().unwrap_or_else(|e| e.into_inner()) = Some(sink);
    }
    /// Where a question that never got a popup is reported (the shell: the main window's timeline).
    pub fn set_notify(&self, sink: LogSink) {
        *self.notify.lock().unwrap_or_else(|e| e.into_inner()) = Some(sink);
    }
    fn notify(&self, v: &Value) {
        if let Some(f) = self.notify.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            f(v);
        }
    }
    fn write_log(&self, v: &Value) {
        if let Some(f) = self.log.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            f(v);
        }
    }
    /// A task starts: fresh metrics; stop watchers of an earlier task are dropped.
    pub fn begin_task(&self, task_id: &str) {
        *self.metrics.lock().unwrap_or_else(|e| e.into_inner()) = Some(Metrics::new(task_id));
        self.watchers.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
    /// The task ended (`end`: finished / stopped / crashed / ...): the summary goes to the log and is returned.
    pub fn end_task(&self, end: &str) -> Option<Value> {
        let m = self.metrics.lock().unwrap_or_else(|e| e.into_inner()).take()?;
        let v = m.summary(end);
        self.write_log(&v);
        Some(v)
    }
    /// Stop: tell every watching extension, wait up to `wait` for the acks. Some(reset) when one acked.
    pub fn signal_stop(&self, wait: Duration) -> Option<bool> {
        let ws: Vec<Sender<StopReq>> = self.watchers.lock().unwrap_or_else(|e| e.into_inner()).drain(..).collect();
        let mut acks = vec![];
        for w in ws {
            let (tx, rx) = channel();
            if w.send(tx).is_ok() {
                acks.push(rx);
            }
        }
        let deadline = Instant::now() + wait;
        let mut out = None;
        for rx in acks {
            let left = deadline.saturating_duration_since(Instant::now());
            if let Ok(Some(reset)) = rx.recv_timeout(left) {
                out = Some(out.unwrap_or(false) || reset);
            }
        }
        out
    }
    /// A `watch_stop` line with the right token registers a watcher; None for anything else.
    pub fn watch(&self, line: &str) -> Option<Receiver<StopReq>> {
        let req: Request = serde_json::from_str(line).ok()?;
        if req.op != "watch_stop" || !ct_eq(&req.token, &self.token) || self.closing.load(Ordering::SeqCst) {
            return None;
        }
        let (tx, rx) = channel();
        self.watchers.lock().unwrap_or_else(|e| e.into_inner()).push(tx);
        Some(rx)
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
            // waited in the queue (another question was open) past its deadline: never shown. Recorded like any
            // other question (B8: 0.3.0 dropped it from human.jsonl) and answered as "no answer: expired".
            let o = Outcome::Expired;
            let rec = self.metrics.lock().unwrap_or_else(|e| e.into_inner()).as_mut()
                .map(|m| (m.task_id.clone(), m.record(&req.kind, &req.message, &req.meta, &o, 0.0)));
            if let Some((task, r)) = rec {
                self.write_log(&json!({"ev": "human_q", "schema": "gui-pi-human/1", "task": task, "type": r.ty, "outcome": r.outcome,
                                       "refused": r.refused, "secs": r.secs, "repeated": r.repeated, "takeover_steps": r.takeover_steps,
                                       "shown": false}));
            }
            self.notify(&json!({"kind": req.kind, "message": req.message, "outcome": "expired", "refused": true, "reason": "expired"}));
            let mut r = to_elicit(&req.kind, &o);
            r["id"] = json!(id);
            return r;
        }
        let t0 = Instant::now();
        let mut o = (self.popup)(&req, left);
        if t0.elapsed() > Duration::from_secs_f64(left + 1.0) && !matches!(o, Outcome::Cancelled) {
            o = Outcome::Timeout;
        }
        let rec = self.metrics.lock().unwrap_or_else(|e| e.into_inner()).as_mut()
            .map(|m| (m.task_id.clone(), m.record(&req.kind, &req.message, &req.meta, &o, t0.elapsed().as_secs_f64())));
        if let Some((task, r)) = rec {
            self.write_log(&json!({"ev": "human_q", "schema": "gui-pi-human/1", "task": task, "type": r.ty, "outcome": r.outcome,
                                   "refused": r.refused, "secs": r.secs, "repeated": r.repeated, "takeover_steps": r.takeover_steps}));
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
    let mut br = BufReader::new(rd);
    let mut line = String::new();
    if br.read_line(&mut line).is_err() {
        return;
    }
    let mut w = s;
    if let Some(rx) = svc.watch(line.trim()) {
        // held open until Stop (or until the service drops this watcher at the next task)
        if let Ok(ack) = rx.recv() {
            let _ = w.write_all(b"{\"stop\":true}\n");
            let _ = w.flush();
            let mut l2 = String::new();
            let reset = match br.read_line(&mut l2) {
                Ok(n) if n > 0 => serde_json::from_str::<Value>(l2.trim()).ok()
                    .filter(|v| v.get("ack").is_some()).map(|v| v.get("reset").and_then(|b| b.as_bool()).unwrap_or(false)),
                _ => None,
            };
            let _ = ack.send(reset);
        }
        return;
    }
    let r = svc.handle_line(line.trim());
    let _ = w.write_all(format!("{r}\n").as_bytes());
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
    fn four_result_types_one_view() {
        let req = |kind: &str, meta: Value| Request { kind: kind.into(), message: "m".into(), meta, ..Default::default() };
        // approve: the gate-confirmed object, scope, consequence
        let v = view(&req("approve", json!({"tool": "run_command", "target": "rm -rf ~/Documents/x", "reason": "high risk",
            "effect": {"tool": "run_command", "hazard": "delete", "resources": ["/home/u/Documents/x", "/home/u/Documents/y"], "app": "Terminal"}})));
        assert_eq!(v["type"], "approve");
        assert_eq!(v["object"], "rm -rf ~/Documents/x");
        assert_eq!(v["scope"], "2 items in Terminal");
        assert!(v["consequence"].as_str().unwrap().contains("delete"));
        let v = view(&req("approve", json!({"object": "q3.ods", "scope": "1 file", "consequence": "q3.ods is overwritten"})));
        assert_eq!((v["object"].as_str(), v["scope"].as_str(), v["consequence"].as_str()), (Some("q3.ods"), Some("1 file"), Some("q3.ods is overwritten")));
        // clarify (also the gate's older "answer"): options and what each changes
        for k in ["clarify", "answer"] {
            let v = view(&req(k, json!({"options": [{"label": "Overwrite", "value": "overwrite", "effect": "the old file is replaced"}, "Save as copy"]})));
            assert_eq!(v["type"], "clarify");
            assert_eq!(v["options"][0]["effect"], "the old file is replaced");
            assert_eq!(v["options"][1]["value"], "Save as copy");
        }
        assert_eq!(view(&req("answer", json!({})))["free_text"], true);
        // takeover: minimal steps, input paused, done does not re-run
        let v = view(&req("takeover", json!({"steps": ["Type your password", "Press Log in"]})));
        assert_eq!(v["steps"].as_array().unwrap().len(), 2);
        assert!(v["paused"].as_str().unwrap().contains("paused") && v["note"].as_str().unwrap().contains("will not repeat"));
        // help (also the gate's "ask_help"): what was tried; do it for me / give a hint
        let v = view(&req("ask_help", json!({"tried": [{"method": "click Save"}, "ctrl+s"]})));
        assert_eq!((v["type"].as_str(), v["tried"][0].as_str(), v["tried"][1].as_str()), (Some("help"), Some("click Save"), Some("ctrl+s")));
        let labels: Vec<&str> = v["buttons"].as_array().unwrap().iter().map(|b| b["label"].as_str().unwrap()).collect();
        assert_eq!(labels, vec!["Give a hint", "I did it for you"]);
        // strict results per type: the forms never mix
        assert_eq!(to_elicit("clarify", &Outcome::Text("overwrite".into()))["content"], json!({"answer": "overwrite"}));
        assert_eq!(to_elicit("help", &Outcome::No("use the menu".into()))["content"], json!({"choice": "try_other", "note": "use the menu"}));
        assert_eq!(to_elicit("help", &Outcome::Yes)["content"]["choice"], "did_it");
        assert_eq!(to_elicit("clarify", &Outcome::Yes)["action"], "decline", "a yes is not an answer");
        assert_eq!(to_elicit("approve", &Outcome::Text("allow".into()))["action"], "decline", "an answer is not an approval");
        assert_eq!(to_elicit("takeover", &Outcome::Yes)["content"], json!({"done": true}));
    }

    #[test]
    fn metrics_go_to_the_log_not_the_screen() {
        let lines = Arc::new(Mutex::new(Vec::<Value>::new()));
        let l2 = lines.clone();
        let answers = Arc::new(Mutex::new(vec![Outcome::Yes, Outcome::Timeout, Outcome::Yes, Outcome::Closed, Outcome::Text("a.ods".into())]));
        let a2 = answers.clone();
        let popup: Popup = Arc::new(move |_, _| { std::thread::sleep(Duration::from_millis(20)); a2.lock().unwrap().remove(0) });
        let svc = Service::new("t".into(), popup);
        svc.set_log(Box::new(move |v| l2.lock().unwrap().push(v.clone())));
        svc.begin_task("task-7");
        for (i, (k, msg, meta)) in [("approve", "Delete x?", json!({})), ("approve", "Delete  x?", json!({})),
                                    ("takeover", "Log in", json!({"steps": ["type password", "press Log in"]})),
                                    ("help", "Stuck", json!({})), ("clarify", "Which file?", json!({}))].iter().enumerate() {
            let line = json!({"token": "t", "id": format!("q{i}"), "kind": k, "message": msg, "timeout_s": 30, "meta": meta}).to_string();
            svc.handle_line(&line);
        }
        let s = svc.end_task("finished").unwrap();
        assert_eq!(s["asks"], 5);
        assert_eq!(s["repeated"], 1, "the same approve question twice");
        assert_eq!(s["takeover_steps"], 2);
        assert_eq!((s["timed_out"].as_u64(), s["cancelled"].as_u64()), (Some(1), Some(1)));
        assert_eq!(s["by_type"]["clarify"], 1);
        assert!(s["person_active_s"].as_f64().unwrap() >= 0.0 && s["agent_blocked_s"].as_f64().unwrap() >= s["person_active_s"].as_f64().unwrap());
        let l = lines.lock().unwrap();
        assert_eq!(l.iter().filter(|v| v["ev"] == "human_q").count(), 5);
        assert_eq!(l.last().unwrap()["ev"], "human_summary");
        assert!(l.iter().all(|v| v["schema"] == "gui-pi-human/1" && v["task"] == "task-7"));
        assert!(svc.end_task("again").is_none());
    }

    #[test]
    fn a_question_that_expires_in_the_queue_is_recorded_and_reported() {
        // B8: while one popup is open, a second question's deadline passes; it was never shown
        let lines = Arc::new(Mutex::new(Vec::<Value>::new()));
        let shown = Arc::new(Mutex::new(Vec::<Value>::new()));
        let (l2, s2) = (lines.clone(), shown.clone());
        let popup: Popup = Arc::new(move |_, _| { std::thread::sleep(Duration::from_millis(1500)); Outcome::Yes });
        let svc = Service::new("t".into(), popup);
        svc.set_log(Box::new(move |v| l2.lock().unwrap().push(v.clone())));
        svc.set_notify(Box::new(move |v| s2.lock().unwrap().push(v.clone())));
        svc.begin_task("task-8");
        let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
        let a = svc.clone();
        let first = std::thread::spawn(move || a.handle_line(&json!({"token": "t", "id": "q1", "kind": "takeover", "message": "Log in", "timeout_s": 30}).to_string()));
        std::thread::sleep(Duration::from_millis(100));
        let r = svc.handle_line(&json!({"token": "t", "id": "q2", "kind": "help", "message": "Stuck", "deadline_ms": now_ms + 900}).to_string());
        assert_eq!((r["action"].as_str(), r["reason"].as_str()), (Some("decline"), Some("expired")));
        assert_eq!(first.join().unwrap()["content"]["done"], true);
        let s = svc.end_task("finished").unwrap();
        assert_eq!((s["asks"].as_u64(), s["expired"].as_u64()), (Some(2), Some(1)));
        let l = lines.lock().unwrap();
        let q: Vec<&Value> = l.iter().filter(|v| v["ev"] == "human_q").collect();
        assert_eq!(q.len(), 2);
        let e = q.iter().find(|v| v["type"] == "help").unwrap();
        assert_eq!((e["outcome"].as_str(), e["refused"].as_bool(), e["shown"].as_bool()), (Some("expired"), Some(true), Some(false)));
        let n = shown.lock().unwrap();
        assert_eq!((n.len(), n[0]["outcome"].as_str()), (1, Some("expired")));
    }

    #[cfg(unix)]
    #[test]
    fn stop_reaches_the_watching_extension() {
        use std::os::unix::net::UnixStream;
        let path = std::env::temp_dir().join(format!("gpd-w-{}/h.sock", std::process::id()));
        let svc = Service::new("tok".into(), Arc::new(|_, _| Outcome::Yes));
        serve_unix(&path, svc.clone()).unwrap();
        svc.begin_task("t1");
        // a wrong token never becomes a watcher
        let mut bad = UnixStream::connect(&path).unwrap();
        bad.write_all(b"{\"token\":\"no\",\"op\":\"watch_stop\"}\n").unwrap();
        let mut s = UnixStream::connect(&path).unwrap();
        s.write_all(b"{\"token\":\"tok\",\"op\":\"watch_stop\",\"id\":\"stop\"}\n").unwrap();
        let s2 = s.try_clone().unwrap();
        let h = std::thread::spawn(move || {
            let mut l = String::new();
            BufReader::new(s2).read_line(&mut l).unwrap();
            assert_eq!(l.trim(), "{\"stop\":true}");
            let mut s = s;
            s.write_all(b"{\"ack\":\"stopped\",\"reset\":true}\n").unwrap();
        });
        let t0 = Instant::now();
        while svc.watchers.lock().unwrap().len() < 1 && t0.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(svc.watchers.lock().unwrap().len(), 1);
        assert_eq!(svc.signal_stop(Duration::from_secs(3)), Some(true));
        h.join().unwrap();
        // nobody watching (or no ack in time): None, and Stop goes on (the shell kills the process tree)
        assert_eq!(svc.signal_stop(Duration::from_millis(100)), None);
    }

    #[test]
    fn tokens_differ() {
        assert_ne!(new_token(), new_token());
        assert_eq!(new_token().len(), 32);
    }
}
