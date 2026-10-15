//! OpenAI-compatible chat completions (`POST {base}/chat/completions`, `stream: true`): OpenAI,
//! Ollama, LM Studio and other servers that speak this protocol.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{ChatRequest, Codec, Decoder, parse_tool_args, push_capped, token};
use crate::model::{AssistantTurn, Block, Delta, MAX_BLOCKS, Message, Part, Role, Stop, ToolDef, Usage};
use crate::net::{HttpRequest, join_url};
use crate::settings::Secret;
use crate::sse::SseEvent;

pub struct OpenAi;

impl Codec for OpenAi {
    fn request(&self, req: &ChatRequest, base_url: &str, key: &Secret) -> Result<HttpRequest, String> {
        let mut headers =
            vec![("content-type".to_string(), Secret::new("application/json")), ("accept".to_string(), Secret::new("text/event-stream"))];
        if !key.is_empty() {
            headers.push(("authorization".into(), Secret::new(format!("Bearer {}", key.expose()))));
        }
        Ok(HttpRequest { url: join_url(base_url, "chat/completions"), headers, body: serde_json::to_vec(&body(req)).map_err(|e| e.to_string())? })
    }

    fn decoder(&self) -> Box<dyn Decoder> {
        Box::new(OpenAiDecoder::default())
    }
}

pub fn body(req: &ChatRequest) -> Value {
    let mut messages = vec![json!({"role": "system", "content": req.system})];
    for m in req.messages {
        push_message(&mut messages, m, req.vision);
    }
    let mut b = json!({
        "model": req.model,
        "stream": true,
        "stream_options": {"include_usage": true},
        "messages": messages,
    });
    if !req.tools.is_empty() {
        b["tools"] = Value::Array(req.tools.iter().map(tool).collect());
    }
    b
}

fn tool(t: &ToolDef) -> Value {
    json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.schema}})
}

fn image_url(mime: &str, data: &str) -> Value {
    json!({"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{data}")}})
}

fn push_message(out: &mut Vec<Value>, m: &Message, vision: bool) {
    match m.role {
        Role::Assistant => {
            let text: Vec<&str> = m.blocks.iter().filter_map(|b| if let Block::Text(t) = b { Some(t.as_str()) } else { None }).collect();
            let calls: Vec<Value> = m
                .blocks
                .iter()
                .filter_map(|b| match b {
                    Block::ToolUse { id, name, input, .. } => {
                        Some(json!({"id": id, "type": "function", "function": {"name": name, "arguments": input.to_string()}}))
                    }
                    _ => None,
                })
                .collect();
            if text.is_empty() && calls.is_empty() {
                return;
            }
            let mut v = json!({"role": "assistant", "content": if text.is_empty() { Value::Null } else { Value::String(text.join("\n\n")) }});
            if !calls.is_empty() {
                v["tool_calls"] = Value::Array(calls);
            }
            out.push(v);
        }
        Role::User => {
            let mut parts: Vec<Value> = Vec::new();
            let mut pictures: Vec<Value> = Vec::new();
            let flush = |out: &mut Vec<Value>, parts: &mut Vec<Value>| {
                if !parts.is_empty() {
                    out.push(json!({"role": "user", "content": std::mem::take(parts)}));
                }
            };
            for b in &m.blocks {
                match b {
                    Block::Text(t) if !t.trim().is_empty() => parts.push(json!({"type": "text", "text": t})),
                    Block::Image { mime, data } if vision => parts.push(image_url(mime, data)),
                    Block::ToolResult { id, parts: rp, is_error } => {
                        let mut text: Vec<String> = rp.iter().filter_map(|p| if let Part::Text(t) = p { Some(t.clone()) } else { None }).collect();
                        if *is_error {
                            text.insert(0, "Error:".into());
                        }
                        let n_images = rp.iter().filter(|p| matches!(p, Part::Image { .. })).count();
                        if n_images > 0 {
                            text.push(if vision {
                                "(the image follows in the next message)".to_string()
                            } else {
                                "(an image was produced; this model cannot see images)".to_string()
                            });
                        }
                        let text = if text.is_empty() { "(no output)".to_string() } else { text.join("\n") };
                        out.push(json!({"role": "tool", "tool_call_id": id, "content": text}));
                        if vision {
                            pictures.extend(
                                rp.iter().filter_map(|p| if let Part::Image { mime, data } = p { Some(image_url(mime, data)) } else { None }),
                            );
                        }
                    }
                    _ => {}
                }
            }
            flush(out, &mut parts);
            // Tool messages are text only; a picture goes in a user message right after them.
            if !pictures.is_empty() {
                out.push(json!({"role": "user", "content": pictures}));
            }
        }
    }
}

#[derive(Default)]
struct Call {
    id: String,
    name: String,
    args: String,
}

#[derive(Default)]
struct OpenAiDecoder {
    text: String,
    calls: BTreeMap<usize, Call>,
    usage: Usage,
    finish: Option<String>,
    done: bool,
    got_any: bool,
}

impl Decoder for OpenAiDecoder {
    fn event(&mut self, ev: &SseEvent, sink: &mut dyn FnMut(Delta)) -> Result<(), String> {
        let data = ev.data.trim();
        if data.is_empty() {
            return Ok(());
        }
        if data == "[DONE]" {
            self.done = true;
            return Ok(());
        }
        let v: Value = serde_json::from_str(data).map_err(|_| "the service sent an event that is not JSON".to_string())?;
        if let Some(e) = v.get("error") {
            let m = e.get("message").and_then(Value::as_str).or_else(|| e.as_str()).unwrap_or("unknown error");
            return Err(format!("The service reported an error: {m}"));
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            self.usage.input = token(u.get("prompt_tokens"));
            self.usage.output = token(u.get("completion_tokens"));
            self.usage.cache_read = token(u.pointer("/prompt_tokens_details/cached_tokens"));
        }
        let Some(choice) = v.get("choices").and_then(Value::as_array).and_then(|c| c.first()) else { return Ok(()) };
        self.got_any = true;
        let delta = choice.get("delta").unwrap_or(&Value::Null);
        if let Some(s) = delta.get("content").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            push_capped(&mut self.text, s)?;
            sink(Delta::Text(s.to_string()));
        }
        if let Some(s) = delta.get("reasoning_content").or_else(|| delta.get("reasoning")).and_then(Value::as_str).filter(|s| !s.is_empty()) {
            sink(Delta::Thinking(s.to_string()));
        }
        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for c in calls {
                let index = c.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok()).unwrap_or(0);
                if index >= MAX_BLOCKS {
                    return Err("the reply has too many tool calls".into());
                }
                let is_new = !self.calls.contains_key(&index);
                let call = self.calls.entry(index).or_default();
                if let Some(id) = c.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    call.id = id.to_string();
                }
                if let Some(f) = c.get("function") {
                    if let Some(n) = f.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                        call.name.push_str(n);
                    }
                    if let Some(a) = f.get("arguments").and_then(Value::as_str) {
                        push_capped(&mut call.args, a)?;
                    }
                }
                if is_new {
                    sink(Delta::ToolStart { name: call.name.clone() });
                }
            }
        }
        if let Some(r) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish = Some(r.to_string());
        }
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<AssistantTurn, String> {
        let this = *self;
        if !this.done && this.finish.is_none() {
            return Err("The reply ended before it was complete.".into());
        }
        let mut blocks = Vec::new();
        if !this.text.is_empty() {
            blocks.push(Block::Text(this.text));
        }
        for (n, c) in this.calls.into_iter() {
            if c.name.is_empty() {
                continue;
            }
            let id = if c.id.is_empty() { format!("call_{n}") } else { c.id };
            let (input, error) = parse_tool_args(&c.args);
            blocks.push(Block::ToolUse { id, name: c.name, input, error });
        }
        let has_tools = blocks.iter().any(|b| matches!(b, Block::ToolUse { .. }));
        let stop = match this.finish.as_deref() {
            Some("length") => Stop::MaxTokens,
            Some("tool_calls") | Some("function_call") if has_tools => Stop::ToolUse,
            // Some servers finish a tool call with "stop".
            Some("stop") | None if has_tools => Stop::ToolUse,
            Some("stop") | None => Stop::EndTurn,
            Some(other) => Stop::Other(other.to_string()),
        };
        // A reply cut at the token limit may hold a half-written call; never run that.
        let blocks = if stop == Stop::MaxTokens { blocks.into_iter().filter(|b| !matches!(b, Block::ToolUse { .. })).collect() } else { blocks };
        Ok(AssistantTurn { blocks, stop, usage: this.usage })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(events: &[&str]) -> Result<(AssistantTurn, Vec<Delta>), String> {
        let mut d: Box<dyn Decoder> = Box::new(OpenAiDecoder::default());
        let mut deltas = Vec::new();
        for e in events {
            d.event(&SseEvent { name: None, data: e.to_string() }, &mut |x| deltas.push(x))?;
        }
        Ok((d.finish()?, deltas))
    }

    #[test]
    fn text_then_a_tool_call_in_pieces() {
        let (turn, deltas) = run(&[
            r#"{"choices":[{"delta":{"role":"assistant","content":"Let me "}}]}"#,
            r#"{"choices":[{"delta":{"content":"look."}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"inspect_design","arguments":""}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"meas"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ure\":false}"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call_b","function":{"name":"undo","arguments":"{}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":50,"completion_tokens":9,"prompt_tokens_details":{"cached_tokens":20}}}"#,
            "[DONE]",
        ])
        .unwrap();
        assert_eq!(turn.stop, Stop::ToolUse);
        assert_eq!(turn.usage, Usage { input: 50, output: 9, cache_read: 20, cache_write: 0 });
        assert_eq!(
            turn.blocks,
            vec![
                Block::Text("Let me look.".into()),
                Block::ToolUse { id: "call_a".into(), name: "inspect_design".into(), input: json!({"measure": false}), error: None },
                Block::ToolUse { id: "call_b".into(), name: "undo".into(), input: json!({}), error: None },
            ]
        );
        assert_eq!(deltas.iter().filter(|d| matches!(d, Delta::ToolStart { .. })).count(), 2);
    }

    #[test]
    fn servers_without_ids_or_with_stop_after_calls() {
        let (turn, _) = run(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"undo","arguments":"{}"}}]},"finish_reason":"stop"}]}"#,
            "[DONE]",
        ])
        .unwrap();
        assert_eq!(turn.stop, Stop::ToolUse);
        assert!(matches!(&turn.blocks[0], Block::ToolUse { id, .. } if id == "call_0"));
    }

    #[test]
    fn malformed_arguments_are_recorded() {
        let (turn, _) = run(&[r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"x","function":{"name":"execute","arguments":"{command: solid.box"}}]},"finish_reason":"tool_calls"}]}"#, "[DONE]"]).unwrap();
        assert!(matches!(&turn.blocks[0], Block::ToolUse { error: Some(_), input, .. } if input == &json!({})));
    }

    #[test]
    fn a_cut_off_reply_runs_no_tool() {
        let (turn, _) = run(&[r#"{"choices":[{"delta":{"content":"hi","tool_calls":[{"index":0,"id":"x","function":{"name":"execute","arguments":"{\"com"}}]},"finish_reason":"length"}]}"#, "[DONE]"]).unwrap();
        assert_eq!(turn.stop, Stop::MaxTokens);
        assert_eq!(turn.blocks, vec![Block::Text("hi".into())]);
    }

    #[test]
    fn errors_and_truncated_streams() {
        assert!(run(&[r#"{"error":{"message":"model not found"}}"#]).unwrap_err().contains("model not found"));
        assert!(run(&[r#"{"choices":[{"delta":{"content":"x"}}]}"#]).is_err());
        let mut d = OpenAiDecoder::default();
        assert!(d.event(&SseEvent { name: None, data: "{oops".into() }, &mut |_| {}).is_err());
    }

    #[test]
    fn hostile_events_do_not_panic() {
        for ev in [
            r#"{"choices":"x"}"#,
            r#"{"choices":[null]}"#,
            r#"{"choices":[{"delta":{"tool_calls":"x"}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":99999999999999}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[1,null,{"function":5}]}}]}"#,
            r#"[1]"#,
            r#""s""#,
            "null",
        ] {
            let mut d = OpenAiDecoder::default();
            let _ = d.event(&SseEvent { name: None, data: ev.into() }, &mut |_| {});
        }
    }

    #[test]
    fn the_request_body_keeps_tool_pairs_and_moves_pictures() {
        let tools = vec![ToolDef { name: "a".into(), description: "A".into(), schema: json!({"type": "object"}) }];
        let messages = vec![
            Message::user_text("hi"),
            Message::assistant(vec![Block::ToolUse { id: "c1".into(), name: "a".into(), input: json!({"x": 1}), error: None }]),
            Message {
                role: Role::User,
                blocks: vec![Block::ToolResult {
                    id: "c1".into(),
                    parts: vec![Part::Text("ok".into()), Part::Image { mime: "image/png".into(), data: "AA".into() }],
                    is_error: false,
                }],
            },
        ];
        let req = ChatRequest { model: "m", system: "sys", messages: &messages, tools: &tools, vision: true, max_tokens: 10 };
        let b = body(&req);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][2]["role"], "assistant");
        assert!(b["messages"][2]["content"].is_null());
        assert_eq!(b["messages"][2]["tool_calls"][0]["function"]["arguments"], "{\"x\":1}");
        assert_eq!(b["messages"][3]["role"], "tool");
        assert_eq!(b["messages"][3]["tool_call_id"], "c1");
        assert_eq!(b["messages"][4]["role"], "user");
        assert!(b["messages"][4]["content"][0]["image_url"]["url"].as_str().unwrap_or("").starts_with("data:image/png;base64,"));
        assert_eq!(b["stream_options"]["include_usage"], true);
        let req = ChatRequest { vision: false, ..req };
        assert_eq!(body(&req)["messages"].as_array().map(Vec::len), Some(4));
    }

    #[test]
    fn the_key_is_optional() {
        let req = ChatRequest { model: "m", system: "s", messages: &[], tools: &[], vision: false, max_tokens: 1 };
        let r = OpenAi.request(&req, "http://localhost:11434/v1/", &Secret::default()).unwrap();
        assert_eq!(r.url, "http://localhost:11434/v1/chat/completions");
        assert!(!r.headers.iter().any(|(n, _)| n == "authorization"));
        let r = OpenAi.request(&req, "https://api.openai.com/v1", &Secret::new("sk")).unwrap();
        assert!(r.headers.iter().any(|(n, v)| n == "authorization" && v.expose() == "Bearer sk"));
    }
}
