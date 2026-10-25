//! Release hygiene, checked at every start:
//!   - integrity: the bundled resources (node, Pi, the extension, prompts, skills, gui-gate, Cua Driver with its
//!     signature files as shipped) must match the SHA256SUMS published with the release and shipped next to them.
//!     A changed or missing safety component (gate, driver, extension, node, Pi) blocks Start; nothing is repaired or
//!     re-signed here, the driver's own signature stays as shipped.
//!   - updates: no auto-update. At most an "update available" notice from the public latest-release record; the
//!     user downloads and installs it themselves (packaging/linux/appimage-update.sh keeps the previous AppImage).
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Report {
    /// files listed and matching
    pub ok: usize,
    pub missing: Vec<String>,
    pub mismatched: Vec<String>,
    /// no SHA256SUMS next to the resources (a dev run): nothing could be verified
    pub unverified: bool,
}

impl Report {
    /// Start is blocked when a listed file is missing or changed.
    pub fn blocks_start(&self) -> bool {
        !self.missing.is_empty() || !self.mismatched.is_empty()
    }
    pub fn message(&self) -> String {
        if self.unverified {
            return "Bundled components were not verified (no SHA256SUMS: a development run).".into();
        }
        if !self.blocks_start() {
            return format!("All {} bundled files match the release's SHA256SUMS.", self.ok);
        }
        let mut parts = vec![];
        if !self.mismatched.is_empty() {
            parts.push(format!("changed: {}", self.mismatched.iter().take(5).cloned().collect::<Vec<_>>().join(", ")));
        }
        if !self.missing.is_empty() {
            parts.push(format!("missing: {}", self.missing.iter().take(5).cloned().collect::<Vec<_>>().join(", ")));
        }
        format!("Bundled components do not match the release ({}). Reinstall the app from the release page; no task can start until then.", parts.join("; "))
    }
}

pub fn sha256_file(p: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// `sha256sum` output lines: "<64 hex>  ./path" (text mode) or "<hex> *path" (binary mode).
pub fn parse_sums(text: &str) -> Vec<(String, String)> {
    text.lines().filter_map(|l| {
        let l = l.trim_end();
        if l.len() < 66 || l.starts_with('#') {
            return None;
        }
        let (h, rest) = l.split_at(64);
        if !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let path = rest.strip_prefix("  ").or_else(|| rest.strip_prefix(" *"))?;
        let path = path.strip_prefix("./").unwrap_or(path);
        // never leave the resources directory
        if path.is_empty() || path.starts_with('/') || path.split('/').any(|c| c == "..") {
            return None;
        }
        Some((h.to_lowercase(), path.to_string()))
    }).collect()
}

/// Verify `dir` against `dir/SHA256SUMS`.
pub fn verify(dir: &Path) -> Report {
    let Ok(sums) = std::fs::read_to_string(dir.join("SHA256SUMS")) else {
        return Report { unverified: true, ..Default::default() };
    };
    let mut r = Report::default();
    for (h, rel) in parse_sums(&sums) {
        match sha256_file(&dir.join(&rel)) {
            Ok(x) if x == h => r.ok += 1,
            Ok(_) => r.mismatched.push(rel),
            Err(_) => r.missing.push(rel),
        }
    }
    if r.ok == 0 && r.missing.is_empty() && r.mismatched.is_empty() {
        r.unverified = true;
    }
    r
}

// ------------------------------------------------------------------ updates (notice only)

/// The public record the notice reads (no prereleases: CI builds are prereleases).
pub const LATEST_RELEASE_API: &str = "https://api.github.com/repos/cszf1/gui-pi-desktop/releases/latest";

fn semver(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches('v');
    let core = v.split(|c| c == '-' || c == '+').next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next()??, it.next().flatten().unwrap_or(0)))
}

/// From the latest-release JSON: Some(notice) when it is newer than `current`. Never downloads anything.
pub fn update_notice(current: &str, latest_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(latest_json).ok()?;
    if v.get("prerelease").and_then(|b| b.as_bool()).unwrap_or(false) || v.get("draft").and_then(|b| b.as_bool()).unwrap_or(false) {
        return None;
    }
    let tag = v.get("tag_name")?.as_str()?;
    let (new, cur) = (semver(tag)?, semver(current)?);
    if new <= cur {
        return None;
    }
    let url = v.get("html_url").and_then(|u| u.as_str()).filter(|u| u.starts_with("https://github.com/cszf1/gui-pi-desktop/"))
        .unwrap_or("https://github.com/cszf1/gui-pi-desktop/releases");
    Some(format!("Update available: {tag} (you have {current}). Nothing is installed automatically; download it from {url} and verify it against its SHA256SUMS. The previous version stays available for rollback."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("gpd-int-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("cua-driver")).unwrap();
        std::fs::write(d.join("gui-gate"), b"gate binary").unwrap();
        std::fs::write(d.join("cua-driver/cua-driver"), b"driver").unwrap();
        std::fs::write(d.join("cua-driver/cua-driver.sig"), b"signature as shipped").unwrap();
        d
    }

    #[test]
    fn verifies_against_the_shipped_sums() {
        let d = tree();
        let sums: String = ["gui-gate", "cua-driver/cua-driver", "cua-driver/cua-driver.sig"].iter()
            .map(|f| format!("{}  ./{f}\n", sha256_file(&d.join(f)).unwrap())).collect();
        assert_eq!(sha256_file(&d.join("gui-gate")).unwrap().len(), 64);
        std::fs::write(d.join("SHA256SUMS"), &sums).unwrap();
        let r = verify(&d);
        assert_eq!((r.ok, r.blocks_start(), r.unverified), (3, false, false));
        assert!(r.message().contains("All 3"));
        // a changed gate and a missing signature block Start
        std::fs::write(d.join("gui-gate"), b"patched gate").unwrap();
        std::fs::remove_file(d.join("cua-driver/cua-driver.sig")).unwrap();
        let r = verify(&d);
        assert!(r.blocks_start());
        assert_eq!(r.mismatched, vec!["gui-gate".to_string()]);
        assert_eq!(r.missing, vec!["cua-driver/cua-driver.sig".to_string()]);
        assert!(r.message().contains("Reinstall"));
        // no sums file: a dev run, reported as unverified, not blocking
        std::fs::remove_file(d.join("SHA256SUMS")).unwrap();
        let r = verify(&d);
        assert!(r.unverified && !r.blocks_start());
    }

    #[test]
    fn sums_never_point_outside() {
        let h = "a".repeat(64);
        let p = parse_sums(&format!("{h}  ./ok\n{h}  ../../etc/passwd\n{h}  /etc/shadow\n{h} *bin/x\nnot a line\n"));
        assert_eq!(p.iter().map(|x| x.1.as_str()).collect::<Vec<_>>(), vec!["ok", "bin/x"]);
    }

    #[test]
    fn update_notice_only_for_newer_releases() {
        let j = |tag: &str, pre: bool| format!(r#"{{"tag_name":"{tag}","prerelease":{pre},"html_url":"https://github.com/cszf1/gui-pi-desktop/releases/tag/{tag}"}}"#);
        let n = update_notice("0.3.0", &j("v0.4.0", false)).unwrap();
        assert!(n.contains("v0.4.0") && n.contains("Nothing is installed automatically") && n.contains("rollback"));
        assert!(update_notice("0.3.0", &j("v0.3.0", false)).is_none());
        assert!(update_notice("0.3.0", &j("v0.2.9", false)).is_none());
        assert!(update_notice("0.3.0", &j("v9.0.0", true)).is_none(), "prereleases (CI builds) are not offered");
        assert!(update_notice("0.3.0", "not json").is_none());
        let evil = r#"{"tag_name":"v1.0.0","html_url":"https://evil.example.com/x"}"#;
        assert!(update_notice("0.3.0", evil).unwrap().contains("https://github.com/cszf1/gui-pi-desktop/releases"));
    }
}
