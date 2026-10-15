//! The Anthropic Messages API (`POST /v1/messages`, `stream: true`).

use serde_json::{Value, json};

use super::{ChatRequest, Codec, Decoder, parse_tool_args, push_capped, token};
use crate::model::{AssistantTurn, Block, Delta, MAX_BLOCKS, Message, Part, Role, Stop, ToolDef, Usage};
use crate::net::{HttpRequest, join_url};
use crate::settings::Secret;
use crate::sse::SseEvent;

pub const VERSION: &str = "2023-06-01";

pub struct Anthropic;

impl Codec for Anthropic {
    fn request(&self, req: &ChatRequest, base_url: &str, key: &Secret) -> Result<HttpRequest, String> {
        if key.is_empty() {
            return Err("No API key: add one in the assistant settings or set ANTHROPIC_API_KEY.".into());
        }
        let path = if base_url.trim_end_matches('/').ends_with("/v1") { "messages" } else { "v1/messages" };
        let body = serde_json::to_vec(&body(req)).map_err(|e| e.to_string())?;
        Ok(HttpRequest {
            url: join_url(base_url, path),
            headers: vec![
                ("x-api-key".into(), key.clone()),
                ("anthropic-version".into(), Secret::new(VERSION)),
                ("content-type".into(), Secret::new("application/json")),
                ("accept".into(), Secret::new("text/event-stream")),
            ],
            body,
        })
    }

    fn decoder(&self) -> Box<dyn Decoder> {
        Box::new(AnthropicDecoder::default())
    }
}

/// The request body.
pub fn body(req: &ChatRequest) -> Value {
    let mut tools: Vec<Value> = req.tools.iter().map(tool).collect();
    if let Some(last) = tools.last_mut() {
        last["cache_control"] = json!({"type": "ephemeral"});
    }
    let mut b = json!({
        "model": req.model,
        "max_tokens": req.max_tokens,
        "stream": true,
        "system": [{"type": "text", "text": req.system, "cache_control": {"type": "ephemeral"}}],
        "messages": req.messages.iter().filter_map(|m| message(m, req.vision)).collect::<Vec<_>>(),
    });
    if !tools.is_empty() {
        b["tools"] = Value::Array(tools);
    }
    b
}

fn tool(t: &ToolDef) -> Value {
    json!({"name": t.name, "description": t.description, "input_schema": t.schema})
}

fn image(mime: &str, data: &str) -> Value {
    json!({"type": "image", "source": {"type": "base64", "media_type": mime, "data": data}})
}

fn message(m: &Message, vision: bool) -> Option<Value> {
    let blocks: Vec<Value> = m
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Text(t) if t.trim().is_empty() => None,
            Block::Text(t) => Some(json!({"type": "text", "text": t})),
            Block::Image { mime, data } => vision.then(|| image(mime, data)),
            Block::ToolUse { id, name, input, .. } => Some(json!({"type": "tool_use", "id": id, "name": name, "input": input})),
            Block::ToolResult { id, parts, is_error } => {
                let mut content: Vec<Value> = parts
                    .iter()
                    .filter_map(|p| match p {
                        Part::Text(t) => Some(json!({"type": "text", "text": t})),
                        Part::Image { mime, data } => vision.then(|| image(mime, data)),
                    })
                    .collect();
                if content.is_empty() {
                    content.push(json!({"type": "text", "text": "(no output)"}));
                }
                let mut v = json!({"type": "tool_result", "tool_use_id": id, "content": content});
                if *is_error {
                    v["is_error"] = json!(true);
                }
                Some(v)
            }
        })
        .collect();
    if blocks.is_empty() {
        return None;
    }
    Some(json!({"role": match m.role { Role::User => "user", Role::Assistant => "assistant" }, "content": blocks}))
}

enum Slot {
    Text(String),
    Tool {
        id: String,
        name: String,
        json: String,
    },
    /// Thinking and other blocks that are not kept.
    Skip,
}

#[derive(Default)]
struct AnthropicDecoder {
    slots: Vec<Option<Slot>>,
    usage: Usage,
    stop: Option<Stop>,
    ended: bool,
}

impl Decoder for AnthropicDecoder {
    fn event(&mut self, ev: &SseEvent, sink: &mut dyn FnMut(Delta)) -> Result<(), String> {
        let data = ev.data.trim();
        if data.is_empty() {
            return Ok(());
        }
        let v: Value = serde_json::from_str(data).map_err(|_| "the service sent an event that is not JSON".to_string())?;
        let kind = v.get("type").and_then(Value::as_str).or(ev.name.as_deref()).unwrap_or("");
        let index = v.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok());
        match kind {
            "message_start" => {
                let u = v.pointer("/message/usage");
                self.usage.input = token(u.and_then(|u| u.get("input_tokens")));
                self.usage.cache_read = token(u.and_then(|u| u.get("cache_read_input_tokens")));
                self.usage.cache_write = token(u.and_then(|u| u.get("cache_creation_input_tokens")));
                self.usage.output = token(u.and_then(|u| u.get("output_tokens")));
            }
            "content_block_start" => {
                let i = index.ok_or("a content block has no index")?;
                if i >= MAX_BLOCKS {
                    return Err("the reply has too many blocks".into());
                }
                let cb = v.get("content_block").unwrap_or(&Value::Null);
                let slot = match cb.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        let mut t = String::new();
                        if let Some(s) = cb.get("text").and_then(Value::as_str) {
                            push_capped(&mut t, s)?;
                            if !s.is_empty() {
                                sink(Delta::Text(s.to_string()));
                            }
                        }
                        Slot::Text(t)
                    }
                    Some("tool_use") => {
                        let name = cb.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                        sink(Delta::ToolStart { name: name.clone() });
                        Slot::Tool { id: cb.get("id").and_then(Value::as_str).unwrap_or("").to_string(), name, json: String::new() }
                    }
                    _ => Slot::Skip,
                };
                if self.slots.len() <= i {
                    self.slots.resize_with(i + 1, || None);
                }
                if let Some(s) = self.slots.get_mut(i) {
                    *s = Some(slot);
                }
            }
            "content_block_delta" => {
                let i = index.ok_or("a content delta has no index")?;
                let d = v.get("delta").unwrap_or(&Value::Null);
                let slot = self.slots.get_mut(i).and_then(Option::as_mut);
                match (d.get("type").and_then(Value::as_str), slot) {
                    (Some("text_delta"), Some(Slot::Text(t))) => {
                        let s = d.get("text").and_then(Value::as_str).unwrap_or("");
                        push_capped(t, s)?;
                        if !s.is_empty() {
                            sink(Delta::Text(s.to_string()));
                        }
                    }
                    (Some("input_json_delta"), Some(Slot::Tool { json, .. })) => {
                        push_capped(json, d.get("partial_json").and_then(Value::as_str).unwrap_or(""))?;
                    }
                    (Some("thinking_delta"), _) => {
                        if let Some(s) = d.get("thinking").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                            sink(Delta::Thinking(s.to_string()));
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(r) = v.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.stop = Some(match r {
                        "end_turn" | "stop_sequence" => Stop::EndTurn,
                        "tool_use" => Stop::ToolUse,
                        "max_tokens" => Stop::MaxTokens,
                        other => Stop::Other(other.to_string()),
                    });
                }
                if let Some(o) = v.pointer("/usage/output_tokens").and_then(Value::as_u64) {
                    self.usage.output = o;
                }
            }
            "message_stop" => self.ended = true,
            "error" => {
                let m = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("unknown error");
                let t = v.pointer("/error/type").and_then(Value::as_str).unwrap_or("error");
                return Err(format!("The service reported an error ({t}): {m}"));
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<AssistantTurn, String> {
        let this = *self;
        if !this.ended && this.stop.is_none() {
            return Err("The reply ended before it was complete.".into());
        }
        let mut blocks = Vec::new();
        for slot in this.slots.into_iter().flatten() {
            match slot {
                Slot::Text(t) if t.is_empty() => {}
                Slot::Text(t) => blocks.push(Block::Text(t)),
                Slot::Tool { id, name, json } => {
                    let (input, error) = parse_tool_args(&json);
                    blocks.push(Block::ToolUse { id, name, input, error });
                }
                Slot::Skip => {}
            }
        }
        let has_tools = blocks.iter().any(|b| matches!(b, Block::ToolUse { .. }));
        let stop = match this.stop {
            // A truncated tool call must not run, so a cut-off reply is never "tool use".
            Some(Stop::ToolUse) if !has_tools => Stop::EndTurn,
            Some(s) => s,
            None if has_tools => Stop::ToolUse,
            None => Stop::EndTurn,
        };
        Ok(AssistantTurn { blocks, stop, usage: this.usage })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(events: &[Value]) -> Result<(AssistantTurn, Vec<Delta>), String> {
        let mut d: Box<dyn Decoder> = Box::new(AnthropicDecoder::default());
        let mut deltas = Vec::new();
        for e in events {
            d.event(&SseEvent { name: None, data: e.to_string() }, &mut |x| deltas.push(x))?;
        }
        Ok((d.finish()?, deltas))
    }

    fn stream_with_tool() -> Vec<Value> {
        vec![
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 100, "cache_read_input_tokens": 40, "output_tokens": 1}}}),
            json!({"type": "ping"}),
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "hmm"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "Making a "}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "box."}}),
            json!({"type": "content_block_start", "index": 2, "content_block": {"type": "tool_use", "id": "toolu_1", "name": "execute", "input": {}}}),
            json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "{\"command\": \"solid."}}),
            json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "box\", \"params\": {\"length\": 10}}"}}),
            json!({"type": "content_block_stop", "index": 2}),
            json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 55}}),
            json!({"type": "message_stop"}),
        ]
    }

    #[test]
    fn a_reply_with_text_and_a_tool_call() {
        let (turn, deltas) = run(&stream_with_tool()).unwrap();
        assert_eq!(turn.stop, Stop::ToolUse);
        assert_eq!(turn.usage, Usage { input: 100, output: 55, cache_read: 40, cache_write: 0 });
        assert_eq!(
            turn.blocks,
            vec![
                Block::Text("Making a box.".into()),
                Block::ToolUse {
                    id: "toolu_1".into(),
                    name: "execute".into(),
                    input: json!({"command": "solid.box", "params": {"length": 10}}),
                    error: None
                },
            ]
        );
        assert_eq!(
            deltas,
            vec![
                Delta::Thinking("hmm".into()),
                Delta::Text("Making a ".into()),
                Delta::Text("box.".into()),
                Delta::ToolStart { name: "execute".into() }
            ]
        );
    }

    #[test]
    fn bad_tool_arguments_are_recorded_not_fatal() {
        let mut ev = stream_with_tool();
        ev[10] = json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "box\" oops"}});
        let (turn, _) = run(&ev).unwrap();
        match &turn.blocks[1] {
            Block::ToolUse { input, error, .. } => {
                assert_eq!(input, &json!({}));
                assert!(error.as_deref().unwrap_or("").contains("not valid JSON"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_truncated_stream_is_an_error() {
        let ev = stream_with_tool();
        assert!(run(&ev[..8]).is_err());
    }

    #[test]
    fn error_events_carry_the_service_message() {
        let e = run(&[json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}})]).unwrap_err();
        assert!(e.contains("overloaded_error") && e.contains("Overloaded"), "{e}");
    }

    #[test]
    fn hostile_events_do_not_panic() {
        for ev in [
            json!({"type": "content_block_delta", "index": 99999999999u64, "delta": {"type": "text_delta", "text": "x"}}),
            json!({"type": "content_block_delta", "delta": {}}),
            json!({"type": "content_block_start", "index": 1000, "content_block": {}}),
            json!({"type": "content_block_start", "index": "a"}),
            json!({"type": 5}),
            json!([1, 2]),
            json!("x"),
            json!(null),
        ] {
            let mut d = AnthropicDecoder::default();
            let _ = d.event(&SseEvent { name: None, data: ev.to_string() }, &mut |_| {});
        }
        let mut d = AnthropicDecoder::default();
        assert!(d.event(&SseEvent { name: None, data: "not json".into() }, &mut |_| {}).is_err());
    }

    #[test]
    fn an_oversized_reply_is_cut_off() {
        let mut d = AnthropicDecoder::default();
        d.event(
            &SseEvent {
                name: None,
                data: json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}).to_string(),
            },
            &mut |_| {},
        )
        .unwrap();
        let chunk = "a".repeat(300_000);
        let mut failed = false;
        for _ in 0..6 {
            let ev = json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": chunk}});
            if d.event(&SseEvent { name: None, data: ev.to_string() }, &mut |_| {}).is_err() {
                failed = true;
                break;
            }
        }
        assert!(failed);
    }

    #[test]
    fn the_request_body() {
        let tools = vec![
            ToolDef { name: "a".into(), description: "A".into(), schema: json!({"type": "object"}) },
            ToolDef { name: "b".into(), description: "B".into(), schema: json!({"type": "object"}) },
        ];
        let messages = vec![
            Message::user_text("make a box"),
            Message::assistant(vec![
                Block::Text("ok".into()),
                Block::ToolUse { id: "t1".into(), name: "a".into(), input: json!({"x": 1}), error: None },
            ]),
            Message {
                role: Role::User,
                blocks: vec![Block::ToolResult {
                    id: "t1".into(),
                    parts: vec![Part::Text("done".into()), Part::Image { mime: "image/png".into(), data: "AAAA".into() }],
                    is_error: true,
                }],
            },
        ];
        let req = ChatRequest { model: "m", system: "sys", messages: &messages, tools: &tools, vision: true, max_tokens: 100 };
        let b = body(&req);
        assert_eq!(b["model"], "m");
        assert_eq!(b["stream"], true);
        assert_eq!(b["system"][0]["cache_control"]["type"], "ephemeral");
        assert!(b["tools"][0].get("cache_control").is_none());
        assert_eq!(b["tools"][1]["cache_control"]["type"], "ephemeral");
        assert_eq!(b["messages"][1]["content"][1]["type"], "tool_use");
        let tr = &b["messages"][2]["content"][0];
        assert_eq!(tr["type"], "tool_result");
        assert_eq!(tr["tool_use_id"], "t1");
        assert_eq!(tr["is_error"], true);
        assert_eq!(tr["content"][1]["source"]["media_type"], "image/png");
        // Without vision the image is dropped.
        let req = ChatRequest { vision: false, ..req };
        assert_eq!(body(&req)["messages"][2]["content"][0]["content"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn the_request_has_the_key_and_the_right_address() {
        let req = ChatRequest { model: "m", system: "s", messages: &[], tools: &[], vision: false, max_tokens: 1 };
        let r = Anthropic.request(&req, "https://api.anthropic.com/", &Secret::new("sk")).unwrap();
        assert_eq!(r.url, "https://api.anthropic.com/v1/messages");
        assert!(r.headers.iter().any(|(n, v)| n == "x-api-key" && v.expose() == "sk"));
        let r = Anthropic.request(&req, "https://proxy.example/v1", &Secret::new("sk")).unwrap();
        assert_eq!(r.url, "https://proxy.example/v1/messages");
        assert!(Anthropic.request(&req, "https://x", &Secret::default()).is_err());
    }
}
