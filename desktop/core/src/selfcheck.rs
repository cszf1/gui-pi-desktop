//! Startup self-check (Linux, Windows, macOS): what the agent needs from this desktop session, with a clear message
//! for each gap.
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
    /// Windows readings (None elsewhere)
    pub win: Option<WinProbe>,
    /// macOS readings (None elsewhere)
    pub mac: Option<MacProbe>,
}

#[derive(Clone, Debug)]
pub struct WinProbe {
    /// "Windows 11 Pro" (from the registry; "Windows 10" names are corrected to 11 from the build number)
    pub product: String,
    /// CurrentBuildNumber, e.g. 22631
    pub build: Option<u32>,
    /// DisplayVersion, e.g. "23H2"
    pub display_version: String,
    /// the WebView2 runtime version the app found (Tauri's webview_version, else the registry); None: not installed
    pub webview2: Option<String>,
    /// UI Automation answered: Ok(top-level window count) or Err(what went wrong)
    pub uia: Result<u32, String>,
    /// SESSIONNAME: "Console", "RDP-Tcp#0", ... (empty: unknown, e.g. a service)
    pub session: String,
    /// the app runs elevated (High integrity level)
    pub elevated: bool,
}

impl Default for WinProbe {
    fn default() -> Self {
        WinProbe { product: String::new(), build: None, display_version: String::new(), webview2: None, uia: Err("not checked".into()),
                   session: String::new(), elevated: false }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MacProbe {
    /// sw_vers -productVersion, e.g. "14.6.1"
    pub version: String,
    /// uname -m: arm64 / x86_64
    pub arch: String,
    /// the bundled CuaDriver.app's permissions (`cua-driver permissions status --json`): "granted" | "denied" |
    /// "unknown" (no daemon running yet, or the command failed)
    pub accessibility: String,
    pub screen_recording: String,
    /// the raw answer, for the log
    pub raw: String,
    /// CuaDriver.app was found in the bundle
    pub driver: bool,
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
    match p.os.as_str() {
        "linux" => {}
        "windows" => return evaluate_windows(p),
        "macos" => return evaluate_macos(p),
        other => {
            out.push(c("platform", "warn", format!("{other} is not a supported platform (Linux, Windows and macOS are).")));
            return out;
        }
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

/// The oldest Windows build supported: Windows 10 1809 (WebView2 and UI Automation as the driver uses them).
pub const MIN_WIN_BUILD: u32 = 17763;

fn keychain_check(p: &Probe, name: &str) -> Check {
    if p.secret_service {
        Check { id: "keychain", level: "ok", message: format!("{name} available: keys are stored in it.") }
    } else {
        Check { id: "keychain", level: "warn", message: format!("{name} is not usable here. {}", crate::keys::store_notice("memory")) }
    }
}

pub fn evaluate_windows(p: &Probe) -> Vec<Check> {
    let mut out = vec![];
    let c = |id, level, m: String| Check { id, level, message: m };
    let w = p.win.clone().unwrap_or_default();
    match w.build {
        Some(b) if b >= MIN_WIN_BUILD => out.push(c("windows", "ok", format!("{} {} (build {b}).", w.product, w.display_version).replace("  ", " "))),
        Some(b) => out.push(c("windows", "fail", format!("{} build {b} is older than Windows 10 1809 (build {MIN_WIN_BUILD}), which this build needs.", w.product))),
        None => out.push(c("windows", "warn", "Could not read the Windows version from the registry.".into())),
    }
    match &w.webview2 {
        Some(v) if !v.is_empty() && v != "0.0.0.0" => out.push(c("webview2", "ok", format!("WebView2 runtime {v}."))),
        _ => out.push(c("webview2", "fail", "The Microsoft Edge WebView2 runtime was not found; the app windows need it. Install it from https://developer.microsoft.com/microsoft-edge/webview2/ (the installer normally does this).".into())),
    }
    match &w.uia {
        Ok(n) => out.push(c("uia", "ok", format!("UI Automation answers ({n} top-level windows)."))),
        Err(e) => out.push(c("uia", "fail", format!("UI Automation did not answer ({e}), so app windows cannot be read. Run the app in a normal, unlocked desktop session."))),
    }
    let sess = w.session.to_ascii_lowercase();
    if sess == "console" {
        out.push(c("session", "ok", "Interactive console session.".into()));
    } else if sess.starts_with("rdp-") {
        out.push(c("session", "warn", format!("Remote Desktop session ({}). Keep the RDP window open and connected: when it is minimized or disconnected Windows has no interactive desktop and the agent can neither see nor click anything.", w.session)));
    } else {
        out.push(c("session", "warn", format!("Unknown session type ({}); the agent needs an interactive, unlocked desktop.", if w.session.is_empty() { "SESSIONNAME not set" } else { w.session.as_str() })));
    }
    if w.elevated {
        out.push(c("elevation", "warn", "The app runs as administrator, so the agent could act on administrator windows too. Start it normally; it asks you to take over where admin rights are needed.".into()));
    }
    out.push(keychain_check(p, "Windows Credential Manager"));
    out
}

pub fn evaluate_macos(p: &Probe) -> Vec<Check> {
    let mut out = vec![];
    let c = |id, level, m: String| Check { id, level, message: m };
    let m = p.mac.clone().unwrap_or_default();
    match ver(&m.version) {
        Some(v) if v >= (13, 0) => out.push(c("macos", "ok", format!("macOS {} ({}).", m.version, m.arch))),
        Some(_) => out.push(c("macos", "fail", format!("macOS {} is older than 13 (Ventura), which the bundled Cua Driver needs.", m.version))),
        None => out.push(c("macos", "warn", "Could not read the macOS version.".into())),
    }
    if !m.driver {
        out.push(c("driver", "fail", "CuaDriver.app is missing from the app bundle; reinstall the app.".into()));
        out.push(keychain_check(p, "macOS Keychain"));
        return out;
    }
    let perm = |id: &'static str, what: &str, state: &str, pane: &str| -> Check {
        match state {
            "granted" => c(id, "ok", format!("{what}: granted to CuaDriver (com.trycua.driver).")),
            "denied" => c(id, "fail", format!("{what} is not granted to CuaDriver. Press \"Grant permissions\" below (or System Settings > Privacy & Security > {pane}: turn on CuaDriver), then restart gui-pi.")),
            _ => c(id, "warn", format!("{what} for CuaDriver is unknown (its helper is not running yet). Press \"Grant permissions\" below; macOS shows its own prompt for CuaDriver.")),
        }
    };
    out.push(perm("accessibility", "Accessibility", &m.accessibility, "Accessibility"));
    out.push(perm("screen", "Screen Recording", &m.screen_recording, "Screen & System Audio Recording"));
    out.push(keychain_check(p, "macOS Keychain"));
    out
}

/// Reads "granted" / "denied" / "unknown" for one permission out of `cua-driver permissions status --json`.
/// Shapes of Cua Driver 0.38.0 (the first one seen in CI on a macOS runner, the second its daemon's
/// check_permissions answer, from Cua's source):
/// - no CuaDriver daemon running: `{"daemon_running": false, "status": "unknown", "reason": "..."}` -> unknown
/// - daemon running: `{"accessibility": bool, "screen_recording": bool, "screen_recording_capturable": bool|null,
///   "direct_capture_status": ..., "source": {...}}`
/// Text before the JSON (a telemetry notice) is skipped. Other shapes: any key containing `needle`
/// (case-insensitive) with a boolean or a status string.
pub fn permission_state(raw: &str, needle: &str) -> String {
    let body = raw.find('{').map(|i| &raw[i..]).unwrap_or(raw);
    let Some(Ok(v)) = serde_json::Deserializer::from_str(body).into_iter::<serde_json::Value>().next() else {
        return "unknown".into();
    };
    let exact = match needle { "screen" => "screen_recording", n => n };
    match v.get(exact) {
        Some(serde_json::Value::Bool(b)) => return if *b { "granted" } else { "denied" }.into(),
        Some(serde_json::Value::Null) => return "unknown".into(),
        None if v.get("status").and_then(|s| s.as_str()) == Some("unknown") => return "unknown".into(),
        _ => {}
    }
    permission_state_any(&v, needle)
}

fn permission_state_any(v: &serde_json::Value, needle: &str) -> String {
    fn walk(v: &serde_json::Value, needle: &str, key: &str) -> Option<String> {
        let hit = key.to_ascii_lowercase().replace(['_', '-', ' '], "").contains(needle);
        match v {
            serde_json::Value::Bool(b) if hit => Some(if *b { "granted" } else { "denied" }.into()),
            serde_json::Value::String(s) if hit => {
                let l = s.to_ascii_lowercase();
                Some(if ["granted", "true", "yes", "authorized", "allowed", "ok"].iter().any(|g| l == *g) { "granted" }
                     else if ["denied", "false", "no", "not_granted", "notgranted", "missing", "not granted"].iter().any(|g| l == *g) { "denied" }
                     else { "unknown" }.into())
            }
            serde_json::Value::Object(m) => {
                for (k, x) in m {
                    if let Some(r) = walk(x, needle, k) {
                        return Some(r);
                    }
                    if hit {
                        if let Some(r) = walk(x, needle, &format!("{key}{k}")) { return Some(r); }
                    }
                }
                None
            }
            _ => None,
        }
    }
    walk(v, needle, "").unwrap_or_else(|| "unknown".into())
}

/// Run a read-only system command with the session's environment and a time limit; stdout when it succeeded.
fn run_t(cmd: &str, args: &[&str], secs: u64) -> Result<String, String> {
    let mut c = Command::new(cmd);
    c.args(args).env_clear().envs(crate::launch::system_env(std::env::vars()))
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // no console window
    }
    let mut ch = c.spawn().map_err(|e| format!("{cmd}: {e}"))?;
    let t0 = std::time::Instant::now();
    loop {
        match ch.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if t0.elapsed().as_secs() < secs => std::thread::sleep(std::time::Duration::from_millis(50)),
            _ => {
                let _ = ch.kill();
                let _ = ch.wait();
                return Err(format!("{cmd} did not answer within {secs} s"));
            }
        }
    }
    let o = ch.wait_with_output().map_err(|e| e.to_string())?;
    let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if o.status.success() { Ok(out) } else { Err(format!("{cmd} exit {}: {}", o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stderr).trim().chars().take(200).collect::<String>())) }
}

/// `reg query KEY /v NAME` -> the value's data
fn reg(key: &str, name: &str) -> Option<String> {
    let o = run_t("reg", &["query", key, "/v", name], 10).ok()?;
    o.lines().find(|l| l.trim_start().starts_with(name)).and_then(|l| l.split("REG_").nth(1))
        .and_then(|r| r.split_once(char::is_whitespace).map(|(_, v)| v.trim().to_string())).filter(|v| !v.is_empty())
}

const WEBVIEW2_GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

/// The UI Automation probe: count the desktop's top-level windows through System.Windows.Automation.
pub const UIA_PS: &str = "Add-Type -AssemblyName UIAutomationClient; $r = [System.Windows.Automation.AutomationElement]::RootElement; $n = $r.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition).Count; Write-Output \"uia $n\"";

pub fn gather_windows(webview: Option<String>) -> WinProbe {
    let nt = "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion";
    let build = reg(nt, "CurrentBuildNumber").and_then(|b| b.parse::<u32>().ok());
    let mut product = reg(nt, "ProductName").unwrap_or_else(|| "Windows".into());
    if build.map(|b| b >= 22000).unwrap_or(false) {
        product = product.replace("Windows 10", "Windows 11");
    }
    let display_version = reg(nt, "DisplayVersion").unwrap_or_default();
    let webview2 = webview.filter(|v| !v.is_empty()).or_else(|| {
        [format!("HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{WEBVIEW2_GUID}"),
         format!("HKLM\\SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{WEBVIEW2_GUID}"),
         format!("HKCU\\Software\\Microsoft\\EdgeUpdate\\Clients\\{WEBVIEW2_GUID}")]
            .iter().find_map(|k| reg(k, "pv")).filter(|v| v != "0.0.0.0")
    });
    let uia = run_t("powershell", &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", UIA_PS], 30)
        .and_then(|o| o.lines().find_map(|l| l.trim().strip_prefix("uia ").and_then(|n| n.trim().parse::<u32>().ok())).ok_or_else(|| format!("unexpected answer: {}", o.chars().take(120).collect::<String>())));
    let session = std::env::var("SESSIONNAME").unwrap_or_default();
    // S-1-16-12288 = High Mandatory Level (elevated)
    let elevated = run_t("whoami", &["/groups"], 10).map(|o| o.contains("S-1-16-12288") || o.contains("S-1-16-16384")).unwrap_or(false);
    WinProbe { product, build, display_version, webview2, uia, session, elevated }
}

/// `driver`: the bundled CuaDriver.app's executable (its permissions are the ones that matter: it reads and acts).
pub fn gather_macos(driver: Option<&std::path::Path>) -> MacProbe {
    let version = run_t("sw_vers", &["-productVersion"], 10).unwrap_or_default();
    let arch = run_t("uname", &["-m"], 10).unwrap_or_default();
    let Some(d) = driver.filter(|d| d.exists()) else {
        return MacProbe { version, arch, accessibility: "unknown".into(), screen_recording: "unknown".into(), raw: String::new(), driver: false };
    };
    let raw = run_t(&d.display().to_string(), &["permissions", "status", "--json"], 20).unwrap_or_else(|e| e);
    MacProbe { version, arch, accessibility: permission_state(&raw, "accessibility"), screen_recording: permission_state(&raw, "screen"), raw, driver: true }
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    // the session's own environment: inside an AppImage its bundled libraries must not reach system tools (B4)
    let o = Command::new(cmd).args(args).env_clear().envs(crate::launch::system_env(std::env::vars())).output().ok()?;
    if !o.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// What the shell knows that the self-check cannot read itself.
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// the webview runtime version Tauri reports (WebView2 on Windows)
    pub webview: Option<String>,
    /// macOS: the bundled CuaDriver.app executable
    pub mac_driver: Option<std::path::PathBuf>,
}

/// Read this machine. `secret_service` comes from the shell's keychain probe.
pub fn gather(secret_service: bool) -> Probe {
    gather_with(secret_service, &Context::default())
}

pub fn gather_with(secret_service: bool, ctx: &Context) -> Probe {
    let os = std::env::consts::OS.to_string();
    if os == "windows" {
        return Probe { os, secret_service, win: Some(gather_windows(ctx.webview.clone())), ..Default::default() };
    }
    if os == "macos" {
        return Probe { os, secret_service, mac: Some(gather_macos(ctx.mac_driver.as_deref())), ..Default::default() };
    }
    let mut env = HashMap::new();
    for k in LINUX_ENV {
        if let Ok(v) = std::env::var(k) {
            env.insert(k.to_string(), v);
        }
    }
    let (dbus_ok, atspi_bus) = buses(&env);
    let glibc = run("getconf", &["GNU_LIBC_VERSION"]).and_then(|s| s.split_whitespace().nth(1).map(String::from));
    let webkit = run("ldconfig", &["-p"]).or_else(|| run("/sbin/ldconfig", &["-p"])).and_then(|o| {
        o.lines().find(|l| l.contains("libwebkit2gtk-4.1.so")).and_then(|l| l.split("=>").nth(1)).map(|p| {
            let p = p.trim();
            std::fs::canonicalize(p).ok().and_then(|r| r.file_name().map(|f| f.to_string_lossy().to_string())).unwrap_or_else(|| p.to_string())
        })
    });
    Probe { os, env, atspi_bus, dbus_ok, secret_service, glibc, webkit, ..Default::default() }
}

/// The session variables the Linux self-check reads.
pub const LINUX_ENV: &[&str] = &["XDG_SESSION_TYPE", "WAYLAND_DISPLAY", "DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "AT_SPI_BUS_ADDRESS"];

/// A `unix:path=...` bus address whose socket file is gone (the bus died; the variable is stale). Abstract and TCP
/// addresses cannot be checked this way and count as present.
pub fn stale_unix_bus(addr: &str) -> bool {
    addr.split(';').next().unwrap_or("").strip_prefix("unix:").map(|rest| {
        rest.split(',').find_map(|kv| kv.strip_prefix("path=")).map(|p| !std::path::Path::new(p).exists()).unwrap_or(false)
    }).unwrap_or(false)
}

/// DBus session bus and AT-SPI bus, from the given environment: (dbus answered, AT-SPI address). Used at start and
/// again when the extension reports the accessibility bus lost mid-task (Re-check on the settings page). A lost or
/// stale bus reads as unreachable, never as present.
pub fn buses(env: &HashMap<String, String>) -> (bool, Option<String>) {
    let bus = env.get("DBUS_SESSION_BUS_ADDRESS").cloned().filter(|s| !s.trim().is_empty() && !stale_unix_bus(s));
    let dbus_ok = bus.as_ref().map(|b| run_bus(b, &["--session", "--print-reply", "--dest=org.freedesktop.DBus", "/org/freedesktop/DBus",
                                                     "org.freedesktop.DBus.ListNames"]).is_some()).unwrap_or(false);
    let atspi = env.get("AT_SPI_BUS_ADDRESS").cloned().filter(|s| !s.is_empty() && !stale_unix_bus(s)).or_else(|| {
        let b = bus.as_ref().filter(|_| dbus_ok)?;
        run_bus(b, &["--session", "--print-reply", "--dest=org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus.GetAddress"])
            .and_then(|o| o.split('"').nth(1).map(String::from)).filter(|s| !s.is_empty() && !stale_unix_bus(s))
    });
    (dbus_ok, atspi)
}

/// dbus-send against an explicit session bus address (the given environment, not this process's), 5 s at most.
fn run_bus(addr: &str, args: &[&str]) -> Option<String> {
    let mut env: Vec<(String, String)> = crate::launch::system_env(std::env::vars()).into_iter().collect();
    env.retain(|(k, _)| k != "DBUS_SESSION_BUS_ADDRESS");
    env.push(("DBUS_SESSION_BUS_ADDRESS".into(), addr.into()));
    let mut c = Command::new("dbus-send");
    c.args(["--reply-timeout=5000"]).args(args).env_clear().envs(env).stdin(std::process::Stdio::null());
    let o = c.output().ok()?;
    if !o.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
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
                glibc: Some("2.35".into()), webkit: Some("libwebkit2gtk-4.1.so.0.13.7".into()), ..Default::default() }
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
    fn a_lost_dbus_or_atspi_bus_reads_as_unreachable_and_blocks_start() {
        // the session bus socket is gone (bus died, variable stale) and AT-SPI points at a dead socket
        let dir = std::env::temp_dir().join(format!("gpd-bus-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let gone = dir.join("bus-gone");
        let env: HashMap<String, String> = [("DISPLAY", ":0".to_string()), ("XDG_SESSION_TYPE", "x11".into()),
            ("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={}", gone.display())),
            ("AT_SPI_BUS_ADDRESS", format!("unix:path={},guid=abc", gone.display()))].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        assert!(stale_unix_bus(&env["DBUS_SESSION_BUS_ADDRESS"]));
        assert!(!stale_unix_bus("unix:abstract=/tmp/dbus-x,guid=1"), "abstract sockets cannot be checked: present");
        assert!(!stale_unix_bus("tcp:host=127.0.0.1,port=1"));
        let (dbus_ok, atspi) = buses(&env);
        assert!(!dbus_ok && atspi.is_none());
        let mut p = good();
        p.env = env;
        p.dbus_ok = dbus_ok;
        p.atspi_bus = atspi;
        let cs = evaluate(&p);
        assert_eq!((level(&cs, "dbus"), level(&cs, "atspi")), ("fail", "fail"));
        assert!(!can_start(&cs));
        // a socket file that exists but nothing answers: dbus-send fails (or is not installed): still unreachable
        std::fs::write(&gone, b"").unwrap();
        let (dbus_ok, _) = buses(&p.env);
        assert!(!dbus_ok);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn other_platforms_say_so_and_gather_does_not_panic() {
        let cs = evaluate(&Probe { os: "freebsd".into(), ..Default::default() });
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].level, "warn");
        let p = gather(false);
        assert_eq!(p.os, std::env::consts::OS);
    }

    fn win_good() -> Probe {
        Probe { os: "windows".into(), secret_service: true, win: Some(WinProbe { product: "Windows 11 Pro".into(), build: Some(22631),
            display_version: "23H2".into(), webview2: Some("131.0.2903.70".into()), uia: Ok(7), session: "Console".into(), elevated: false }),
            ..Default::default() }
    }

    #[test]
    fn windows_checks() {
        let cs = evaluate(&win_good());
        assert!(cs.iter().all(|c| c.level == "ok"), "{cs:?}");
        for id in ["windows", "webview2", "uia", "session", "keychain"] {
            assert_eq!(level(&cs, id), "ok", "{id}");
        }
        assert!(cs.iter().find(|c| c.id == "keychain").unwrap().message.contains("Credential Manager"));
        let mut p = win_good();
        let w = p.win.as_mut().unwrap();
        w.build = Some(17134);
        w.webview2 = Some("0.0.0.0".into());
        w.uia = Err("powershell: not found".into());
        w.session = "RDP-Tcp#3".into();
        w.elevated = true;
        p.secret_service = false;
        let cs = evaluate(&p);
        for id in ["windows", "webview2", "uia"] {
            assert_eq!(level(&cs, id), "fail", "{id}");
        }
        assert_eq!(level(&cs, "session"), "warn");
        assert!(cs.iter().find(|c| c.id == "session").unwrap().message.contains("disconnected"));
        assert_eq!(level(&cs, "elevation"), "warn");
        assert_eq!(level(&cs, "keychain"), "warn");
        assert!(!can_start(&cs));
    }

    #[test]
    fn macos_checks() {
        let p = |a: &str, s: &str, driver: bool| Probe { os: "macos".into(), secret_service: true, mac: Some(MacProbe { version: "14.6.1".into(),
            arch: "arm64".into(), accessibility: a.into(), screen_recording: s.into(), raw: String::new(), driver }), ..Default::default() };
        let cs = evaluate(&p("granted", "granted", true));
        assert!(cs.iter().all(|c| c.level == "ok"), "{cs:?}");
        let cs = evaluate(&p("denied", "unknown", true));
        assert_eq!(level(&cs, "accessibility"), "fail");
        assert!(cs.iter().find(|c| c.id == "accessibility").unwrap().message.contains("Grant permissions"));
        assert_eq!(level(&cs, "screen"), "warn");
        assert!(!can_start(&cs));
        let cs = evaluate(&p("granted", "granted", false));
        assert_eq!(level(&cs, "driver"), "fail");
        let mut old = p("granted", "granted", true);
        old.mac.as_mut().unwrap().version = "12.7".into();
        assert_eq!(level(&evaluate(&old), "macos"), "fail");
    }

    #[test]
    fn permission_json_shapes() {
        assert_eq!(permission_state(r#"{"accessibility": true, "screen_recording": false}"#, "accessibility"), "granted");
        assert_eq!(permission_state(r#"{"accessibility": true, "screen_recording": false}"#, "screen"), "denied");
        assert_eq!(permission_state(r#"{"permissions": {"Accessibility": "granted", "ScreenRecording": "denied"}}"#, "screen"), "denied");
        assert_eq!(permission_state(r#"{"accessibility": {"granted": true}}"#, "accessibility"), "granted");
        assert_eq!(permission_state(r#"{"status": "unknown"}"#, "accessibility"), "unknown");
        assert_eq!(permission_state("not json", "screen"), "unknown");
    }

    /// The exact outputs of the bundled Cua Driver 0.38.0 (`cua-driver permissions status --json`).
    #[test]
    fn permission_json_cua_driver_0_38() {
        // seen in CI (macOS 26.6.2 runner, nothing granted, no daemon), with the telemetry notice the CLI prints first
        let ci = "Cua Driver sends content-free product telemetry by default. Run `cua-driver telemetry disable` to stop it.\n{\n  \"daemon_running\": false,\n  \"reason\": \"No CuaDriver daemon is running under the driver's own identity (com.trycua.driver), so its real TCC status can't be read from this process.\",\n  \"status\": \"unknown\"\n}\n";
        assert_eq!(permission_state(ci, "accessibility"), "unknown");
        assert_eq!(permission_state(ci, "screen"), "unknown");
        // the daemon's answer (Cua source: platform-macos check_permissions structuredContent)
        let up = r#"{"accessibility": true, "screen_recording": false, "screen_recording_capturable": null, "direct_capture_status": "not_checked", "direct_capture_error": null, "source": {"attribution": "driver-daemon", "bundle_id": "com.trycua.driver"}}"#;
        assert_eq!(permission_state(up, "accessibility"), "granted");
        assert_eq!(permission_state(up, "screen"), "denied");
        let both = r#"{"accessibility": true, "screen_recording": true, "screen_recording_capturable": true}"#;
        assert_eq!(permission_state(both, "screen"), "granted");
        assert_eq!(permission_state(r#"{"daemon_running": true, "status": "unknown", "reason": "listening"}"#, "screen"), "unknown");
    }
}
