//! Pi RPC (JSONL over stdio): the commands the shell sends and a compact view of the events for the main window.
use serde::Serialize;
use serde_json::{json, Value};

pub fn prompt(id: &str, message: &str) -> String {
    json!({"id": id, "type": "prompt", "message": message}).to_string()
}

pub fn abort() -> String {
    json!({"type": "abort"}).to_string()
}

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

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UiEvent {
    /// "tool" | "gate" | "result" | "text" | "done" | "error" | "ui_request"
    pub kind: String,
    pub text: String,
    /// a screenshot the tool returned (base64), for the thumbnail
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

fn content_text(c: &Value) -> String {
    c.as_array().map(|a| a.iter().filter_map(|x| x.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("\n")).unwrap_or_default()
}

fn first_image(c: &Value) -> Option<String> {
    c.as_array()?.iter().find(|x| x.get("type").and_then(|t| t.as_str()) == Some("image"))
        .and_then(|x| x.get("data").and_then(|d| d.as_str())).map(String::from)
}

/// One Pi event -> what the main window shows (None: not shown).
pub fn ui_event(line: &str) -> Option<UiEvent> {
    let v: Value = serde_json::from_str(line).ok()?;
    let t = v.get("type")?.as_str()?;
    let ev = |kind: &str, text: String, image: Option<String>| Some(UiEvent { kind: kind.into(), text, image });
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
            let kind = if first.starts_with("[gate]") || first.starts_with("[human via") { "gate" } else { "result" };
            ev(kind, first, first_image(c))
        }
        "message_end" => {
            let m = v.get("message")?;
            if m.get("role").and_then(|r| r.as_str()) != Some("assistant") {
                return None;
            }
            let text = m.get("content").and_then(|c| c.as_array()).map(|a| a.iter()
                .filter(|x| x.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|x| x.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("\n")).unwrap_or_default();
            if text.trim().is_empty() { None } else { ev("text", text, None) }
        }
        "agent_end" => ev("done", String::new(), None),
        "extension_ui_request" => ev("ui_request", v.to_string(), None),
        "response" if v.get("success") == Some(&json!(false)) => ev("error", v.get("error").map(|e| e.to_string()).unwrap_or_default(), None),
        _ => None,
    }
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
        assert_eq!(ui_event(r#"{"type":"agent_end","messages":[]}"#).unwrap().kind, "done");
        assert!(ui_event(r#"{"type":"turn_start"}"#).is_none());
        let m = ui_event(r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"Done: saved q3.ods"}]}}"#).unwrap();
        assert_eq!(m.text, "Done: saved q3.ods");
        assert!(prompt("1", "hi").contains("\"type\":\"prompt\""));
    }
}
