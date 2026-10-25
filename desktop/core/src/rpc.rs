//! Pi RPC (JSONL over stdio): the commands the shell sends and a compact view of the events for the main window.
use serde::Serialize;
use serde_json::{json, Value};

pub fn prompt(id: &str, message: &str) -> String {
    json!({"id": id, "type": "prompt", "message": message}).to_string()
}

pub fn abort() -> String {
    json!({"type": "abort"}).to_string()
}

/// Answer an extension dialog the shell does not show (person questions go through the ask-a-person IPC, never
/// through Pi's own dialogs): cancelled, which the extension treats as a refusal.
pub fn ui_cancel(id: &str) -> String {
    json!({"type": "extension_ui_response", "id": id, "cancelled": true}).to_string()
}

/// Dialog methods that block Pi until answered (rpc-extension-ui.md).
pub const DIALOG_METHODS: &[&str] = &["select", "confirm", "input", "editor"];

/// The marker the extension writes when gui-gate exits (ui.notify and stderr).
pub const GATE_EXIT_MARK: &str = "[gui-pi] gui-gate exited";

/// Split RPC records on LF only (Pi's framing rule), stripping one trailing CR.
pub fn split_records(buf: &mut String) -> Vec<String> {
    let mut out = vec![];
    while let Some(i) = buf.find('\n') {
        let mut line: String = buf.drain(..=i).collect();
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
        if !line.trim().is_empty() {
            out.push(line);
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct UiEvent {
    /// "tool" | "gate" | "result" | "text" | "done" | "error" | "ui_request" | "notice" | "gate_exit" | "human" |
    /// "crashed" | "stopped"
    pub kind: String,
    pub text: String,
    /// a screenshot the tool returned (base64), for the thumbnail
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// the gate's status for a tool result (executed / blocked / partial / unknown / input_required / done)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// a person's outcome when it was a refusal (declined / cancelled / timed_out / closed / unavailable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human: Option<String>,
    /// extension dialog id the shell must answer (ui_request)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// the main window offers "Restart" (crashed)
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub restart: bool,
}

impl UiEvent {
    pub fn new(kind: &str, text: impl Into<String>) -> UiEvent {
        UiEvent { kind: kind.into(), text: text.into(), ..Default::default() }
    }
}

fn content_text(c: &Value) -> String {
    c.as_array().map(|a| a.iter().filter_map(|x| x.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("\n")).unwrap_or_default()
}

fn first_image(c: &Value) -> Option<String> {
    c.as_array()?.iter().find(|x| x.get("type").and_then(|t| t.as_str()) == Some("image"))
        .and_then(|x| x.get("data").and_then(|d| d.as_str())).map(String::from)
}

fn human_word(h: &str) -> &'static str {
    match h {
        "declined" => "declined",
        "cancelled" => "cancelled",
        "timed_out" => "timed out",
        "closed" => "window closed",
        "unavailable" => "unavailable",
        _ => "refused",
    }
}

/// One Pi event -> what the main window shows (None: not shown).
pub fn ui_event(line: &str) -> Option<UiEvent> {
    let v: Value = serde_json::from_str(line).ok()?;
    let t = v.get("type")?.as_str()?;
    let ev = |kind: &str, text: String, image: Option<String>| Some(UiEvent { kind: kind.into(), text, image, ..Default::default() });
    match t {
        "tool_execution_start" => {
            let name = v.get("toolName").and_then(|n| n.as_str()).unwrap_or("?");
            let args = v.get("args").map(|a| a.to_string()).unwrap_or_default();
            ev("tool", format!("{name} {}", args.chars().take(200).collect::<String>()), None)
        }
        "tool_execution_end" => {
            let r = v.get("result").cloned().unwrap_or(json!({}));
            let empty = json!([]);
            let c = r.get("content").unwrap_or(&empty);
            let text = content_text(c);
            let first = text.lines().next().unwrap_or("").chars().take(300).collect::<String>();
            let d = r.get("details").cloned().unwrap_or(json!({}));
            let status = d.get("status").and_then(|s| s.as_str()).map(String::from);
            let human = d.get("human_outcome").and_then(|s| s.as_str()).map(String::from);
            let gate_kind = matches!(status.as_deref(), Some("blocked") | Some("partial") | Some("unknown") | Some("input_required"))
                || human.is_some() || first.starts_with("[gate]") || first.starts_with("[human via");
            let mut shown = first.clone();
            match status.as_deref() {
                Some("unknown") => shown = format!("outcome unknown - the agent must re-read before acting. {first}"),
                Some("partial") => {
                    let ran = d.pointer("/steps/ran").and_then(|x| x.as_u64());
                    let total = d.pointer("/steps/total").and_then(|x| x.as_u64());
                    if let Some(ran) = ran {
                        shown = format!("batch stopped after {ran}{} steps. {first}", total.map(|t| format!(" of {t}")).unwrap_or_default());
                    }
                }
                _ => {}
            }
            if let Some(h) = &human {
                shown = format!("person: {} (refused). {shown}", human_word(h));
            }
            Some(UiEvent { kind: if gate_kind { "gate".into() } else { "result".into() }, text: shown, image: first_image(c), status, human, ..Default::default() })
        }
        "message_end" => {
            let m = v.get("message")?;
            if m.get("role").and_then(|r| r.as_str()) != Some("assistant") {
                return None;
            }
            // a failed model call (401, quota, network...) ends the turn with stopReason "error": show it as an error,
            // never as a normal finish (0.3.0 showed only "finished.")
            if m.get("stopReason").and_then(|r| r.as_str()) == Some("error") {
                let e = m.get("errorMessage").and_then(|e| e.as_str()).unwrap_or("unknown error");
                return ev("error", format!("model request failed: {}", e.chars().take(400).collect::<String>()), None);
            }
            let text = m.get("content").and_then(|c| c.as_array()).map(|a| a.iter()
                .filter(|x| x.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|x| x.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("\n")).unwrap_or_default();
            if text.trim().is_empty() { None } else { ev("text", text, None) }
        }
        // agent_end is one low-level run; Pi may still retry or compact. agent_settled is the end (rpc.md).
        "agent_settled" => ev("done", if v.get("aborted") == Some(&json!(true)) { "aborted".into() } else { String::new() }, None),
        "auto_retry_end" if v.get("success") == Some(&json!(false)) =>
            ev("error", format!("model request failed: {}", v.get("finalError").and_then(|e| e.as_str()).unwrap_or("?")), None),
        "extension_ui_request" => {
            let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let id = v.get("id").and_then(|i| i.as_str()).map(String::from);
            let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string();
            if DIALOG_METHODS.contains(&method) {
                return Some(UiEvent { kind: "ui_request".into(), text: method.into(), id, ..Default::default() });
            }
            if method == "notify" {
                if msg.starts_with(GATE_EXIT_MARK) {
                    return ev("gate_exit", msg, None);
                }
                return ev("notice", msg, None);
            }
            None
        }
        "response" if v.get("success") == Some(&json!(false)) => ev("error", v.get("error").map(|e| e.to_string()).unwrap_or_default(), None),
        _ => None,
    }
}

/// A line of the agent's stderr worth showing (the extension's gate-exit report); None otherwise.
pub fn stderr_event(line: &str) -> Option<UiEvent> {
    let i = line.find(GATE_EXIT_MARK)?;
    Some(UiEvent::new("gate_exit", line[i..].chars().take(400).collect::<String>()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_is_lf_only() {
        let mut b = String::from("{\"a\":\"x\u{2028}y\"}\r\n{\"b\":1}\npartial");
        let r = split_records(&mut b);
        assert_eq!(r.len(), 2);
        assert!(r[0].contains('\u{2028}'));
        assert_eq!(b, "partial");
    }
    #[test]
    fn events() {
        let e = ui_event(r#"{"type":"tool_execution_end","toolName":"click","result":{"content":[{"type":"text","text":"[gate] Not executed: risky"},{"type":"image","data":"AAAA"}]}}"#).unwrap();
        assert_eq!(e.kind, "gate");
        assert_eq!(e.image.as_deref(), Some("AAAA"));
        assert!(ui_event(r#"{"type":"agent_end","messages":[]}"#).is_none(), "agent_end is not the end");
        assert_eq!(ui_event(r#"{"type":"agent_settled","aborted":false}"#).unwrap().kind, "done");
        assert!(ui_event(r#"{"type":"turn_start"}"#).is_none());
        let m = ui_event(r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"Done: saved q3.ods"}]}}"#).unwrap();
        assert_eq!(m.text, "Done: saved q3.ods");
        assert!(prompt("1", "hi").contains("\"type\":\"prompt\""));
        // B2: a model error is an error in the timeline, not an empty / normal assistant message
        let e = ui_event(r#"{"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":"401 UNAUTHORIZED"}}"#).unwrap();
        assert_eq!(e.kind, "error");
        assert!(e.text.contains("401 UNAUTHORIZED"), "{}", e.text);
    }

    #[test]
    fn statuses_and_person_outcomes_in_the_timeline() {
        let e = ui_event(r#"{"type":"tool_execution_end","toolName":"click","result":{"content":[{"type":"text","text":"[gate] Not executed: risky"}],"details":{"status":"blocked","human_outcome":"timed_out"}}}"#).unwrap();
        assert_eq!((e.kind.as_str(), e.human.as_deref()), ("gate", Some("timed_out")));
        assert!(e.text.starts_with("person: timed out (refused)"), "{}", e.text);
        for (h, w) in [("declined", "declined"), ("cancelled", "cancelled"), ("closed", "window closed"), ("unavailable", "unavailable")] {
            let line = format!(r#"{{"type":"tool_execution_end","result":{{"content":[],"details":{{"status":"blocked","human_outcome":"{h}"}}}}}}"#);
            assert!(ui_event(&line).unwrap().text.contains(w));
        }
        let e = ui_event(r#"{"type":"tool_execution_end","result":{"content":[{"type":"text","text":"x"}],"details":{"status":"unknown"}}}"#).unwrap();
        assert!(e.kind == "gate" && e.text.starts_with("outcome unknown"));
        let e = ui_event(r#"{"type":"tool_execution_end","result":{"content":[{"type":"text","text":"x"}],"details":{"status":"partial","steps":{"ran":2,"total":5}}}}"#).unwrap();
        assert!(e.text.starts_with("batch stopped after 2 of 5 steps"));
        let e = ui_event(r#"{"type":"tool_execution_end","result":{"content":[{"type":"text","text":"Clicked"}],"details":{"status":"executed"}}}"#).unwrap();
        assert_eq!(e.kind, "result");
    }

    #[test]
    fn gate_exit_and_dialogs() {
        let e = ui_event(r#"{"type":"extension_ui_request","id":"u1","method":"notify","message":"[gui-pi] gui-gate exited (code 101); no GUI action can run.","notifyType":"error"}"#).unwrap();
        assert_eq!(e.kind, "gate_exit");
        assert_eq!(stderr_event("node: [gui-pi] gui-gate exited (code 9)").unwrap().kind, "gate_exit");
        assert!(stderr_event("some other noise").is_none());
        let d = ui_event(r#"{"type":"extension_ui_request","id":"u2","method":"select","title":"x","options":["a"]}"#).unwrap();
        assert_eq!((d.kind.as_str(), d.id.as_deref()), ("ui_request", Some("u2")));
        let v: Value = serde_json::from_str(&ui_cancel("u2")).unwrap();
        assert_eq!((v["type"].as_str(), v["id"].as_str(), v["cancelled"].as_bool()), (Some("extension_ui_response"), Some("u2"), Some(true)));
        assert!(ui_event(r#"{"type":"extension_ui_request","id":"u3","method":"setStatus","statusKey":"k"}"#).is_none());
        assert_eq!(ui_event(r#"{"type":"auto_retry_end","success":false,"attempt":3,"finalError":"529"}"#).unwrap().kind, "error");
    }
}
