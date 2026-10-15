//! The loop against the real engine: a scripted model drives a headless session through the
//! MCP tools, and everything it learns comes back through the same tools.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use solidcraft_mcp::{Backend, Headless};

use super::*;
use crate::net::{HttpError, HttpRequest};
use crate::provider::anthropic::Anthropic;

struct HeadlessHost(Mutex<Headless>);

impl HeadlessHost {
    fn new() -> Self {
        Self(Mutex::new(Headless::default()))
    }

    fn bodies(&self) -> usize {
        let mut h = self.0.lock().unwrap();
        h.call("document.inspect", json!({"measure": false})).unwrap()["bodies"].as_array().map(Vec::len).unwrap_or(0)
    }
}

impl ToolHost for HeadlessHost {
    fn call(&self, method: &str, params: Value, _cancel: &Cancel) -> Result<Value, String> {
        self.0.lock().unwrap().call(method, params)
    }
}

/// A scripted service: each request gets the next canned reply; request bodies are recorded.
struct Script {
    replies: Mutex<VecDeque<Result<Vec<u8>, HttpError>>>,
    seen: Mutex<Vec<Value>>,
    on_reply: Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl Script {
    fn new(replies: Vec<Result<Vec<u8>, HttpError>>) -> Arc<Self> {
        Arc::new(Self { replies: Mutex::new(replies.into()), seen: Mutex::new(Vec::new()), on_reply: Mutex::new(None) })
    }

    fn stream(self: &Arc<Self>) -> HttpStream {
        let me = self.clone();
        Arc::new(move |req: &HttpRequest, cancel: &Cancel, sink: &mut dyn FnMut(&[u8]) -> bool| {
            me.seen.lock().unwrap().push(serde_json::from_slice(&req.body).unwrap());
            let reply =
                me.replies.lock().unwrap().pop_front().unwrap_or_else(|| Err(HttpError::Transport("the script ran out of replies".into())))?;
            for (i, chunk) in reply.chunks(7).enumerate() {
                if let Some(f) = me.on_reply.lock().unwrap().as_ref().filter(|_| i == 2) {
                    f();
                }
                if cancel.is_cancelled() || !sink(chunk) {
                    break;
                }
            }
            Ok(())
        })
    }
}

fn sse(events: &[Value]) -> Vec<u8> {
    events.iter().map(|e| format!("event: {}\ndata: {}\n\n", e["type"].as_str().unwrap_or("x"), e)).collect::<String>().into_bytes()
}

fn text_reply(text: &str) -> Result<Vec<u8>, HttpError> {
    Ok(sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 10, "output_tokens": 1}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 5}}),
        json!({"type": "message_stop"}),
    ]))
}

fn tool_reply(calls: &[(&str, &str, &str)]) -> Result<Vec<u8>, HttpError> {
    let mut ev = vec![json!({"type": "message_start", "message": {"usage": {"input_tokens": 10, "output_tokens": 1}}})];
    for (i, (id, name, args)) in calls.iter().enumerate() {
        ev.push(json!({"type": "content_block_start", "index": i, "content_block": {"type": "tool_use", "id": id, "name": name, "input": {}}}));
        ev.push(json!({"type": "content_block_delta", "index": i, "delta": {"type": "input_json_delta", "partial_json": args}}));
        ev.push(json!({"type": "content_block_stop", "index": i}));
    }
    ev.push(json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 20}}));
    ev.push(json!({"type": "message_stop"}));
    Ok(sse(&ev))
}

struct Fixture {
    tools: Vec<ToolDef>,
    key: Secret,
}

impl Fixture {
    fn new() -> Self {
        Self { tools: crate::tools::definitions(false), key: Secret::new("sk-test") }
    }

    fn run(
        &self,
        script: &Arc<Script>,
        host: &dyn ToolHost,
        history: &mut Vec<Message>,
        text: &str,
        cancel: &Cancel,
        max_steps: usize,
    ) -> (TurnOutcome, Vec<Event>) {
        let http = script.stream();
        let cfg = TurnConfig {
            codec: &Anthropic,
            http: &http,
            base_url: "https://api.anthropic.com",
            allow_http: false,
            key: &self.key,
            model: "test-model",
            system: "system prompt",
            tools: &self.tools,
            vision: false,
            max_tokens: 1000,
            max_steps,
            context_window: 200_000,
            folders: &[],
        };
        let mut events = Vec::new();
        let out = run_turn(&cfg, history, Message::user_text(text), host, cancel, &mut |e| events.push(e));
        (out, events)
    }
}

const BOX_BATCH: &str = r#"{"commands": [
    {"command": "sketch.create", "params": {"plane": "XY"}},
    {"command": "sketch.rectangle.two_point", "params": {"p0": [0, 0], "p1": [40, 30]}},
    {"command": "sketch.finish"},
    {"command": "solid.extrude", "params": {"distance": 20}}
]}"#;

#[test]
fn a_scripted_model_builds_and_measures_a_box() {
    let host = HeadlessHost::new();
    let script = Script::new(vec![
        tool_reply(&[("t1", "batch", BOX_BATCH)]),
        tool_reply(&[("t2", "measure", "{}")]),
        text_reply("The box is 40 x 30 x 20 mm."),
    ]);
    let mut history = Vec::new();
    let (out, events) = Fixture::new().run(&script, &host, &mut history, "make a 40x30x20 box", &Cancel::new(), MAX_STEPS);
    assert_eq!(out.finish, Finish::Done, "{events:?}");
    assert_eq!((out.steps, out.tool_calls), (3, 2));
    assert_eq!(out.usage.input, 30);
    assert_eq!(host.bodies(), 1);
    // user, assistant(tool), user(result), assistant(tool), user(result), assistant(text)
    assert_eq!(history.len(), 6);
    let measured = events.iter().find_map(|e| match e {
        Event::ToolResult { name, text, is_error: false, .. } if name == "measure" => Some(text.clone()),
        _ => None,
    });
    let text = measured.expect("measure result");
    assert!(text.contains("24000") || text.contains("2.4e4"), "{text}");
    // The second request carried the first tool result back to the model.
    let seen = script.seen.lock().unwrap();
    assert_eq!(seen[1]["messages"][2]["content"][0]["type"], "tool_result");
    assert_eq!(seen[1]["messages"][2]["content"][0]["tool_use_id"], "t1");
    assert_eq!(seen[0]["tools"].as_array().map(|t| t.iter().any(|x| x["name"] == "screenshot")), Some(false));
}

#[test]
fn a_failing_batch_rolls_back_and_the_model_sees_why() {
    let host = HeadlessHost::new();
    let bad = r#"{"commands": [
        {"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}},
        {"command": "no.such.command", "params": {}}
    ]}"#;
    let script = Script::new(vec![tool_reply(&[("t1", "batch", bad)]), text_reply("That failed, nothing changed.")]);
    let mut history = Vec::new();
    let (out, events) = Fixture::new().run(&script, &host, &mut history, "go", &Cancel::new(), MAX_STEPS);
    assert_eq!(out.finish, Finish::Done);
    assert_eq!(host.bodies(), 0, "the box was undone");
    let failed = events.iter().find_map(|e| match e {
        Event::ToolResult { is_error: true, text, .. } => Some(text.clone()),
        _ => None,
    });
    assert!(failed.expect("an error result").contains("no.such.command"));
}

#[test]
fn malformed_tool_arguments_are_answered_not_run() {
    let host = HeadlessHost::new();
    let script = Script::new(vec![tool_reply(&[("t1", "execute", "{command: solid.box")]), text_reply("sorry")]);
    let mut history = Vec::new();
    let (out, events) = Fixture::new().run(&script, &host, &mut history, "go", &Cancel::new(), MAX_STEPS);
    assert_eq!((out.finish, out.tool_calls), (Finish::Done, 0));
    assert!(events.iter().any(|e| matches!(e, Event::ToolResult { is_error: true, text, .. } if text.contains("not valid JSON"))));
    assert_eq!(host.bodies(), 0);
}

#[test]
fn a_first_request_that_fails_leaves_the_conversation_alone() {
    let host = HeadlessHost::new();
    let script = Script::new(vec![Err(HttpError::Status {
        code: 401,
        retry_after: None,
        body: r#"{"error":{"message":"invalid x-api-key sk-test"}}"#.into(),
    })]);
    let mut history = vec![Message::user_text("earlier"), Message::assistant(vec![Block::Text("hello".into())])];
    let (out, _) = Fixture::new().run(&script, &host, &mut history, "go", &Cancel::new(), MAX_STEPS);
    match out.finish {
        Finish::Error(m) => {
            assert!(m.contains("key was rejected"), "{m}");
            assert!(!m.contains("sk-test"), "the key must not be echoed: {m}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(history.len(), 2);
}

#[test]
fn stopping_mid_reply_keeps_nothing_of_it() {
    let host = HeadlessHost::new();
    let cancel = Cancel::new();
    let script = Script::new(vec![text_reply(&"long answer ".repeat(50))]);
    let c = cancel.clone();
    *script.on_reply.lock().unwrap() = Some(Box::new(move || c.cancel()));
    let mut history = Vec::new();
    let (out, _) = Fixture::new().run(&script, &host, &mut history, "go", &cancel, MAX_STEPS);
    assert_eq!(out.finish, Finish::Cancelled);
    assert!(history.is_empty());
}

#[test]
fn a_stop_between_tool_calls_still_answers_every_call() {
    struct StopOnFirst(Cancel);
    impl ToolHost for StopOnFirst {
        fn call(&self, _m: &str, _p: Value, _c: &Cancel) -> Result<Value, String> {
            self.0.cancel();
            Ok(json!([]))
        }
    }
    let cancel = Cancel::new();
    let script = Script::new(vec![tool_reply(&[("a", "list_commands", "{}"), ("b", "undo", "{}"), ("c", "undo", "{}")])]);
    let mut history = Vec::new();
    let (out, _) = Fixture::new().run(&script, &StopOnFirst(cancel.clone()), &mut history, "go", &cancel, MAX_STEPS);
    assert_eq!(out.finish, Finish::Cancelled);
    assert_eq!(out.tool_calls, 1);
    let Some(Message { blocks, .. }) = history.last() else { panic!("no history") };
    let ids: Vec<&str> = blocks.iter().filter_map(|b| if let Block::ToolResult { id, .. } = b { Some(id.as_str()) } else { None }).collect();
    assert_eq!(ids, ["a", "b", "c"], "every call answered, so the history stays valid");
}

#[test]
fn the_step_limit_ends_a_runaway_turn_with_a_valid_history() {
    let host = HeadlessHost::new();
    let script = Script::new((0..10).map(|i| tool_reply(&[(&format!("u{i}"), "undo", "{}")])).collect());
    let mut history = Vec::new();
    let (out, _) = Fixture::new().run(&script, &host, &mut history, "go", &Cancel::new(), 3);
    assert_eq!((out.finish, out.steps), (Finish::StepLimit, 3));
    assert!(matches!(history.last(), Some(Message { role: Role::User, blocks }) if matches!(blocks[0], Block::ToolResult { .. })));
}

#[test]
fn an_unencrypted_remote_address_is_refused_before_anything_is_sent() {
    let host = HeadlessHost::new();
    let script = Script::new(vec![text_reply("x")]);
    let http = script.stream();
    let f = Fixture::new();
    let cfg = TurnConfig {
        codec: &Anthropic,
        http: &http,
        base_url: "http://example.com",
        allow_http: false,
        key: &f.key,
        model: "m",
        system: "s",
        tools: &f.tools,
        vision: false,
        max_tokens: 10,
        max_steps: 3,
        context_window: 200_000,
        folders: &[],
    };
    let mut history = Vec::new();
    let out = run_turn(&cfg, &mut history, Message::user_text("hi"), &host, &Cancel::new(), &mut |_| {});
    assert!(matches!(out.finish, Finish::Error(_)));
    assert!(script.seen.lock().unwrap().is_empty());
    assert!(history.is_empty());
}

#[test]
fn old_pictures_are_not_sent_again() {
    let result = |id: &str| Message {
        role: Role::User,
        blocks: vec![Block::ToolResult {
            id: id.into(),
            parts: vec![Part::Text("t".into()), Part::Image { mime: "image/png".into(), data: "AA".into() }],
            is_error: false,
        }],
    };
    let mut h = vec![Message::user_text("a"), result("1"), Message::assistant(vec![Block::Text("x".into())]), result("2")];
    strip_old_pictures(&mut h);
    let has_image =
        |m: &Message| m.blocks.iter().any(|b| matches!(b, Block::ToolResult { parts, .. } if parts.iter().any(|p| matches!(p, Part::Image { .. }))));
    assert!(!has_image(&h[1]));
    assert!(has_image(&h[3]));
}

#[test]
fn big_results_are_cut() {
    let (parts, _) = limit(vec![Part::Text("x".repeat(MAX_RESULT * 3)), Part::Text("y".repeat(100))], false);
    let total: usize = parts.iter().map(|p| if let Part::Text(t) = p { t.len() } else { 0 }).sum();
    assert!(total < MAX_RESULT + 400, "{total}");
}

#[test]
fn titles_name_the_command() {
    assert_eq!(call_title("execute", &json!({"command": "solid.extrude"})), "execute solid.extrude");
    assert_eq!(
        call_title("batch", &json!({"commands": [{"command": "a"}, {"command": "b"}, {"command": "c"}, {"command": "d"}]})),
        "batch (4 steps: a, b, c, …)"
    );
    assert_eq!(call_title("measure", &json!({})), "measure");
    assert_eq!(call_title("execute", &json!(null)), "execute");
}

#[test]
fn a_stop_before_anything_was_sent_leaves_the_conversation_alone() {
    let host = HeadlessHost::new();
    let cancel = Cancel::new();
    cancel.cancel();
    let script = Script::new(vec![text_reply("never")]);
    let mut history = vec![Message::user_text("earlier"), Message::assistant(vec![Block::Text("hello".into())])];
    let (out, _) = Fixture::new().run(&script, &host, &mut history, "go", &cancel, MAX_STEPS);
    assert_eq!((out.finish, out.steps), (Finish::Cancelled, 0));
    assert_eq!(history.len(), 2);
    assert!(script.seen.lock().unwrap().is_empty());
}
