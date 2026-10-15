//! Running a turn on a worker thread, so the window keeps drawing while the model answers and
//! tools run. The worker reports through a channel the UI drains once per frame.

use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::channel;
use std::sync::mpsc::{Receiver, Sender};

#[cfg(not(target_arch = "wasm32"))]
use crate::acp::process;
#[cfg(not(target_arch = "wasm32"))]
use crate::acp::protocol::Wire;
use crate::acp::session::{AgentSession, McpServer, PermissionRequest};
#[cfg(not(target_arch = "wasm32"))]
use crate::model::Usage;
use crate::model::{Message, ToolDef};
use crate::net::{Cancel, HttpStream};
use crate::provider::Codec;
#[cfg(not(target_arch = "wasm32"))]
use crate::settings::AgentMode;
use crate::settings::{AgentProfile, Secret};
use crate::tool_loop::{Event, ToolHost, TurnOutcome};
#[cfg(not(target_arch = "wasm32"))]
use crate::tool_loop::{Finish, TurnConfig, run_turn};

/// Everything one turn needs, owned, so it can move to the worker.
pub struct Job {
    pub codec: Arc<dyn Codec>,
    pub http: HttpStream,
    pub base_url: String,
    pub allow_http: bool,
    pub key: Secret,
    pub model: String,
    pub system: String,
    pub tools: Vec<ToolDef>,
    pub vision: bool,
    pub max_tokens: u32,
    pub max_steps: usize,
    pub context_window: u32,
    pub folders: Vec<std::path::PathBuf>,
    pub history: Vec<Message>,
    pub user: Message,
    pub host: Arc<dyn ToolHost>,
    /// Asks the window for a frame (the worker calls it after every message).
    pub repaint: Arc<dyn Fn() + Send + Sync>,
}

/// What the worker sends.
pub enum WorkerMsg {
    Event(Event),
    /// The turn is over; `history` is the conversation as the model has it now.
    Done {
        outcome: TurnOutcome,
        history: Vec<Message>,
    },
    /// An agent turn is over; `session` is kept for the next message while it is usable.
    AgentDone {
        outcome: TurnOutcome,
        session: Option<AgentSession>,
    },
    /// The agent asks whether it may do something; answer on `reply` (`None` declines).
    Permission {
        request: PermissionRequest,
        reply: Sender<Option<String>>,
    },
}

/// A running turn.
pub struct TurnHandle {
    pub rx: Receiver<WorkerMsg>,
    pub cancel: Cancel,
}

/// Start `job` on its own thread.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_turn(job: Job, cancel: Cancel) -> Result<TurnHandle, String> {
    let (tx, rx) = channel::<WorkerMsg>();
    let worker_cancel = cancel.clone();
    std::thread::Builder::new()
        .name("assistant-turn".into())
        .spawn(move || work(job, worker_cancel, tx))
        .map_err(|e| format!("cannot start the assistant: {e}"))?;
    Ok(TurnHandle { rx, cancel })
}

#[cfg(target_arch = "wasm32")]
pub fn spawn_turn(_job: Job, _cancel: Cancel) -> Result<TurnHandle, String> {
    Err("the assistant needs the desktop app".into())
}

#[cfg(not(target_arch = "wasm32"))]
fn work(job: Job, cancel: Cancel, tx: Sender<WorkerMsg>) {
    let Job {
        codec,
        http,
        base_url,
        allow_http,
        key,
        model,
        system,
        tools,
        vision,
        max_tokens,
        max_steps,
        context_window,
        folders,
        mut history,
        user,
        host,
        repaint,
    } = job;
    let send = |m: WorkerMsg| {
        let _ = tx.send(m);
        repaint();
    };
    // A bug in a codec or a tool must end the turn with a message, not take the app down.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let cfg = TurnConfig {
            codec: codec.as_ref(),
            http: &http,
            base_url: &base_url,
            allow_http,
            key: &key,
            model: &model,
            system: &system,
            tools: &tools,
            vision,
            max_tokens,
            max_steps,
            context_window,
            folders: &folders,
        };
        run_turn(&cfg, &mut history, user, host.as_ref(), &cancel, &mut |e| send(WorkerMsg::Event(e)))
    }))
    .unwrap_or_else(|_| TurnOutcome {
        finish: Finish::Error("The assistant hit an internal error and stopped.".into()),
        usage: Usage::default(),
        steps: 0,
        tool_calls: 0,
    });
    send(WorkerMsg::Done { outcome, history });
}

/// The folders (as strings) an assistant's tool calls may use: the app's recovery folder, the
/// private scratch folder for pictures, and `folders`. The app puts them in every request's
/// `allow_paths`.
pub fn agent_roots(folders: &[String]) -> Vec<String> {
    let scope = solidcraft_mcp::agent_scope(folders.iter().map(std::path::PathBuf::from));
    scope.roots().iter().map(|p| p.to_string_lossy().into_owned()).collect()
}

/// How to reach the app's agent endpoint, and the program that bridges to it.
#[derive(Clone, Debug)]
pub struct Bridge {
    pub cli: std::path::PathBuf,
    pub port: u16,
    pub token: String,
}

impl Bridge {
    /// The MCP server an agent is asked to start: `solidcraft-cli mcp --connect HOST:PORT`, with
    /// the token in its environment (arguments are visible to other users).
    pub fn mcp_server(&self) -> McpServer {
        McpServer {
            name: "solidcraft".into(),
            command: self.cli.to_string_lossy().into_owned(),
            args: vec!["mcp".into(), "--connect".into(), format!("127.0.0.1:{}", self.port)],
            env: vec![("SOLIDCRAFT_CONTROL_TOKEN".into(), self.token.clone())],
        }
    }
}

/// One message for an external agent.
pub struct AgentJob {
    pub profile: AgentProfile,
    pub text: String,
    pub picture: Option<(String, String)>,
    pub bridge: Option<Bridge>,
    pub cwd: std::path::PathBuf,
    pub auto_allow: bool,
    /// The agent from the previous message, if it is still running.
    pub session: Option<AgentSession>,
    pub repaint: Arc<dyn Fn() + Send + Sync>,
}

/// Run one message through an external agent on its own thread.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_agent_turn(job: AgentJob, cancel: Cancel) -> Result<TurnHandle, String> {
    let (tx, rx) = channel::<WorkerMsg>();
    let worker_cancel = cancel.clone();
    std::thread::Builder::new()
        .name("assistant-agent-turn".into())
        .spawn(move || work_agent(job, worker_cancel, tx))
        .map_err(|e| format!("cannot start the assistant: {e}"))?;
    Ok(TurnHandle { rx, cancel })
}

#[cfg(target_arch = "wasm32")]
pub fn spawn_agent_turn(_job: AgentJob, _cancel: Cancel) -> Result<TurnHandle, String> {
    Err("the assistant needs the desktop app".into())
}

#[cfg(not(target_arch = "wasm32"))]
fn work_agent(mut job: AgentJob, cancel: Cancel, tx: Sender<WorkerMsg>) {
    let send = |m: WorkerMsg| {
        let _ = tx.send(m);
        (job.repaint)();
    };
    let session = job.session.take();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| agent_turn(&job, session, &cancel, &tx, &send)));
    match result {
        Ok((outcome, session)) => send(WorkerMsg::AgentDone { outcome, session }),
        Err(_) => send(WorkerMsg::AgentDone {
            outcome: TurnOutcome {
                finish: Finish::Error("The assistant hit an internal error and stopped.".into()),
                usage: Usage::default(),
                steps: 0,
                tool_calls: 0,
            },
            session: None,
        }),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn agent_turn(
    job: &AgentJob,
    running: Option<AgentSession>,
    cancel: &Cancel,
    tx: &Sender<WorkerMsg>,
    send: &dyn Fn(WorkerMsg),
) -> (TurnOutcome, Option<AgentSession>) {
    let err = |m: String| TurnOutcome { finish: Finish::Error(m), usage: Usage::default(), steps: 0, tool_calls: 0 };
    let mcp = job.bridge.as_ref().map(Bridge::mcp_server);
    let mut on_event = |e: Event| send(WorkerMsg::Event(e));
    if job.profile.mode == AgentMode::Print {
        let out = crate::printmode::run(&job.profile, &job.text, mcp.as_ref(), &job.cwd, cancel, &mut on_event);
        return (out, None);
    }
    // Reuse the agent from the last message while it is running; otherwise start one.
    let mut session = match running {
        Some(s) => s,
        None => match start_agent(job, mcp.as_ref(), cancel) {
            Ok(s) => s,
            Err(e) => return (err(e), None),
        },
    };
    let mut ask = |req: &PermissionRequest| -> Option<String> {
        let (rtx, rrx) = channel::<Option<String>>();
        if tx.send(WorkerMsg::Permission { request: req.clone(), reply: rtx }).is_err() {
            return None;
        }
        (job.repaint)();
        loop {
            match rrx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(answer) => return answer,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if cancel.is_cancelled() {
                        return None;
                    }
                }
                Err(_) => return None,
            }
        }
    };
    let picture = job.picture.as_ref().map(|(m, d)| (m.as_str(), d.as_str()));
    let outcome = session.prompt(&job.text, picture, cancel, job.auto_allow, &mut ask, &mut on_event);
    let keep = session.usable();
    (outcome, keep.then_some(session))
}

#[cfg(not(target_arch = "wasm32"))]
fn start_agent(job: &AgentJob, mcp: Option<&McpServer>, cancel: &Cancel) -> Result<AgentSession, String> {
    let spawned = process::spawn(&job.profile.command, &job.profile.args, &job.profile.env, &job.cwd)?;
    let wire = Wire::new(spawned.stdout, spawned.stdin);
    AgentSession::start(wire, Some(spawned.guard), spawned.stderr, &job.cwd.to_string_lossy(), mcp, cancel)
}

/// The folders an agent's requests may name: the app's `own` roots, plus the private scratch
/// folders of SolidCraft's MCP server (`<temp>/solidcraft-mcp-<pid>-<n>`), where it renders the
/// pictures it reads back. Anything else a request claims is dropped.
pub fn merge_roots(own: &[String], requested: Option<&serde_json::Value>, temp: &std::path::Path) -> Vec<String> {
    let mut out: Vec<String> = own.to_vec();
    let Some(list) = requested.and_then(|v| v.as_array()) else { return out };
    for r in list.iter().take(64).filter_map(|v| v.as_str()) {
        let p = std::path::Path::new(r);
        let ours = p.parent() == Some(temp)
            && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.strip_prefix("solidcraft-mcp-").is_some_and(|rest| {
                    let mut it = rest.split('-');
                    matches!((it.next(), it.next(), it.next()), (Some(pid), Some(n), None) if !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()) && !n.is_empty() && n.bytes().all(|b| b.is_ascii_hexdigit()))
                })
            });
        if ours && !out.iter().any(|o| o == r) {
            out.push(r.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_the_mcp_servers_own_scratch_folder_is_taken_from_a_request() {
        let temp = std::path::Path::new("/tmp");
        let own = vec!["/home/me/recovery".to_string()];
        let req = json!([
            "/tmp/solidcraft-mcp-4242-1a2b",
            "/",
            "/home/me/.ssh",
            "/tmp/solidcraft-mcp-x-1",
            "/tmp/solidcraft-mcp-12-zz",
            "/tmp/other/solidcraft-mcp-1-2",
            "/tmp/solidcraft-mcp-1-2-3",
            5
        ]);
        let roots = merge_roots(&own, Some(&req), temp);
        assert_eq!(roots, ["/home/me/recovery", "/tmp/solidcraft-mcp-4242-1a2b"]);
        assert_eq!(merge_roots(&own, None, temp), own);
        assert_eq!(merge_roots(&own, Some(&json!("x")), temp), own);
    }

    #[test]
    fn the_bridge_keeps_the_token_out_of_the_arguments() {
        let b = Bridge { cli: "/b/solidcraft-cli".into(), port: 4711, token: "secret-token".into() };
        let m = b.mcp_server();
        assert_eq!(m.args, ["mcp", "--connect", "127.0.0.1:4711"]);
        assert!(!m.args.iter().any(|a| a.contains("secret")));
        assert_eq!(m.env, [("SOLIDCRAFT_CONTROL_TOKEN".to_string(), "secret-token".to_string())]);
    }
}
