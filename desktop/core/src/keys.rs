//! Key slots. The app stores keys in the OS keychain (Secret Service / Windows Credential Manager / macOS
//! Keychain) through the `KeyStore` the shell provides; there is no plain-text fallback. When no keychain is
//! available the user may choose "this run only": keys then live only in this process's memory.
use std::collections::HashMap;
use std::sync::Mutex;

pub const SERVICE: &str = "gui-pi-desktop";

pub trait KeyStore: Send + Sync {
    fn get(&self, slot: &str) -> Option<String>;
    fn set(&self, slot: &str, value: &str) -> Result<(), String>;
    fn delete(&self, slot: &str) -> Result<(), String>;
    /// "keychain" | "memory"
    fn kind(&self) -> &'static str;
    fn has(&self, slot: &str) -> bool {
        self.get(slot).map(|v| !v.trim().is_empty()).unwrap_or(false)
    }
}

/// Keys for this run only (no keychain on this machine, and in tests).
#[derive(Default)]
pub struct MemoryStore {
    map: Mutex<HashMap<String, String>>,
}

impl KeyStore for MemoryStore {
    fn get(&self, slot: &str) -> Option<String> {
        self.map.lock().ok()?.get(slot).cloned()
    }
    fn set(&self, slot: &str, value: &str) -> Result<(), String> {
        self.map.lock().map_err(|e| e.to_string())?.insert(slot.into(), value.trim().into());
        Ok(())
    }
    fn delete(&self, slot: &str) -> Result<(), String> {
        self.map.lock().map_err(|e| e.to_string())?.remove(slot);
        Ok(())
    }
    fn kind(&self) -> &'static str {
        "memory"
    }
}

/// Never show a key: the settings page gets "set" + the last 4 characters at most.
pub fn preview(v: &str) -> String {
    let n = v.chars().count();
    if n < 12 {
        return "set".into();
    }
    format!("set (...{})", v.chars().skip(n - 4).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_store_and_preview() {
        let m = MemoryStore::default();
        assert!(!m.has("main"));
        m.set("main", " sk-abcdefghijklmnop ").unwrap();
        assert!(m.has("main"));
        assert_eq!(preview(&m.get("main").unwrap()), "set (...mnop)");
        assert_eq!(preview("short"), "set");
        m.delete("main").unwrap();
        assert!(!m.has("main"));
    }
}
