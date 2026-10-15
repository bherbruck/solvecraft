//! One assistant turn: ask the model, run the tools it calls, show it the results, repeat until
//! it answers in words, the user stops it or the step limit is reached.
//!
//! The loop runs on a worker thread and never touches the design itself: every tool call goes
//! through a [`ToolHost`], which the app implements by sending a control request to the UI
//! thread. Tests implement it over a headless session.

use std::path::PathBuf;

use serde_json::{Value, json};
use solidcraft_mcp::{Backend, call_tool};

use crate::model::{AssistantTurn, Block, Delta, Message, Part, Role, Stop, ToolDef, Usage, truncate};
use crate::net::{Cancel, HttpStream, check_url};
use crate::provider::{ChatRequest, Codec, StreamError, stream_turn};
use crate::settings::Secret;
use crate::tools::result_parts;

/// Most model requests in one turn.
pub const MAX_STEPS: usize = 24;

/// Most bytes of one tool result shown to the model.
pub const MAX_RESULT: usize = 32 * 1024;

/// Runs control-channel methods for a turn.
pub trait ToolHost: Send + Sync {
    /// Run `method` and return its result, or the error the model should see. Implementations
    /// poll `cancel` while they wait.
    fn call(&self, method: &str, params: Value, cancel: &Cancel) -> Result<Value, String>;
}

/// The MCP tools' [`Backend`] over a [`ToolHost`].
pub struct HostBackend<'a> {
    pub host: &'a dyn ToolHost,
    pub cancel: &'a Cancel,
}

impl Backend for HostBackend<'_> {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        if self.cancel.is_cancelled() {
            return Err("cancelled by the user".into());
        }
        self.host.call(method, params, self.cancel)
    }

    /// Pictures are model renders, never the window: the window shows this very conversation.
    fn has_ui(&self) -> bool {
        false
    }

    fn describe(&self) -> String {
        "the SolidCraft app".into()
    }
}

/// What the person can watch while a turn runs.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Text(String),
    Thinking(String),
    /// A tool call began streaming in.
    ToolStart {
        name: String,
    },
    /// A tool call is about to run.
    ToolCall {
        id: String,
        name: String,
        input: Value,
    },
    /// A tool call finished; `text` is what the model was shown.
    ToolResult {
        id: String,
        name: String,
        is_error: bool,
        text: String,
        images: usize,
    },
    /// Token counts of the whole turn so far.
    Usage(Usage),
    /// Something the person should know (a retry, a limit).
    Notice(String),
}

/// How a turn ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Finish {
    /// The model answered in words.
    Done,
    Cancelled,
    /// 24 requests without a final answer.
    StepLimit,
    /// The model's reply was cut off at the token limit.
    MaxTokens,
    /// A message for the person.
    Error(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TurnOutcome {
    pub finish: Finish,
    pub usage: Usage,
    /// Model requests made.
    pub steps: usize,
    /// How many tool calls ran.
    pub tool_calls: usize,
}

/// Everything a turn needs besides the conversation.
pub struct TurnConfig<'a> {
    pub codec: &'a dyn Codec,
    pub http: &'a HttpStream,
    pub base_url: &'a str,
    pub allow_http: bool,
    pub key: &'a Secret,
    pub model: &'a str,
    pub system: &'a str,
    pub tools: &'a [ToolDef],
    pub vision: bool,
    pub max_tokens: u32,
    pub max_steps: usize,
    /// Tokens the model can read; the history is trimmed to fit.
    pub context_window: u32,
    /// Folders tool calls may read and write in (besides the app's own recovery and scratch
    /// folders).
    pub folders: &'a [PathBuf],
}

/// Run one user message to completion. `history` gains the user message, every model reply and
/// every tool result; a turn that fails on its first request leaves `history` as it was.
pub fn run_turn(
    cfg: &TurnConfig,
    history: &mut Vec<Message>,
    user: Message,
    host: &dyn ToolHost,
    cancel: &Cancel,
    on_event: &mut dyn FnMut(Event),
) -> TurnOutcome {
    let mut base = history.len();
    let fixed = crate::context::fixed_tokens(cfg.system, cfg.tools);
    let mut out = TurnOutcome { finish: Finish::Done, usage: Usage::default(), steps: 0, tool_calls: 0 };
    if let Err(e) = check_url(cfg.base_url, cfg.allow_http) {
        out.finish = Finish::Error(e);
        return out;
    }
    history.push(user);
    loop {
        if cancel.is_cancelled() {
            out.finish = Finish::Cancelled;
            // Stopped before the model was asked anything: the message was never sent.
            if out.steps == 0 {
                history.truncate(base);
            }
            return out;
        }
        if out.steps >= cfg.max_steps {
            out.finish = Finish::StepLimit;
            return out;
        }
        out.steps += 1;
        strip_old_pictures(history);
        crate::context::strip_old_context(history);
        let dropped = crate::context::trim_history(history, cfg.context_window, fixed);
        base = base.saturating_sub(dropped.messages);
        if dropped.turns > 0 {
            on_event(Event::Notice(format!("The conversation is long: the oldest {} turn(s) are no longer sent to the model.", dropped.turns)));
        }
        let req =
            ChatRequest { model: cfg.model, system: cfg.system, messages: history, tools: cfg.tools, vision: cfg.vision, max_tokens: cfg.max_tokens };
        let http_req = match cfg.codec.request(&req, cfg.base_url, cfg.key) {
            Ok(r) => r,
            Err(e) => return fail(out, history, base, e),
        };
        let reply = stream_turn(cfg.codec, cfg.http, &http_req, cancel, &mut |d| {
            on_event(match d {
                Delta::Text(t) => Event::Text(t),
                Delta::Thinking(t) => Event::Thinking(t),
                Delta::ToolStart { name } => Event::ToolStart { name },
            })
        });
        let turn: AssistantTurn = match reply {
            Ok(t) => t,
            Err(StreamError::Cancelled) => {
                out.finish = Finish::Cancelled;
                // Nothing of the cut-off reply is kept; a turn without any step stays unsent.
                if out.steps == 1 {
                    history.truncate(base);
                }
                return out;
            }
            Err(e) => return fail(out, history, base, e.describe(cfg.key)),
        };
        out.usage.add(&turn.usage);
        on_event(Event::Usage(out.usage));
        let calls: Vec<(String, String, Value, Option<String>)> = turn
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::ToolUse { id, name, input, error } => Some((id.clone(), name.clone(), input.clone(), error.clone())),
                _ => None,
            })
            .collect();
        let stop = turn.stop.clone();
        // An empty reply must not enter the history: some providers reject empty messages.
        if !turn.blocks.is_empty() {
            history.push(Message::assistant(turn.blocks));
        }
        if calls.is_empty() || stop != Stop::ToolUse {
            // A reply cut off by the token limit holds no tool call that may run.
            out.finish = if stop == Stop::MaxTokens { Finish::MaxTokens } else { Finish::Done };
            if let Stop::Other(r) = stop {
                on_event(Event::Notice(format!("The model stopped: {r}")));
            }
            return out;
        }
        let mut results = Vec::with_capacity(calls.len());
        for (id, name, input, error) in calls {
            // Every call gets an answer, also after a cancel: the history must stay valid.
            let r = if cancel.is_cancelled() {
                (vec![Part::Text("Cancelled by the user before this ran.".into())], true)
            } else if let Some(e) = error {
                (vec![Part::Text(format!("Not run: {e}. Send the call again with valid JSON arguments."))], true)
            } else {
                on_event(Event::ToolCall { id: id.clone(), name: name.clone(), input: input.clone() });
                out.tool_calls += 1;
                run_tool(host, cancel, cfg.folders, &name, &input)
            };
            let (parts, is_error) = limit(r.0, r.1);
            let text = parts.iter().filter_map(|p| if let Part::Text(t) = p { Some(t.as_str()) } else { None }).collect::<Vec<_>>().join("\n");
            let images = parts.iter().filter(|p| matches!(p, Part::Image { .. })).count();
            on_event(Event::ToolResult { id: id.clone(), name, is_error, text, images });
            results.push(Block::ToolResult { id, parts, is_error });
        }
        history.push(Message { role: Role::User, blocks: results });
    }
}

fn fail(mut out: TurnOutcome, history: &mut Vec<Message>, base: usize, msg: String) -> TurnOutcome {
    out.finish = Finish::Error(msg);
    // Nothing happened yet: leave the conversation as it was, so "retry" sends it again.
    if out.steps <= 1 {
        history.truncate(base);
    }
    out
}

/// Run one tool call, with file access confined to the assistant's folders on this thread.
fn run_tool(host: &dyn ToolHost, cancel: &Cancel, folders: &[PathBuf], name: &str, input: &Value) -> (Vec<Part>, bool) {
    let mut backend = HostBackend { host, cancel };
    let scope = solidcraft_mcp::agent_scope(folders.iter().cloned());
    let r = solidcraft_engine::io::vfs::scoped(Some(scope), || call_tool(&mut backend, name, input));
    result_parts(&r)
}

/// Cap what the model is shown from one tool call.
fn limit(parts: Vec<Part>, is_error: bool) -> (Vec<Part>, bool) {
    let mut budget = MAX_RESULT;
    let parts = parts
        .into_iter()
        .map(|p| match p {
            Part::Text(t) => {
                let t = truncate(&t, budget, "output truncated; ask for less (a filter, one body)");
                budget = budget.saturating_sub(t.len());
                Part::Text(t)
            }
            img => img,
        })
        .collect();
    (parts, is_error)
}

/// Pictures stay in the history only until the model has seen them: older tool results keep a
/// note instead, so each later request does not carry every image again.
fn strip_old_pictures(history: &mut [Message]) {
    let n = history.len();
    for (i, m) in history.iter_mut().enumerate() {
        if i + 1 == n {
            continue;
        }
        for b in &mut m.blocks {
            match b {
                Block::ToolResult { parts, .. } => {
                    for p in parts.iter_mut() {
                        if matches!(p, Part::Image { .. }) {
                            *p = Part::Text("(an earlier picture, not sent again)".into());
                        }
                    }
                }
                Block::Image { .. } => *b = Block::Text("(an earlier picture, not sent again)".into()),
                _ => {}
            }
        }
    }
}

/// A short description of what a call did, for a card header: the command id of `execute`, the
/// number of steps of `batch`.
pub fn call_title(name: &str, input: &Value) -> String {
    match name {
        "execute" => input.get("command").and_then(Value::as_str).map(|c| format!("execute {c}")).unwrap_or_else(|| "execute".into()),
        "batch" => {
            let n = input.get("commands").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
            let first: Vec<&str> = input
                .get("commands")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|c| c.get("command").and_then(Value::as_str)).take(3).collect())
                .unwrap_or_default();
            if first.is_empty() {
                format!("batch ({n} steps)")
            } else {
                format!("batch ({n} steps: {}{})", first.join(", "), if n > first.len() { ", …" } else { "" })
            }
        }
        other => other.to_string(),
    }
}

/// Used by tests and the UI to describe a call as JSON text.
pub fn call_json(name: &str, input: &Value) -> Value {
    json!({"tool": name, "input": input})
}

#[cfg(test)]
mod tests;
