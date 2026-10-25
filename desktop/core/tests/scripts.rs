//! packaging/linux scripts: the uninstall path list is the app's own and nothing else (a pre-existing Cua Driver
//! and user data survive a real run); the AppImage update keeps the previous version and refuses unverified files.
#![cfg(unix)]
use std::path::{Path, PathBuf};
use std::process::Command;

fn script(n: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/linux").join(n)
}

fn home(tag: &str) -> PathBuf {
    let h = std::env::temp_dir().join(format!("gpd-home-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&h);
    std::fs::create_dir_all(&h).unwrap();
    h
}

fn run(s: &str, h: &Path, args: &[&str]) -> (i32, String, String) {
    let o = Command::new("bash").arg(script(s)).args(args).env_clear()
        .env("PATH", "/usr/bin:/bin").env("HOME", h).env("XDG_RUNTIME_DIR", h.join("run")).output().unwrap();
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into(), String::from_utf8_lossy(&o.stderr).into())
}

#[test]
fn uninstall_paths_are_the_apps_own() {
    let h = home("u");
    let (code, out, err) = run("uninstall.sh", &h, &["--print-paths"]);
    assert_eq!(code, 0, "{err}");
    let paths: Vec<&str> = out.lines().collect();
    let hs = h.display().to_string();
    let want = [format!("{hs}/.config/io.github.cszf1.gui-pi-desktop"), format!("{hs}/.local/share/io.github.cszf1.gui-pi-desktop"),
                format!("{hs}/.cache/io.github.cszf1.gui-pi-desktop"), format!("{hs}/Applications/gui-pi-desktop"),
                format!("{hs}/.local/share/applications/io.github.cszf1.gui-pi-desktop.desktop"), format!("{hs}/run/gui-pi-desktop")];
    assert_eq!(paths, want.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    for p in &paths {
        assert!(!p.to_lowercase().contains("cua"), "{p}");
        assert!(p.ends_with("gui-pi-desktop") || p.ends_with("gui-pi-desktop.desktop"), "{p}");
    }
}

#[test]
fn uninstall_keeps_a_pre_existing_cua_driver_and_user_data() {
    let h = home("r");
    let mk = |rel: &str| {
        let p = h.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "x").unwrap();
        p
    };
    let ours = [mk(".config/io.github.cszf1.gui-pi-desktop/config.json"), mk(".local/share/io.github.cszf1.gui-pi-desktop/logs/agent.jsonl"),
                mk("Applications/gui-pi-desktop/gui-pi-desktop.AppImage"), mk("run/gui-pi-desktop/1.sock")];
    let theirs = [mk(".local/bin/cua-driver"), mk(".cua/config.json"), mk(".local/share/cua-driver/cua-driver"), mk("Documents/q3.ods"),
                  mk(".pi/agent/settings.json"), mk(".config/gui-gate/reviewer.env"), mk(".local/share/applications/other.desktop")];
    let (code, out, _) = run("uninstall.sh", &h, &["--dry-run"]);
    assert_eq!(code, 0);
    assert!(out.contains("would remove") && ours.iter().all(|p| p.exists()));
    let (code, _, err) = run("uninstall.sh", &h, &[]);
    assert_eq!(code, 0, "{err}");
    for p in &ours {
        assert!(!p.exists(), "{} survived", p.display());
    }
    for p in &theirs {
        assert!(p.exists(), "{} was deleted", p.display());
    }
    // a hostile XDG setting cannot widen the scope
    let o = Command::new("bash").arg(script("uninstall.sh")).arg("--print-paths").env_clear().env("PATH", "/usr/bin:/bin")
        .env("HOME", &h).env("XDG_CONFIG_HOME", "/etc").output().unwrap();
    assert_ne!(o.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&o.stderr).contains("refusing"));
}

#[test]
fn appimage_update_keeps_the_previous_and_rolls_back() {
    let h = home("a");
    let dir = h.join("dl");
    std::fs::create_dir_all(&dir).unwrap();
    let write = |n: &str, c: &str| {
        let p = dir.join(n);
        std::fs::write(&p, c).unwrap();
        p
    };
    let sha = |p: &Path| String::from_utf8(Command::new("sha256sum").arg(p).output().unwrap().stdout).unwrap()[..64].to_string();
    let v1 = write("gui-pi-desktop_0.2.0_amd64.AppImage", "v1");
    let v2 = write("gui-pi-desktop_0.3.0_amd64.AppImage", "v2");
    let sums = write("SHA256SUMS", &format!("{}  ./{}\n{}  ./{}\n", sha(&v1), "gui-pi-desktop_0.2.0_amd64.AppImage", sha(&v2), "gui-pi-desktop_0.3.0_amd64.AppImage"));
    let app = h.join("Applications/gui-pi-desktop");
    let cur = app.join("gui-pi-desktop.AppImage");
    let prev = app.join("gui-pi-desktop.previous.AppImage");
    let s = |p: &Path| p.display().to_string();
    assert_eq!(run("appimage-update.sh", &h, &["install", &s(&v1), &s(&sums)]).0, 0);
    assert_eq!(run("appimage-update.sh", &h, &["install", &s(&v2), &s(&sums)]).0, 0);
    assert_eq!(std::fs::read_to_string(&cur).unwrap(), "v2");
    assert_eq!(std::fs::read_to_string(&prev).unwrap(), "v1");
    assert_eq!(run("appimage-update.sh", &h, &["rollback"]).0, 0);
    assert_eq!((std::fs::read_to_string(&cur).unwrap(), std::fs::read_to_string(&prev).unwrap()), ("v1".into(), "v2".into()));
    // a tampered or unlisted file is refused and changes nothing
    let bad = write("gui-pi-desktop_0.3.0_amd64.AppImage", "tampered");
    assert_eq!(run("appimage-update.sh", &h, &["install", &s(&bad), &s(&sums)]).0, 5);
    let other = write("other.AppImage", "x");
    assert_eq!(run("appimage-update.sh", &h, &["install", &s(&other), &s(&sums)]).0, 4);
    assert_ne!(run("appimage-update.sh", &h, &["install", &s(&v1)]).0, 0, "no install without SHA256SUMS");
    assert_eq!(std::fs::read_to_string(&cur).unwrap(), "v1");
}
