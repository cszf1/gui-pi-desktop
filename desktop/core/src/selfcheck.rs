//! Linux startup self-check: what the agent needs from this desktop session, with a clear message for each gap.
//! `gather()` reads the machine (environment, a few read-only commands); `evaluate()` turns the readings into
//! checks and is pure, so every message is tested. The settings page shows the result; a `fail` blocks Start.
use serde::Serialize;
use std::collections::HashMap;
use std::process::Command;

#[derive(Clone, Debug, Default)]
pub struct Probe {
    pub os: String,
    /// environment variables that matter (XDG_SESSION_TYPE, WAYLAND_DISPLAY, DISPLAY, DBUS_SESSION_BUS_ADDRESS, ...)
    pub env: HashMap<String, String>,
    /// the AT-SPI bus address from org.a11y.Bus.GetAddress (None: not reachable)
    pub atspi_bus: Option<String>,
    /// a DBus session bus answered (org.freedesktop.DBus.ListNames)
    pub dbus_ok: bool,
    /// the OS keychain accepted a test write (the shell's keychain probe)
    pub secret_service: bool,
    /// "2.35" from `getconf GNU_LIBC_VERSION`
    pub glibc: Option<String>,
    /// file name of the WebKitGTK library found, e.g. "libwebkit2gtk-4.1.so.0.13.7"
    pub webkit: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Check {
    pub id: &'static str,
    /// "ok" | "warn" | "fail"
    pub level: &'static str,
    pub message: String,
}

/// The oldest glibc the Linux build runs on (built on Ubuntu 22.04).
pub const MIN_GLIBC: (u32, u32) = (2, 35);

fn ver(s: &str) -> Option<(u32, u32)> {
    let mut it = s.trim().split('.').map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().ok());
    Some((it.next()??, it.next()??))
}

pub fn evaluate(p: &Probe) -> Vec<Check> {
    let mut out = vec![];
    let c = |id, level, m: String| Check { id, level, message: m };
    if p.os != "linux" {
        out.push(c("platform", "warn", format!("This build is tested on Linux; {} support is a later milestone.", p.os)));
        return out;
    }
    let e = |k: &str| p.env.get(k).map(|s| s.as_str()).unwrap_or("");
    let session = e("XDG_SESSION_TYPE").to_lowercase();
    let wayland = session == "wayland" || !e("WAYLAND_DISPLAY").is_empty();
    if wayland {
        out.push(c("session", "warn", format!(
            "Wayland session detected. The agent supports X11: under Wayland it cannot read other apps' windows reliably, send them input or take their screenshots{}. Log in with an \"X11\" / \"Xorg\" session for full support.",
            if e("DISPLAY").is_empty() { "" } else { " (XWayland apps partly work)" })));
    } else if !e("DISPLAY").is_empty() {
        out.push(c("session", "ok", format!("X11 session (DISPLAY={}).", e("DISPLAY"))));
    } else {
        out.push(c("session", "fail", "No graphical session found (neither DISPLAY nor WAYLAND_DISPLAY is set). Start the app from your desktop session.".into()));
    }
    if p.dbus_ok {
        out.push(c("dbus", "ok", "DBus session bus reachable.".into()));
    } else {
        out.push(c("dbus", "fail", "No DBus session bus (DBUS_SESSION_BUS_ADDRESS unset or not answering). The accessibility bus and the keychain need it.".into()));
    }
    match &p.atspi_bus {
        Some(a) if !a.is_empty() => out.push(c("atspi", "ok", "AT-SPI accessibility bus reachable.".into())),
        _ => out.push(c("atspi", "fail", "The AT-SPI accessibility bus is not reachable, so app windows cannot be read. Install at-spi2-core and make sure accessibility is enabled (GNOME: gsettings set org.gnome.desktop.interface toolkit-accessibility true), then log out and in.".into())),
    }
    if p.secret_service {
        out.push(c("keychain", "ok", "Secret Service keychain available: keys are stored in it.".into()));
    } else {
        out.push(c("keychain", "warn", crate::keys::store_notice("memory").into()));
    }
    match p.glibc.as_deref().and_then(ver) {
        Some(v) if v >= MIN_GLIBC => out.push(c("glibc", "ok", format!("glibc {}.{}.", v.0, v.1))),
        Some(v) => out.push(c("glibc", "fail", format!("glibc {}.{} is older than {}.{}, which this build needs. Use a newer distribution (Ubuntu 22.04 or later).", v.0, v.1, MIN_GLIBC.0, MIN_GLIBC.1))),
        None => out.push(c("glibc", "warn", "Could not read the glibc version (non-glibc system?). The build needs glibc 2.35 or later.".into())),
    }
    match &p.webkit {
        Some(w) => out.push(c("webkit", "ok", format!("WebKitGTK found ({w})."))),
        None => out.push(c("webkit", "fail", "WebKitGTK 4.1 (libwebkit2gtk-4.1) was not found; the app windows need it. Install libwebkit2gtk-4.1-0.".into())),
    }
    out
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    // the session's own environment: inside an AppImage its bundled libraries must not reach system tools (B4)
    let o = Command::new(cmd).args(args).env_clear().envs(crate::launch::system_env(std::env::vars())).output().ok()?;
    if !o.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Read this machine. `secret_service` comes from the shell's keychain probe.
pub fn gather(secret_service: bool) -> Probe {
    let mut env = HashMap::new();
    for k in ["XDG_SESSION_TYPE", "WAYLAND_DISPLAY", "DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "AT_SPI_BUS_ADDRESS"] {
        if let Ok(v) = std::env::var(k) {
            env.insert(k.to_string(), v);
        }
    }
    let os = std::env::consts::OS.to_string();
    let has_bus = env.contains_key("DBUS_SESSION_BUS_ADDRESS");
    let dbus_ok = has_bus && run("dbus-send", &["--session", "--print-reply", "--dest=org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus.ListNames"]).is_some();
    let atspi_bus = env.get("AT_SPI_BUS_ADDRESS").cloned().filter(|s| !s.is_empty()).or_else(|| {
        if !has_bus { return None; }
        run("dbus-send", &["--session", "--print-reply", "--dest=org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus.GetAddress"])
            .and_then(|o| o.split('"').nth(1).map(String::from)).filter(|s| !s.is_empty())
    });
    let glibc = run("getconf", &["GNU_LIBC_VERSION"]).and_then(|s| s.split_whitespace().nth(1).map(String::from));
    let webkit = run("ldconfig", &["-p"]).or_else(|| run("/sbin/ldconfig", &["-p"])).and_then(|o| {
        o.lines().find(|l| l.contains("libwebkit2gtk-4.1.so")).and_then(|l| l.split("=>").nth(1)).map(|p| {
            let p = p.trim();
            std::fs::canonicalize(p).ok().and_then(|r| r.file_name().map(|f| f.to_string_lossy().to_string())).unwrap_or_else(|| p.to_string())
        })
    });
    Probe { os, env, atspi_bus, dbus_ok, secret_service, glibc, webkit }
}

/// True when nothing blocks starting a task.
pub fn can_start(checks: &[Check]) -> bool {
    !checks.iter().any(|c| c.level == "fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Probe {
        Probe { os: "linux".into(), env: [("XDG_SESSION_TYPE", "x11"), ("DISPLAY", ":0")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                atspi_bus: Some("unix:path=/run/user/1000/at-spi/bus".into()), dbus_ok: true, secret_service: true,
                glibc: Some("2.35".into()), webkit: Some("libwebkit2gtk-4.1.so.0.13.7".into()) }
    }
    fn level(cs: &[Check], id: &str) -> &'static str {
        cs.iter().find(|c| c.id == id).unwrap().level
    }

    #[test]
    fn a_good_x11_session_passes() {
        let cs = evaluate(&good());
        assert!(cs.iter().all(|c| c.level == "ok"), "{cs:?}");
        assert!(can_start(&cs));
    }

    #[test]
    fn wayland_is_explained_not_blocked() {
        let mut p = good();
        p.env.insert("XDG_SESSION_TYPE".into(), "wayland".into());
        p.env.insert("WAYLAND_DISPLAY".into(), "wayland-0".into());
        let cs = evaluate(&p);
        let s = cs.iter().find(|c| c.id == "session").unwrap();
        assert_eq!(s.level, "warn");
        assert!(s.message.contains("Wayland") && s.message.contains("X11") && s.message.contains("XWayland"));
        assert!(can_start(&cs));
    }

    #[test]
    fn missing_pieces_have_clear_messages() {
        let p = Probe { os: "linux".into(), ..Default::default() };
        let cs = evaluate(&p);
        for id in ["session", "dbus", "atspi", "webkit"] {
            assert_eq!(level(&cs, id), "fail", "{id}");
        }
        assert!(cs.iter().find(|c| c.id == "atspi").unwrap().message.contains("at-spi2-core"));
        assert_eq!(level(&cs, "keychain"), "warn");
        assert!(cs.iter().find(|c| c.id == "keychain").unwrap().message.contains("this run only"));
        assert!(!can_start(&cs));
        let mut p = good();
        p.glibc = Some("2.31".into());
        let cs = evaluate(&p);
        assert_eq!(level(&cs, "glibc"), "fail");
        assert!(cs.iter().find(|c| c.id == "glibc").unwrap().message.contains("2.31"));
        assert_eq!(ver("2.39"), Some((2, 39)));
        assert_eq!(ver("x"), None);
    }

    #[test]
    fn other_platforms_say_so_and_gather_does_not_panic() {
        let cs = evaluate(&Probe { os: "windows".into(), ..Default::default() });
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].level, "warn");
        let p = gather(false);
        assert_eq!(p.os, std::env::consts::OS);
    }
}
