//! Provider protocols: how a conversation becomes a streaming HTTP request, and how the
//! streamed events become one [`AssistantTurn`].
//!
//! Replies come from a server we do not control (or from a small local model), so decoders
//! never trust a field's presence or type, cap what they accumulate and turn unusable tool
//! arguments into a recorded error instead of a failure.

use std::time::Duration;

use serde_json::{Map, Value};

use crate::model::{AssistantTurn, Delta, MAX_BLOCK, Message, ToolDef};
use crate::net::{Cancel, HttpError, HttpRequest, HttpStream};
use crate::settings::Secret;
use crate::sse::{SseDecoder, SseEvent};

pub mod anthropic;
pub mod openai;

/// What the model is asked.
pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub system: &'a str,
    pub messages: &'a [Message],
    pub tools: &'a [ToolDef],
    /// Images may be sent (to the model, and inside tool results).
    pub vision: bool,
    pub max_tokens: u32,
}

/// Builds requests and decoders for one protocol.
pub trait Codec: Send + Sync {
    fn request(&self, req: &ChatRequest, base_url: &str, key: &Secret) -> Result<HttpRequest, String>;
    fn decoder(&self) -> Box<dyn Decoder>;
}

/// Turns the events of one streamed reply into a turn.
pub trait Decoder: Send {
    /// One event; text deltas go to `sink` as they arrive. An error aborts the reply.
    fn event(&mut self, ev: &SseEvent, sink: &mut dyn FnMut(Delta)) -> Result<(), String>;
    /// The stream ended: the whole reply, or why there is none.
    fn finish(self: Box<Self>) -> Result<AssistantTurn, String>;
}

/// How a request for one reply failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamError {
    Http(HttpError),
    /// The server answered, but not in the protocol's shape.
    Protocol(String),
    Cancelled,
}

impl StreamError {
    /// A message for the person, with the key removed from anything the server echoed.
    pub fn describe(&self, key: &Secret) -> String {
        let scrub = |s: &str| if key.is_empty() { s.to_string() } else { s.replace(key.expose(), "[key]") };
        match self {
            StreamError::Cancelled => "Stopped.".into(),
            StreamError::Protocol(m) => scrub(m),
            StreamError::Http(HttpError::Cancelled) => "Stopped.".into(),
            StreamError::Http(HttpError::Transport(m)) => scrub(&format!("Cannot reach the assistant service: {m}")),
            StreamError::Http(HttpError::Status { code, retry_after, body }) => {
                let hint = match code {
                    401 | 403 => "The key was rejected; check the API key in the settings.".to_string(),
                    404 => "Not found; check the address and the model name in the settings.".to_string(),
                    408 | 504 => "The service timed out; try again.".to_string(),
                    413 => "The conversation is too large for the model; start a new chat.".to_string(),
                    429 => match retry_after {
                        Some(s) => format!("Rate limited; try again in {s} s."),
                        None => "Rate limited; try again in a moment.".to_string(),
                    },
                    500..=599 => "The service is overloaded or down; try again later.".to_string(),
                    _ => format!("The service refused the request ({code})."),
                };
                let detail = server_message(body);
                scrub(&if detail.is_empty() { hint } else { format!("{hint} ({detail})") })
            }
        }
    }

    /// Worth trying again before anything was received.
    pub fn is_transient(&self) -> bool {
        match self {
            StreamError::Http(HttpError::Transport(_)) => true,
            StreamError::Http(HttpError::Status { code, .. }) => matches!(code, 408 | 429 | 500..=599),
            _ => false,
        }
    }

    /// Seconds the server asked to wait.
    pub fn retry_after(&self) -> Option<u64> {
        match self {
            StreamError::Http(HttpError::Status { retry_after, .. }) => *retry_after,
            _ => None,
        }
    }
}

/// The message inside a provider's JSON error body (`{"error":{"message":…}}`), else the first
/// part of the body.
fn server_message(body: &str) -> String {
    let short = |s: &str| crate::model::truncate(s.trim(), 300, "cut");
    match serde_json::from_str::<Value>(body) {
        Ok(v) => {
            let m = v.pointer("/error/message").or_else(|| v.get("message")).or_else(|| v.get("error")).and_then(Value::as_str);
            short(m.unwrap_or(""))
        }
        Err(_) => short(body),
    }
}

/// Send `req` and decode the stream; `sink` sees text as it arrives. Retries once, after a short
/// wait, when the failure is transient and nothing has been delivered yet.
pub fn stream_turn(
    codec: &dyn Codec,
    http: &HttpStream,
    req: &HttpRequest,
    cancel: &Cancel,
    sink: &mut dyn FnMut(Delta),
) -> Result<AssistantTurn, StreamError> {
    let mut attempt = 0;
    loop {
        let mut delivered = false;
        match stream_once(codec, http, req, cancel, &mut |d| {
            delivered = true;
            sink(d)
        }) {
            Err(e) if attempt == 0 && !delivered && e.is_transient() && !cancel.is_cancelled() => {
                attempt += 1;
                let wait = e.retry_after().unwrap_or(1).min(10);
                for _ in 0..wait * 10 {
                    if cancel.is_cancelled() {
                        return Err(StreamError::Cancelled);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            other => return other,
        }
    }
}

fn stream_once(
    codec: &dyn Codec,
    http: &HttpStream,
    req: &HttpRequest,
    cancel: &Cancel,
    sink: &mut dyn FnMut(Delta),
) -> Result<AssistantTurn, StreamError> {
    let mut decoder = codec.decoder();
    let mut sse = SseDecoder::new();
    let mut failure: Option<String> = None;
    let mut events = Vec::new();
    let result = http(req, cancel, &mut |chunk| {
        events.clear();
        let mut r = sse.push(chunk, &mut events);
        for ev in events.drain(..) {
            if r.is_err() {
                break;
            }
            r = decoder.event(&ev, sink);
        }
        match r {
            Ok(()) => !cancel.is_cancelled(),
            Err(e) => {
                failure = Some(e);
                false
            }
        }
    });
    if cancel.is_cancelled() {
        return Err(StreamError::Cancelled);
    }
    if let Some(m) = failure {
        return Err(StreamError::Protocol(m));
    }
    result.map_err(|e| if e == HttpError::Cancelled { StreamError::Cancelled } else { StreamError::Http(e) })?;
    events.clear();
    sse.finish(&mut events).map_err(StreamError::Protocol)?;
    for ev in events.drain(..) {
        decoder.event(&ev, sink).map_err(StreamError::Protocol)?;
    }
    decoder.finish().map_err(StreamError::Protocol)
}

/// Parse accumulated tool-call arguments: `Ok(object)`, or `Err(reason)` with `{}` as the value
/// to keep in the history (it must stay valid JSON).
pub(crate) fn parse_tool_args(raw: &str) -> (Value, Option<String>) {
    let text = raw.trim();
    if text.is_empty() {
        return (Value::Object(Map::new()), None);
    }
    match serde_json::from_str::<Value>(text) {
        Ok(v @ Value::Object(_)) => (v, None),
        Ok(_) => (Value::Object(Map::new()), Some("the tool arguments must be a JSON object".into())),
        Err(e) => (Value::Object(Map::new()), Some(format!("the tool arguments were not valid JSON ({e})"))),
    }
}

/// Append `s` to `buf` unless that would pass [`MAX_BLOCK`].
pub(crate) fn push_capped(buf: &mut String, s: &str) -> Result<(), String> {
    if buf.len() + s.len() > MAX_BLOCK {
        return Err("the reply is too long".into());
    }
    buf.push_str(s);
    Ok(())
}

pub(crate) fn token(v: Option<&Value>) -> u64 {
    v.and_then(Value::as_u64).unwrap_or(0)
}
