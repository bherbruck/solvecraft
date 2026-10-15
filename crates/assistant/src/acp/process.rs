//! Starting an agent program and making sure it ends when we do.

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use web_time::Instant;

use crate::settings::Secret;

/// What the agent wrote to its error output, last 8 KB, for error messages.
pub type StderrTail = Arc<Mutex<String>>;

/// Owns the child process: dropping it closes the agent's input, gives it a moment to leave, then
/// kills it. There is no unsafe code here (no parent-death signal), so a hard crash of the app
/// leaves the agent to notice that its input ended.
pub struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    /// Whether the program is still running.
    pub fn alive(&mut self) -> bool {
        self.child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    pub fn kill(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let Some(mut c) = self.child.take() else { return };
        // The caller dropped its end of stdin first (the wire), which asks it to finish.
        let until = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < until {
            if matches!(c.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = c.kill();
        let _ = c.wait();
    }
}

/// A running agent program.
pub struct Spawned {
    pub guard: ChildGuard,
    pub stdout: Box<dyn Read + Send>,
    pub stdin: Box<dyn Write + Send>,
    pub stderr: StderrTail,
}

/// Run `command args` in `cwd` with `env` added. Spawn failures say what to do.
pub fn spawn(command: &str, args: &[String], env: &[(String, Secret)], cwd: &std::path::Path) -> Result<Spawned, String> {
    let mut cmd = Command::new(command);
    cmd.args(args).current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v.expose());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => format!("`{command}` was not found. Install it, or give its full path in the assistant settings."),
        std::io::ErrorKind::PermissionDenied => format!("`{command}` cannot be run (permission denied)."),
        _ => format!("cannot start `{command}`: {e}"),
    })?;
    let stdin = child.stdin.take().ok_or("the agent has no input")?;
    let stdout = child.stdout.take().ok_or("the agent has no output")?;
    let tail: StderrTail = Arc::new(Mutex::new(String::new()));
    if let Some(mut err) = child.stderr.take() {
        let tail = tail.clone();
        let _ = std::thread::Builder::new().name("assistant-agent-stderr".into()).spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = err.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if let Ok(mut t) = tail.lock() {
                    t.push_str(&String::from_utf8_lossy(buf.get(..n).unwrap_or_default()));
                    let len = t.len();
                    if len > 8192 {
                        let mut cut = len - 8192;
                        while !t.is_char_boundary(cut) {
                            cut += 1;
                        }
                        t.drain(..cut);
                    }
                }
            }
        });
    }
    Ok(Spawned { guard: ChildGuard { child: Some(child) }, stdout: Box::new(stdout), stdin: Box::new(stdin), stderr: tail })
}

/// The last lines the program wrote to its error output, for an error message.
pub fn stderr_text(tail: &StderrTail) -> String {
    let t = tail.lock().map(|t| t.clone()).unwrap_or_default();
    let lines: Vec<&str> = t.lines().filter(|l| !l.trim().is_empty()).collect();
    let from = lines.len().saturating_sub(6);
    lines.get(from..).unwrap_or_default().join("\n")
}

/// Look for `name` next to the running program, then in the folders of `PATH`.
pub fn find_program(name: &str) -> Option<std::path::PathBuf> {
    let exe = if cfg!(windows) && !name.ends_with(".exe") { format!("{name}.exe") } else { name.to_string() };
    if let Ok(me) = std::env::current_exe()
        && let Some(dir) = me.parent()
    {
        let p = dir.join(&exe);
        if p.is_file() {
            return Some(p);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(&exe)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_program_says_so() {
        let e = spawn("solidcraft-no-such-program-xyz", &[], &[], &std::env::temp_dir()).err().unwrap();
        assert!(e.contains("was not found"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn a_program_runs_and_is_ended_with_the_guard() {
        let mut s =
            spawn("sh", &["-c".into(), "echo hello; echo oops >&2; cat".into()], &[("X_TEST".into(), Secret::new("1"))], &std::env::temp_dir())
                .unwrap();
        let mut buf = [0u8; 6];
        s.stdout.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"hello\n");
        for _ in 0..100 {
            if stderr_text(&s.stderr).contains("oops") {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(stderr_text(&s.stderr).contains("oops"));
        assert!(s.guard.alive());
        drop(s.stdin);
        let started = Instant::now();
        drop(s.guard);
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_ignores_the_end_of_input_is_killed() {
        let s = spawn("sh", &["-c".into(), "trap '' TERM; sleep 30".into()], &[], &std::env::temp_dir()).unwrap();
        let started = Instant::now();
        drop(s.stdin);
        drop(s.guard);
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    }
}
