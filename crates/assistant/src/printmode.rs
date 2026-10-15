//! Agents without ACP: run the program once per message, give it the prompt, show what it
//! prints. The program may be handed SolidCraft's MCP server through a config file
//! (`{mcp_config}`), for programs that take one.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;
use web_time::Instant;

use serde_json::json;

use crate::acp::session::McpServer;
use crate::model::Usage;
use crate::net::Cancel;
use crate::settings::AgentProfile;
use crate::tool_loop::{Event, Finish, TurnOutcome};

/// Most output kept from one run.
const MAX_OUTPUT: usize = 256 * 1024;

/// A run may take at most this long.
const TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Remove terminal control sequences (colours, cursor moves) from `s`.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for n in chars.by_ref() {
                        if ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    for n in chars.by_ref() {
                        if n == '\u{7}' || n == '\u{1b}' {
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}

/// The program's arguments with `{prompt}` and `{mcp_config}` filled in; whether the prompt
/// went into an argument (otherwise it is written to the program's input).
pub fn build_args(profile: &AgentProfile, prompt: &str, mcp_config_path: Option<&str>) -> (Vec<String>, bool) {
    let mut in_args = false;
    let args = profile
        .args
        .iter()
        .map(|a| {
            if a.contains("{prompt}") {
                in_args = true;
            }
            a.replace("{prompt}", prompt).replace("{mcp_config}", mcp_config_path.unwrap_or(""))
        })
        .collect();
    (args, in_args)
}

/// The JSON file a program takes with `{mcp_config}`: `{"mcpServers": {name: {command, args, env}}}`.
pub fn mcp_config_json(m: &McpServer) -> serde_json::Value {
    let env: serde_json::Map<String, serde_json::Value> = m.env.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    json!({"mcpServers": {m.name.clone(): {"command": m.command, "args": m.args, "env": env}}})
}

/// Run `profile` for one prompt in `cwd`.
pub fn run(
    profile: &AgentProfile,
    prompt: &str,
    mcp: Option<&McpServer>,
    cwd: &std::path::Path,
    cancel: &Cancel,
    on_event: &mut dyn FnMut(Event),
) -> TurnOutcome {
    let mut out = TurnOutcome { finish: Finish::Done, usage: Usage::default(), steps: 1, tool_calls: 0 };
    let mut config_file: Option<std::path::PathBuf> = None;
    let wants_config = profile.args.iter().any(|a| a.contains("{mcp_config}"));
    if wants_config && let Some(m) = mcp {
        let path = cwd.join(format!(".solidcraft-mcp-{}.json", std::process::id()));
        let text = mcp_config_json(m).to_string();
        match solidcraft_engine::control_auth::write_private_file(&path, text.as_bytes()) {
            Ok(()) => config_file = Some(path),
            Err(e) => {
                out.finish = Finish::Error(format!("cannot write the MCP configuration: {e}"));
                return out;
            }
        }
    }
    let result = run_inner(profile, prompt, config_file.as_deref().and_then(|p| p.to_str()), cwd, cancel, on_event);
    if let Some(p) = config_file {
        let _ = std::fs::remove_file(p);
    }
    out.finish = result;
    out
}

fn run_inner(
    profile: &AgentProfile,
    prompt: &str,
    config: Option<&str>,
    cwd: &std::path::Path,
    cancel: &Cancel,
    on_event: &mut dyn FnMut(Event),
) -> Finish {
    let (args, in_args) = build_args(profile, prompt, config);
    let mut cmd = Command::new(&profile.command);
    cmd.args(&args).current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in &profile.env {
        cmd.env(k, v.expose());
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Finish::Error(format!("`{}` was not found. Install it, or give its full path in the assistant settings.", profile.command));
        }
        Err(e) => return Finish::Error(format!("cannot start `{}`: {e}", profile.command)),
    };
    // The prompt goes to the program's input unless an argument carries it; closing the input
    // tells the program it has everything.
    if let Some(mut stdin) = child.stdin.take()
        && !in_args
    {
        let _ = stdin.write_all(prompt.as_bytes());
    }
    let (Some(mut stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        return Finish::Error("the program has no output".into());
    };
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 4096];
        while let Ok(n) = stdout.read(&mut buf) {
            if n == 0 || tx.send(buf.get(..n).unwrap_or_default().to_vec()).is_err() {
                break;
            }
        }
    });
    let (etx, erx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.by_ref().take(16 * 1024).read_to_string(&mut text);
        let _ = etx.send(text);
    });
    let started = Instant::now();
    let mut total = 0usize;
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let stop = if cancel.is_cancelled() {
            Some(Finish::Cancelled)
        } else if started.elapsed() > TIMEOUT {
            Some(Finish::Error("The program took too long and was stopped.".into()))
        } else {
            None
        };
        if let Some(finish) = stop {
            let _ = child.kill();
            let _ = child.wait();
            return finish;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(bytes) => {
                total += bytes.len();
                if total > MAX_OUTPUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Finish::Error("The program wrote too much.".into());
                }
                pending.extend_from_slice(&bytes);
                // Only whole characters go out.
                let valid = match std::str::from_utf8(&pending) {
                    Ok(s) => s.len(),
                    Err(e) => e.valid_up_to(),
                };
                let text = String::from_utf8_lossy(pending.get(..valid).unwrap_or_default()).into_owned();
                pending.drain(..valid);
                let clean = strip_ansi(&text);
                if !clean.is_empty() {
                    on_event(Event::Text(clean));
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                let status = child.wait();
                let err_text = erx.recv_timeout(Duration::from_secs(1)).unwrap_or_default();
                return match status {
                    Ok(s) if s.success() => Finish::Done,
                    Ok(s) if err_text.trim().is_empty() => Finish::Error(format!("The program ended with {s}.")),
                    Ok(s) => Finish::Error(format!("The program ended with {s}.\n{}", strip_ansi(err_text.trim()))),
                    Err(e) => Finish::Error(e.to_string()),
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{AgentMode, Secret};

    #[test]
    fn terminal_codes_are_removed() {
        assert_eq!(strip_ansi("\u{1b}[32mgreen\u{1b}[0m and \u{1b}]0;title\u{7}plain\r\n"), "green and plain\n");
        assert_eq!(strip_ansi("ä\u{1b}"), "ä");
    }

    #[test]
    fn placeholders_are_filled() {
        let p = AgentProfile {
            name: "a".into(),
            command: "agy".into(),
            args: vec!["-p".into(), "{prompt}".into(), "--mcp-config".into(), "{mcp_config}".into()],
            env: vec![],
            mode: AgentMode::Print,
        };
        let (args, in_args) = build_args(&p, "make a box", Some("/tmp/c.json"));
        assert_eq!(args, ["-p", "make a box", "--mcp-config", "/tmp/c.json"]);
        assert!(in_args);
        let p = AgentProfile { args: vec![], ..p };
        assert_eq!(build_args(&p, "x", None), (vec![], false));
    }

    #[test]
    fn the_mcp_config_names_the_server() {
        let m = McpServer { name: "solidcraft".into(), command: "/b/cli".into(), args: vec!["mcp".into()], env: vec![("T".into(), "x".into())] };
        let v = mcp_config_json(&m);
        assert_eq!(v["mcpServers"]["solidcraft"]["command"], "/b/cli");
        assert_eq!(v["mcpServers"]["solidcraft"]["env"]["T"], "x");
    }

    #[cfg(unix)]
    #[test]
    fn a_program_runs_and_its_output_streams() {
        let p = AgentProfile {
            name: "echo".into(),
            command: "sh".into(),
            args: vec!["-c".into(), "printf '\\033[1mhello\\033[0m '; echo \"$1\"".into(), "sh".into(), "{prompt}".into()],
            env: vec![("A".into(), Secret::new("b"))],
            mode: AgentMode::Print,
        };
        let mut text = String::new();
        let out = run(&p, "a box", None, &std::env::temp_dir(), &Cancel::new(), &mut |e| {
            if let Event::Text(t) = e {
                text.push_str(&t);
            }
        });
        assert_eq!(out.finish, Finish::Done, "{text}");
        assert_eq!(text.trim(), "hello a box");
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_program_and_a_stop() {
        let p = AgentProfile {
            name: "f".into(),
            command: "sh".into(),
            args: vec!["-c".into(), "echo bad >&2; exit 3".into()],
            env: vec![],
            mode: AgentMode::Print,
        };
        let out = run(&p, "x", None, &std::env::temp_dir(), &Cancel::new(), &mut |_| {});
        match out.finish {
            Finish::Error(m) => assert!(m.contains("bad"), "{m}"),
            other => panic!("{other:?}"),
        }
        let p = AgentProfile { args: vec!["-c".into(), "sleep 30".into()], ..p };
        let cancel = Cancel::new();
        let c = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            c.cancel();
        });
        let started = Instant::now();
        let out = run(&p, "x", None, &std::env::temp_dir(), &cancel, &mut |_| {});
        assert_eq!(out.finish, Finish::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(5));
        let p = AgentProfile { command: "solidcraft-no-such-program".into(), ..p };
        assert!(matches!(run(&p, "x", None, &std::env::temp_dir(), &Cancel::new(), &mut |_| {}).finish, Finish::Error(m) if m.contains("not found")));
    }
}
