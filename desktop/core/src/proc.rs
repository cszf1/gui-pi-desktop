//! The agent process (Pi in RPC mode) under supervision, free of GUI crates so it is testable with a fake child.
//!
//! - started with an explicit environment (`env_clear` + the launch's allowlisted variables, see launch::child_env)
//! - stdout is read continuously and split on LF only (rpc::split_records); every record goes to the callback
//! - the end is classified exactly once:
//!     Finished      Pi settled and we closed its stdin (or it exited 0 after settling)
//!     Stopped       the user pressed Stop / the app quit
//!     Crashed       Pi exited on its own before settling (any exit code), or could not be written to
//!     Disconnected  Pi closed its stdout but kept running; it is killed after a grace period
//! - stderr is kept (last lines) for the crash report and scanned for the extension's gate-exit line
//!
//! The shell turns `Exited` into: cancel every open person question, tell the main window, offer a restart.
use crate::rpc;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    Finished,
    Stopped,
    Crashed,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    /// one RPC record from Pi's stdout
    Line(String),
    /// one line of Pi's stderr
    Stderr(String),
    /// the process is gone (sent exactly once, always last)
    Exited { code: Option<i32>, reason: ExitReason, stderr_tail: String },
}

pub type OnEvent = Arc<dyn Fn(AgentEvent) + Send + Sync>;

/// How long a child that closed its stdout may keep running before it is killed as disconnected.
pub const DISCONNECT_GRACE: Duration = Duration::from_secs(3);
/// How long Stop / Finish waits for an orderly exit after closing stdin before killing.
pub const STOP_GRACE: Duration = Duration::from_secs(5);

pub struct Agent {
    child: Arc<Mutex<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    stop_requested: Arc<AtomicBool>,
    settled: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
}

/// Start `program args` with exactly `env` (nothing else inherited) and supervise it.
pub fn spawn(program: &Path, args: &[String], env: &[(String, String)], cwd: Option<&Path>, on_event: OnEvent) -> std::io::Result<Agent> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).env_clear();
    cmd.envs(env.iter().map(|(k, v)| (k, v)));
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take();
    let mut stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let child = Arc::new(Mutex::new(child));
    let stop_requested = Arc::new(AtomicBool::new(false));
    let settled = Arc::new(AtomicBool::new(false));
    let exited = Arc::new(AtomicBool::new(false));
    let tail: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));

    let (t2, ev2) = (tail.clone(), on_event.clone());
    let (err_done_tx, err_done) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let _done = err_done_tx; // dropped when this thread ends
        let mut buf = String::new();
        let mut rd = stderr;
        let mut chunk = [0u8; 8192];
        loop {
            let n = match rd.read(&mut chunk) { Ok(0) | Err(_) => break, Ok(n) => n };
            buf.push_str(&String::from_utf8_lossy(&chunk[..n]));
            while let Some(i) = buf.find('\n') {
                let line: String = buf.drain(..=i).collect();
                let line = line.trim_end().to_string();
                let mut t = t2.lock().unwrap_or_else(|e| e.into_inner());
                t.push_back(line.clone());
                if t.len() > 40 {
                    t.pop_front();
                }
                drop(t);
                ev2(AgentEvent::Stderr(line));
            }
        }
        if !buf.trim().is_empty() {
            t2.lock().unwrap_or_else(|e| e.into_inner()).push_back(buf.trim_end().to_string());
            ev2(AgentEvent::Stderr(buf.trim_end().to_string()));
        }
    });

    let (c2, stop2, settled2, exited2) = (child.clone(), stop_requested.clone(), settled.clone(), exited.clone());
    std::thread::spawn(move || {
        let mut buf = String::new();
        let mut chunk = [0u8; 65536];
        loop {
            let n = match stdout.read(&mut chunk) { Ok(0) | Err(_) => break, Ok(n) => n };
            buf.push_str(&String::from_utf8_lossy(&chunk[..n]));
            for line in rpc::split_records(&mut buf) {
                if line.contains("\"agent_settled\"") && rpc::ui_event(&line).map(|e| e.kind == "done").unwrap_or(false) {
                    settled2.store(true, Ordering::SeqCst);
                }
                on_event(AgentEvent::Line(line));
            }
        }
        // stdout is closed: wait for the process; still running after the grace period = disconnected
        let deadline = Instant::now() + if stop2.load(Ordering::SeqCst) { STOP_GRACE } else { DISCONNECT_GRACE };
        let code;
        let mut killed = false;
        loop {
            let st = c2.lock().unwrap_or_else(|e| e.into_inner()).try_wait();
            match st {
                Ok(Some(s)) => {
                    code = s.code();
                    break;
                }
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
                _ => {
                    let mut c = c2.lock().unwrap_or_else(|e| e.into_inner());
                    let _ = c.kill();
                    code = c.wait().ok().and_then(|s| s.code());
                    killed = true;
                    break;
                }
            }
        }
        // a grandchild may still hold stderr open: wait a little for the last lines, never forever
        let _ = err_done.recv_timeout(Duration::from_secs(1));
        let reason = if stop2.load(Ordering::SeqCst) {
            if settled2.load(Ordering::SeqCst) { ExitReason::Finished } else { ExitReason::Stopped }
        } else if killed {
            ExitReason::Disconnected
        } else if settled2.load(Ordering::SeqCst) && code == Some(0) {
            ExitReason::Finished
        } else {
            ExitReason::Crashed
        };
        let stderr_tail = tail.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect::<Vec<_>>().join("\n");
        exited2.store(true, Ordering::SeqCst);
        on_event(AgentEvent::Exited { code, reason, stderr_tail });
    });

    Ok(Agent { child, stdin: Mutex::new(stdin), stop_requested, settled, exited })
}

impl Agent {
    /// Write one RPC record. An error means Pi can no longer be reached (it is treated as gone).
    pub fn send(&self, line: &str) -> std::io::Result<()> {
        let mut g = self.stdin.lock().unwrap_or_else(|e| e.into_inner());
        let w = g.as_mut().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "agent stdin is closed"))?;
        let r = writeln!(w, "{line}").and_then(|_| w.flush());
        if r.is_err() {
            *g = None;
            // unreachable agent: end it so the supervisor reports the exit
            let _ = self.child.lock().unwrap_or_else(|e| e.into_inner()).kill();
        }
        r
    }

    /// Orderly end: close stdin (Pi disposes its runtime; the extension ends gui-gate, which ends the driver).
    /// The supervisor kills it if it is still there after STOP_GRACE. `user` marks a Stop (vs. a normal finish).
    pub fn stop(&self, user: bool) {
        self.stop_requested.store(true, Ordering::SeqCst);
        if user {
            self.settled.store(false, Ordering::SeqCst);
        }
        *self.stdin.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let child = self.child.clone();
        let exited = self.exited.clone();
        std::thread::spawn(move || {
            let deadline = Instant::now() + STOP_GRACE;
            while Instant::now() < deadline {
                if exited.load(Ordering::SeqCst) || matches!(child.lock().unwrap_or_else(|e| e.into_inner()).try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = child.lock().unwrap_or_else(|e| e.into_inner()).kill();
        });
    }

    pub fn is_settled(&self) -> bool {
        self.settled.load(Ordering::SeqCst)
    }
    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }
    pub fn pid(&self) -> u32 {
        self.child.lock().unwrap_or_else(|e| e.into_inner()).id()
    }
}

/// What the main window shows when the agent is gone, after every open person question was cancelled.
pub fn exit_event(code: Option<i32>, reason: ExitReason, stderr_tail: &str, cancelled_questions: usize) -> rpc::UiEvent {
    let q = if cancelled_questions > 0 { format!(" {cancelled_questions} open question(s) to the person were cancelled.") } else { String::new() };
    let tail: String = stderr_tail.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    match reason {
        ExitReason::Finished => rpc::UiEvent::new("stopped", format!("finished.{q}")),
        ExitReason::Stopped => rpc::UiEvent::new("stopped", format!("stopped.{q}")),
        ExitReason::Crashed | ExitReason::Disconnected => {
            let what = if reason == ExitReason::Crashed {
                format!("The agent process ended unexpectedly (exit code {}).", code.map(|c| c.to_string()).unwrap_or_else(|| "none - killed by a signal".into()))
            } else {
                "The agent process stopped talking to the app and was ended.".to_string()
            };
            let mut e = rpc::UiEvent::new("crashed", format!("{what}{q} Nothing more will run. You can restart the task.{}", if tail.is_empty() { String::new() } else { format!("\n{tail}") }));
            e.restart = true;
            e
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::human::{Outcome, Pending};
    use std::sync::mpsc::{channel, Receiver};

    fn sh(script: &str) -> (Agent, Receiver<AgentEvent>) {
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let on: OnEvent = Arc::new(move |e| {
            let _ = tx.lock().unwrap().send(e);
        });
        let a = spawn(Path::new("/bin/sh"), &["-c".into(), script.into()], &[("PATH".into(), "/usr/bin:/bin".into()), ("ONLY".into(), "this".into())], None, on).unwrap();
        (a, rx)
    }

    fn until_exit(rx: &Receiver<AgentEvent>) -> (Vec<AgentEvent>, AgentEvent) {
        let mut seen = vec![];
        loop {
            let e = rx.recv_timeout(Duration::from_secs(15)).expect("no exit event");
            if matches!(e, AgentEvent::Exited { .. }) {
                return (seen, e);
            }
            seen.push(e);
        }
    }

    #[test]
    fn normal_finish_after_settled() {
        let (a, rx) = sh(r#"echo '{"type":"agent_settled","aborted":false}'; while read l; do :; done; exit 0"#);
        // the shell closes stdin when it sees the run settle
        loop {
            if let AgentEvent::Line(l) = rx.recv_timeout(Duration::from_secs(10)).unwrap() {
                assert!(l.contains("agent_settled"));
                break;
            }
        }
        assert!(a.is_settled());
        a.stop(false);
        let (_, e) = until_exit(&rx);
        assert!(matches!(e, AgentEvent::Exited { reason: ExitReason::Finished, code: Some(0), .. }), "{e:?}");
    }

    #[test]
    fn crash_is_reported_with_stderr_and_cancels_open_questions() {
        let (a, rx) = sh(r#"echo '{"type":"turn_start"}'; echo 'boom: out of memory' >&2; exit 3"#);
        let pending = Pending::default();
        let q = pending.open("q1").unwrap();
        let (seen, e) = until_exit(&rx);
        assert!(seen.iter().any(|x| matches!(x, AgentEvent::Line(l) if l.contains("turn_start"))));
        let AgentEvent::Exited { code, reason, stderr_tail } = e else { unreachable!() };
        assert_eq!((code, reason), (Some(3), ExitReason::Crashed));
        assert!(stderr_tail.contains("boom"));
        // what the shell does on Exited
        let n = pending.cancel_all();
        assert_eq!(pending.wait("q1", q, 1.0), Outcome::Cancelled);
        let ui = exit_event(code, reason, &stderr_tail, n);
        assert_eq!(ui.kind, "crashed");
        assert!(ui.restart && ui.text.contains("exit code 3") && ui.text.contains("1 open question") && ui.text.contains("boom"));
        assert!(a.send("{}").is_err(), "writing to a dead agent fails");
    }

    #[test]
    fn exit_zero_without_settling_is_still_a_crash() {
        let (_a, rx) = sh("exit 0");
        let (_, e) = until_exit(&rx);
        assert!(matches!(e, AgentEvent::Exited { reason: ExitReason::Crashed, code: Some(0), .. }), "{e:?}");
    }

    #[test]
    fn closed_stdout_while_alive_is_a_disconnect() {
        let (a, rx) = sh("exec 1>&-; sleep 30; exit 0");
        let t0 = Instant::now();
        let (_, e) = until_exit(&rx);
        assert!(matches!(e, AgentEvent::Exited { reason: ExitReason::Disconnected, .. }), "{e:?}");
        assert!(t0.elapsed() < Duration::from_secs(10));
        assert!(a.has_exited());
        assert!(exit_event(None, ExitReason::Disconnected, "", 0).restart);
    }

    #[test]
    fn stop_ends_a_busy_agent() {
        let (a, rx) = sh("trap '' TERM; while true; do sleep 1; done");
        std::thread::sleep(Duration::from_millis(100));
        a.stop(true);
        let (_, e) = until_exit(&rx);
        assert!(matches!(e, AgentEvent::Exited { reason: ExitReason::Stopped, .. }), "{e:?}");
        assert!(!exit_event(None, ExitReason::Stopped, "", 0).restart);
    }

    #[test]
    fn gate_exit_line_on_stderr_and_clean_environment() {
        let (_a, rx) = sh(r#"echo "[gui-pi] gui-gate exited (code 101); no GUI action can run." >&2; echo "env:$(env | grep -c .):$ONLY:${HOME:-none}"; exit 1"#);
        let (seen, _) = until_exit(&rx);
        assert!(seen.iter().any(|x| matches!(x, AgentEvent::Stderr(l) if rpc::stderr_event(l).map(|e| e.kind == "gate_exit").unwrap_or(false))));
        // only what we passed (PATH, ONLY; sh may add PWD/SHLVL/_), nothing of the test runner's environment
        let env_line = seen.iter().find_map(|x| if let AgentEvent::Line(l) = x { Some(l.clone()) } else { None }).unwrap();
        assert!(env_line.contains(":this:none"), "{env_line}");
        let n: usize = env_line.split(':').nth(1).unwrap().parse().unwrap();
        assert!(n <= 5, "{env_line}");
    }
}
