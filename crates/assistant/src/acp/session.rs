//! One conversation with an agent over ACP: the handshake, then one `session/prompt` per
//! message with its streamed updates, permission requests and cancellation.

use std::collections::HashMap;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;
use web_time::Instant;

use serde_json::{Value, json};

use super::process::{ChildGuard, StderrTail, stderr_text};
use super::protocol::{Incoming, Wire};
use crate::model::Usage;
use crate::net::Cancel;
use crate::tool_loop::{Event, Finish, TurnOutcome};

/// How long the handshake may take (agents started through `npx` download first).
const START_TIMEOUT: Duration = Duration::from_secs(120);

/// How long a turn may last at most.
pub const TURN_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// How long the agent may take to answer a cancel before it is given up on.
const CANCEL_GRACE: Duration = Duration::from_secs(5);

/// Longest text kept from one tool call's output.
const MAX_TOOL_TEXT: usize = 32 * 1024;

/// The MCP server the agent is asked to start.
#[derive(Clone, Debug)]
pub struct McpServer {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// A question from the agent: may it run this?
#[derive(Clone, Debug, PartialEq)]
pub struct PermissionRequest {
    pub title: String,
    pub kind: String,
    pub raw_input: Value,
    pub options: Vec<PermissionOption>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionOption {
    pub id: String,
    pub name: String,
    /// `allow_once`, `allow_always`, `reject_once` or `reject_always`.
    pub kind: String,
}

impl PermissionRequest {
    /// Whether this is one of SolidCraft's own tools (the MCP server we started).
    pub fn is_solidcraft_tool(&self) -> bool {
        let hay = format!("{} {} {}", self.title, self.kind, self.raw_input).to_ascii_lowercase();
        hay.contains("solidcraft")
    }

    fn allow_once(&self) -> Option<&PermissionOption> {
        self.options.iter().find(|o| o.kind == "allow_once")
    }
}

/// A started agent.
pub struct AgentSession {
    wire: Wire,
    pub session_id: String,
    pub stderr: StderrTail,
    /// Dropped last: it ends the program.
    guard: Option<ChildGuard>,
    /// A cancel that was not answered: the session is not usable any more.
    broken: bool,
}

impl AgentSession {
    /// Run the handshake on a started program: `initialize`, then `session/new` with `mcp`.
    pub fn start(
        mut wire: Wire,
        guard: Option<ChildGuard>,
        stderr: StderrTail,
        cwd: &str,
        mcp: Option<&McpServer>,
        cancel: &Cancel,
    ) -> Result<AgentSession, String> {
        let init = wire.request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
                "clientInfo": {"name": "solidcraft", "title": "SolidCraft", "version": env!("CARGO_PKG_VERSION")},
            }),
        )?;
        let reply = await_response(&wire, &init, cancel, START_TIMEOUT, &stderr)?;
        let auth = reply.get("authMethods").and_then(Value::as_array).map(|a| !a.is_empty()).unwrap_or(false);
        let servers: Vec<Value> = mcp
            .map(|m| {
                json!({
                    "name": m.name,
                    "command": m.command,
                    "args": m.args,
                    "env": m.env.iter().map(|(k, v)| json!({"name": k, "value": v})).collect::<Vec<_>>(),
                })
            })
            .into_iter()
            .collect();
        let new = wire.request("session/new", json!({"cwd": cwd, "mcpServers": servers}))?;
        let reply = match await_response(&wire, &new, cancel, START_TIMEOUT, &stderr) {
            Ok(r) => r,
            Err(e) if auth => return Err(format!("{e} (the agent asks you to sign in first; do that in its own program, then try again)")),
            Err(e) => return Err(e),
        };
        let session_id = reply.get("sessionId").and_then(Value::as_str).ok_or("the agent did not give a session")?.to_string();
        Ok(AgentSession { wire, session_id, stderr, guard, broken: false })
    }

    /// Whether the program is still there and answering.
    pub fn usable(&mut self) -> bool {
        !self.broken && self.guard.as_mut().is_none_or(ChildGuard::alive)
    }

    /// Send one message and follow the turn until the agent is done. `permission` is asked when
    /// the agent wants to run something that is not allowed automatically; `None` declines.
    pub fn prompt(
        &mut self,
        text: &str,
        picture: Option<(&str, &str)>,
        cancel: &Cancel,
        auto_allow_solidcraft: bool,
        permission: &mut dyn FnMut(&PermissionRequest) -> Option<String>,
        on_event: &mut dyn FnMut(Event),
    ) -> TurnOutcome {
        let mut out = TurnOutcome { finish: Finish::Done, usage: Usage::default(), steps: 1, tool_calls: 0 };
        let mut content = vec![json!({"type": "text", "text": text})];
        if let Some((mime, data)) = picture {
            content.push(json!({"type": "image", "mimeType": mime, "data": data}));
        }
        let id = match self.wire.request("session/prompt", json!({"sessionId": self.session_id, "prompt": content})) {
            Ok(id) => id,
            Err(e) => {
                self.broken = true;
                out.finish = Finish::Error(e);
                return out;
            }
        };
        let replier = self.wire.replier();
        let started = Instant::now();
        let mut cancel_sent: Option<Instant> = None;
        let mut tools: HashMap<String, String> = HashMap::new();
        loop {
            if cancel.is_cancelled() && cancel_sent.is_none() {
                let _ = replier.notify("session/cancel", json!({"sessionId": self.session_id}));
                cancel_sent = Some(Instant::now());
            }
            if let Some(at) = cancel_sent
                && at.elapsed() > CANCEL_GRACE
            {
                self.broken = true;
                out.finish = Finish::Cancelled;
                return out;
            }
            if started.elapsed() > TURN_TIMEOUT {
                self.broken = true;
                out.finish = Finish::Error("The agent took too long and was stopped.".into());
                return out;
            }
            let msg = match self.wire.recv(Duration::from_millis(100)) {
                Ok(m) => m,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    self.broken = true;
                    out.finish = Finish::Error("The agent connection was lost.".into());
                    return out;
                }
            };
            match msg {
                Incoming::Closed(why) => {
                    self.broken = true;
                    let err = stderr_text(&self.stderr);
                    out.finish = Finish::Error(if err.is_empty() {
                        format!("The agent program ended ({why}).")
                    } else {
                        format!("The agent program ended ({why}):\n{err}")
                    });
                    return out;
                }
                Incoming::Response { id: rid, result } if rid == id => {
                    out.finish = match result {
                        Err(e) => Finish::Error(format!("The agent reported an error: {e}")),
                        Ok(v) => match v.get("stopReason").and_then(Value::as_str).unwrap_or("end_turn") {
                            "cancelled" => Finish::Cancelled,
                            "max_tokens" => Finish::MaxTokens,
                            "max_turn_requests" => Finish::StepLimit,
                            "refusal" => Finish::Error("The agent declined to continue.".into()),
                            _ => Finish::Done,
                        },
                    };
                    return out;
                }
                Incoming::Response { .. } => {}
                Incoming::Notification { method, params } if method == "session/update" => {
                    update(&params, &mut tools, &mut out, on_event);
                }
                Incoming::Notification { .. } => {}
                Incoming::Request { id: rid, method, params } => match method.as_str() {
                    "session/request_permission" => {
                        let req = permission_request(&params);
                        let chosen = if cancel.is_cancelled() {
                            None
                        } else if auto_allow_solidcraft && req.is_solidcraft_tool() && req.allow_once().is_some() {
                            req.allow_once().map(|o| o.id.clone())
                        } else {
                            permission(&req)
                        };
                        let outcome = match chosen {
                            Some(option) => json!({"outcome": "selected", "optionId": option}),
                            None => json!({"outcome": "cancelled"}),
                        };
                        let _ = replier.respond(&rid, json!({"outcome": outcome}));
                    }
                    _ => {
                        let _ = replier.respond_error(&rid, -32601, "SolidCraft does not provide that");
                    }
                },
            }
        }
    }
}

fn await_response(wire: &Wire, id: &Value, cancel: &Cancel, limit: Duration, stderr: &StderrTail) -> Result<Value, String> {
    let started = Instant::now();
    let replier = wire.replier();
    loop {
        if cancel.is_cancelled() {
            return Err("cancelled".into());
        }
        if started.elapsed() > limit {
            return Err("the agent did not answer in time".into());
        }
        match wire.recv(Duration::from_millis(100)) {
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err("the agent connection was lost".into()),
            Ok(Incoming::Closed(why)) => {
                let err = stderr_text(stderr);
                return Err(if err.is_empty() {
                    format!("the agent program ended ({why})")
                } else {
                    format!("the agent program ended ({why}): {err}")
                });
            }
            Ok(Incoming::Response { id: rid, result }) if &rid == id => return result.map_err(|e| format!("the agent refused: {e}")),
            Ok(Incoming::Request { id: rid, .. }) => {
                let _ = replier.respond_error(&rid, -32601, "SolidCraft does not provide that");
            }
            Ok(_) => {}
        }
    }
}

fn permission_request(params: &Value) -> PermissionRequest {
    let call = params.get("toolCall").unwrap_or(&Value::Null);
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").chars().take(300).collect::<String>();
    let options = params
        .get("options")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(16)
                .map(|o| PermissionOption { id: text(o, "optionId"), name: text(o, "name"), kind: text(o, "kind") })
                .filter(|o| !o.id.is_empty())
                .collect()
        })
        .unwrap_or_default();
    PermissionRequest {
        title: text(call, "title"),
        kind: text(call, "kind"),
        raw_input: call.get("rawInput").cloned().unwrap_or(Value::Null),
        options,
    }
}

fn content_text(v: &Value) -> String {
    let mut out = String::new();
    let mut add = |s: &str| {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(s);
    };
    if let Some(a) = v.as_array() {
        for c in a.iter().take(64) {
            if let Some(t) = c.pointer("/content/text").and_then(Value::as_str) {
                add(t);
            } else if c.get("type").and_then(Value::as_str) == Some("diff") {
                add(&format!("diff of {}", c.get("path").and_then(Value::as_str).unwrap_or("a file")));
            } else if let Some(t) = c.get("text").and_then(Value::as_str) {
                add(t);
            }
        }
    }
    out
}

fn update(params: &Value, tools: &mut HashMap<String, String>, out: &mut TurnOutcome, on_event: &mut dyn FnMut(Event)) {
    let Some(u) = params.get("update") else { return };
    let kind = u.get("sessionUpdate").and_then(Value::as_str).unwrap_or("");
    let chunk = || u.pointer("/content/text").and_then(Value::as_str).unwrap_or("").to_string();
    match kind {
        "agent_message_chunk" => {
            let t = chunk();
            if !t.is_empty() {
                on_event(Event::Text(t));
            }
        }
        "agent_thought_chunk" => {
            let t = chunk();
            if !t.is_empty() {
                on_event(Event::Thinking(t));
            }
        }
        "tool_call" => {
            let id = u.get("toolCallId").and_then(Value::as_str).unwrap_or("").to_string();
            let title = u
                .get("title")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
                .or_else(|| u.get("kind").and_then(Value::as_str))
                .unwrap_or("tool")
                .chars()
                .take(200)
                .collect::<String>();
            let input = u.get("rawInput").filter(|i| i.is_object()).cloned().unwrap_or_else(|| json!({}));
            out.tool_calls += 1;
            tools.insert(id.clone(), title.clone());
            on_event(Event::ToolCall { id: id.clone(), name: title.clone(), input });
            // A call that arrives already finished.
            if matches!(u.get("status").and_then(Value::as_str), Some("completed") | Some("failed")) {
                finish_tool(u, &id, &title, on_event);
            }
        }
        "tool_call_update" => {
            let id = u.get("toolCallId").and_then(Value::as_str).unwrap_or("").to_string();
            let title = tools.get(&id).cloned().unwrap_or_else(|| "tool".into());
            if matches!(u.get("status").and_then(Value::as_str), Some("completed") | Some("failed")) {
                finish_tool(u, &id, &title, on_event);
            }
        }
        "plan" => {
            let items: Vec<String> = u
                .get("entries")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .take(30)
                        .map(|e| {
                            let done = e.get("status").and_then(Value::as_str) == Some("completed");
                            format!("{} {}", if done { "[x]" } else { "[ ]" }, e.get("content").and_then(Value::as_str).unwrap_or(""))
                        })
                        .collect()
                })
                .unwrap_or_default();
            if !items.is_empty() {
                on_event(Event::Notice(items.join("\n")));
            }
        }
        _ => {}
    }
}

fn finish_tool(u: &Value, id: &str, title: &str, on_event: &mut dyn FnMut(Event)) {
    let is_error = u.get("status").and_then(Value::as_str) == Some("failed");
    let mut text = content_text(u.get("content").unwrap_or(&Value::Null));
    if text.is_empty()
        && let Some(raw) = u.get("rawOutput")
    {
        text = match raw {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
    }
    on_event(Event::ToolResult {
        id: id.to_string(),
        name: title.to_string(),
        is_error,
        text: crate::model::truncate(&text, MAX_TOOL_TEXT, "output truncated"),
        images: 0,
    });
}

#[cfg(test)]
mod tests;
