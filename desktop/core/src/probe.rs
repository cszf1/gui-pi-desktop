//! "Test connection": one tiny request per configured model, built here and sent by the shell's Rust side
//! (the key comes from the keychain and goes only into a request header; the webview never sees it, and the
//! answer it gets back is ok / latency / a short error with the key masked).
use crate::config::{check_base_url, ModelCfg};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct ProbeReq {
    pub url: String,
    /// (name, value): the key is only ever in here
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

/// The gate's rule for turning a base URL into the endpoint (mirrors gui-gate reviewer::endpoint).
pub fn gate_endpoint(url: &str, protocol: &str, model: &str) -> String {
    let u = url.trim().trim_end_matches('/').to_string();
    let lower = u.to_lowercase();
    let full = ["/chat/completions", "/responses", "/messages", ":generatecontent", "systemone", "/decide", "/decisions"];
    if full.iter().any(|f| lower.ends_with(f)) {
        return u;
    }
    let path = lower.splitn(4, '/').nth(3).unwrap_or("");
    let last = path.rsplit('/').next().unwrap_or("");
    let is_version = last.len() >= 2 && last.starts_with('v') && last[1..].chars().next().map_or(false, |c| c.is_ascii_digit());
    if !path.is_empty() && !is_version {
        return u;
    }
    let model = model.trim();
    match (protocol, path.is_empty()) {
        ("anthropic", true) => format!("{u}/v1/messages"),
        ("anthropic", false) => format!("{u}/messages"),
        ("gemini", true) => format!("{u}/v1beta/models/{model}:generateContent"),
        ("gemini", false) => format!("{u}/models/{model}:generateContent"),
        ("responses", true) => format!("{u}/v1/responses"),
        ("responses", false) => format!("{u}/responses"),
        ("systemone", true) => format!("{u}/v1/systemone"),
        ("systemone", false) => format!("{u}/systemone"),
        (_, true) => format!("{u}/v1/chat/completions"),
        (_, false) => format!("{u}/chat/completions"),
    }
}

/// Pi's base URL + api name -> the endpoint (Pi appends the API path to its baseUrl).
fn pi_endpoint(base: &str, api: &str, model: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    match api {
        "anthropic-messages" => if b.ends_with("/messages") { b.into() } else { format!("{b}/messages") },
        "google-generative-ai" => format!("{b}/models/{}:generateContent", model.trim()),
        "openai-responses" => if b.ends_with("/responses") { b.into() } else { format!("{b}/responses") },
        _ => if b.ends_with("/chat/completions") { b.into() } else { format!("{b}/chat/completions") },
    }
}

/// Build the probe. `gate`: the reviewer / a role (gate protocol names), else the main model (Pi api names).
pub fn request(m: &ModelCfg, key: &str, gate: bool) -> Result<ProbeReq, String> {
    check_base_url(&m.base_url)?;
    if m.model.trim().is_empty() {
        return Err("model name is required".into());
    }
    let proto: &str = match (gate, m.protocol.as_str()) {
        (false, "anthropic-messages") | (true, "anthropic") => "anthropic",
        (false, "google-generative-ai") | (true, "gemini") => "gemini",
        (false, "openai-responses") | (true, "responses") => "responses",
        (true, "systemone") => "systemone",
        _ => "chat",
    };
    let url = if gate { gate_endpoint(&m.base_url, proto, &m.model) } else { pi_endpoint(&m.base_url, &m.protocol, &m.model) };
    let model = m.model.trim();
    let (headers, body) = match proto {
        "anthropic" => (vec![("x-api-key".to_string(), key.to_string()), ("anthropic-version".into(), "2023-06-01".into())],
                        json!({"model": model, "max_tokens": 8, "messages": [{"role": "user", "content": "ping"}]})),
        "gemini" => (vec![("x-goog-api-key".to_string(), key.to_string())], json!({"contents": [{"parts": [{"text": "ping"}]}]})),
        "responses" => (vec![("Authorization".to_string(), format!("Bearer {key}"))], json!({"model": model, "input": "ping", "max_output_tokens": 16})),
        "systemone" => (vec![("Authorization".to_string(), format!("Bearer {key}"))],
                        json!({"model": model, "messages": [{"role": "user", "content": "ping"}], "max_tokens": 8})),
        _ => (vec![("Authorization".to_string(), format!("Bearer {key}"))],
              json!({"model": model, "max_tokens": 8, "messages": [{"role": "user", "content": "ping"}]})),
    };
    Ok(ProbeReq { url, headers, body })
}

/// The answer the settings page gets: never the key, even when the server echoes it.
pub fn verdict(status: Option<u16>, body: &str, transport_err: Option<&str>, key: &str, systemone: bool) -> Result<String, String> {
    let mask = |s: &str| {
        let s: String = s.chars().take(200).collect();
        if key.trim().len() >= 4 { s.replace(key.trim(), "***") } else { s }
    };
    match (status, transport_err) {
        (_, Some(e)) => Err(format!("cannot reach the server: {}", mask(e))),
        (Some(c), _) if (200..300).contains(&c) => Ok("ok".into()),
        (Some(401), _) | (Some(403), _) => Err(format!("the key was rejected (HTTP {}): {}", status.unwrap(), mask(body))),
        (Some(404), _) => Err(format!("endpoint or model not found (HTTP 404): {}", mask(body))),
        // a decision API may refuse the ping's shape but has accepted the key
        (Some(c), _) if systemone && (c == 400 || c == 422) => Ok(format!("reachable, key accepted (HTTP {c} for the test message)")),
        (Some(c), _) => Err(format!("HTTP {c}: {}", mask(body))),
        (None, None) => Err("no answer".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, url: &str) -> ModelCfg {
        ModelCfg { protocol: p.into(), base_url: url.into(), model: "mod".into(), ..Default::default() }
    }

    #[test]
    fn key_only_in_headers() {
        for (p, gate) in [("openai-completions", false), ("openai-responses", false), ("anthropic-messages", false), ("google-generative-ai", false),
                          ("chat", true), ("responses", true), ("anthropic", true), ("gemini", true), ("systemone", true)] {
            let r = request(&m(p, "https://api.example.com/v1"), "sk-secret-1234", gate).unwrap();
            assert!(!r.url.contains("sk-secret") && !r.body.to_string().contains("sk-secret"), "{p}");
            assert!(r.headers.iter().any(|(_, v)| v.contains("sk-secret-1234")), "{p}");
        }
    }

    #[test]
    fn endpoints() {
        assert_eq!(request(&m("openai-completions", "https://a.example.com/v1/"), "k", false).unwrap().url, "https://a.example.com/v1/chat/completions");
        assert_eq!(request(&m("google-generative-ai", "https://g.example.com/v1beta"), "k", false).unwrap().url, "https://g.example.com/v1beta/models/mod:generateContent");
        assert_eq!(request(&m("chat", "https://rev.example.com"), "k", true).unwrap().url, "https://rev.example.com/v1/chat/completions");
        assert_eq!(request(&m("anthropic", "https://x.example.com/v1"), "k", true).unwrap().url, "https://x.example.com/v1/messages");
        assert_eq!(request(&m("systemone", "https://s.example.com/v1/systemone"), "k", true).unwrap().url, "https://s.example.com/v1/systemone");
        assert!(request(&m("chat", "http://x.example.com"), "k", true).is_err());
        assert!(request(&ModelCfg { model: " ".into(), ..m("chat", "https://x.example.com") }, "k", true).is_err());
    }

    #[test]
    fn verdict_masks_the_key() {
        assert!(verdict(Some(200), "", None, "k", false).is_ok());
        let e = verdict(Some(401), "invalid key sk-secret-1234", None, "sk-secret-1234", false).unwrap_err();
        assert!(e.contains("rejected") && !e.contains("sk-secret-1234") && e.contains("***"));
        assert!(verdict(Some(422), "bad shape", None, "k", true).is_ok());
        assert!(verdict(Some(422), "bad shape", None, "k", false).is_err());
        assert!(verdict(None, "", Some("dns error"), "k", false).unwrap_err().contains("cannot reach"));
    }
}
