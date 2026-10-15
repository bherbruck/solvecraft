//! The sidebar's logic against the real app: a scripted service answers, the worker's tool calls
//! go through the gate, and the design changes only as the policy allows.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use solidcraft_assistant::net::{Cancel, HttpError, HttpRequest, HttpStream};
use solidcraft_assistant::settings::{BackendKind, Settings};

use super::*;
use crate::scenario::Harness;

struct Service {
    replies: Mutex<VecDeque<String>>,
    seen: Mutex<Vec<Value>>,
}

fn sse(chunks: &[Value]) -> String {
    let mut s: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
    s.push_str("data: [DONE]\n\n");
    s
}

fn calls(calls: &[(&str, &str, Value)]) -> String {
    let tc: Vec<Value> = calls
        .iter()
        .enumerate()
        .map(|(i, (id, name, args))| json!({"index": i, "id": id, "function": {"name": name, "arguments": args.to_string()}}))
        .collect();
    sse(&[json!({"choices": [{"delta": {"tool_calls": tc}, "finish_reason": "tool_calls"}]})])
}

fn say(text: &str) -> String {
    sse(&[
        json!({"choices": [{"delta": {"content": text}, "finish_reason": "stop"}]}),
        json!({"choices": [], "usage": {"prompt_tokens": 10, "completion_tokens": 4}}),
    ])
}

fn install(h: &mut Harness, replies: Vec<String>) -> Arc<Service> {
    let svc = Arc::new(Service { replies: Mutex::new(replies.into()), seen: Mutex::new(Vec::new()) });
    let me = svc.clone();
    let http: HttpStream = Arc::new(move |req: &HttpRequest, _c: &Cancel, sink: &mut dyn FnMut(&[u8]) -> bool| {
        me.seen.lock().unwrap().push(serde_json::from_slice(&req.body).unwrap_or(Value::Null));
        let reply = me.replies.lock().unwrap().pop_front().ok_or_else(|| HttpError::Transport("no scripted reply left".into()))?;
        for chunk in reply.as_bytes().chunks(23) {
            if !sink(chunk) {
                break;
            }
        }
        Ok(())
    });
    h.app.services.assistant_http = Some(http);
    let mut s = Settings { backend: BackendKind::OpenAi, ..Settings::default() };
    s.openai.base_url = "http://127.0.0.1:9/v1".into();
    s.openai.model = "test-model".into();
    h.app.assistant.set_settings(s);
    h.app.assistant.data_root = Some(h.dir.join("assistant"));
    svc
}

fn wait_idle(h: &mut Harness) {
    for _ in 0..4000 {
        h.frame();
        if !h.app.assistant.busy() {
            // One more frame to show the last state.
            h.frame();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("the turn did not finish: {}", summary(&h.app));
}

fn bodies(h: &mut Harness) -> usize {
    h.app.session.execute("document.inspect", &json!({})).ok().and_then(|v| v["bodies"].as_array().map(Vec::len)).unwrap_or(0)
}

fn send_ok(h: &mut Harness, text: &str) {
    let r = h.call("ui.assistant", json!({"action": "send", "text": text}));
    assert_eq!(r["ok"], true, "{r}");
}

const BOX: &str = r#"{"command": "solid.box", "params": {"length": 10, "width": 20, "height": 30}}"#;

fn box_args() -> Value {
    serde_json::from_str(BOX).unwrap()
}

#[test]
fn a_message_builds_a_box_and_the_turn_can_be_undone() {
    let mut h = Harness::new();
    let svc = install(&mut h, vec![calls(&[("c1", "execute", box_args())]), say("Made a 10 x 20 x 30 box.")]);
    send_ok(&mut h, "make a box");
    wait_idle(&mut h);
    assert_eq!(bodies(&mut h), 1);
    let s = summary(&h.app);
    let kinds: Vec<&str> = s["items"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["user", "tool", "assistant", "turn_end"], "{s}");
    assert_eq!(s["items"][2]["text"], "Made a 10 x 20 x 30 box.");
    assert_eq!(s["items"][1]["result"]["is_error"], false);
    assert!(s["undoable"]["edited_since"] == false, "{s}");
    assert_eq!(s["usage"]["input"], 10);
    // The design state went with the person's message.
    let first = svc.seen.lock().unwrap()[0].clone();
    let user = first["messages"].as_array().unwrap().iter().find(|m| m["role"] == "user").unwrap();
    assert!(user["content"].to_string().contains("<design_state>"), "{user}");
    // Undo the turn.
    let r = h.call("ui.assistant", json!({"action": "undo_turn"}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(bodies(&mut h), 0);
    assert!(summary(&h.app)["undoable"].is_null());
}

#[test]
fn a_turn_that_changes_nothing_keeps_no_checkpoint() {
    let mut h = Harness::new();
    install(&mut h, vec![say("Hello! The design is empty.")]);
    send_ok(&mut h, "hi");
    wait_idle(&mut h);
    let s = summary(&h.app);
    assert_eq!(s["items"].as_array().unwrap().len(), 2, "{s}");
    assert!(s["undoable"].is_null());
    assert!(!h.app.session.has_checkpoint(1));
}

#[test]
fn replacing_the_design_waits_for_a_yes() {
    let mut h = Harness::new();
    install(&mut h, vec![calls(&[("c1", "execute", box_args())]), calls(&[("c2", "new_design", json!({}))]), say("Started over.")]);
    send_ok(&mut h, "make a box then start over");
    // Run until the request for new_design is held.
    let mut held = false;
    for _ in 0..4000 {
        h.frame();
        if !h.app.assistant.confirms.is_empty() {
            held = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(held, "the file command was not held: {}", summary(&h.app));
    assert_eq!(bodies(&mut h), 1, "nothing ran yet");
    let s = summary(&h.app);
    assert_eq!(s["confirms"][0][0], "file.new");
    // Declined: the model hears so, the box stays.
    let r = h.call("ui.assistant", json!({"action": "confirm", "index": 0, "allow": false}));
    assert_eq!(r["ok"], true, "{r}");
    wait_idle(&mut h);
    assert_eq!(bodies(&mut h), 1);
    let s = summary(&h.app);
    let tool = s["items"].as_array().unwrap().iter().filter(|i| i["kind"] == "tool").nth(1).unwrap().clone();
    assert_eq!(tool["result"]["is_error"], true);
    assert!(tool["result"]["text"].as_str().unwrap().contains("declined"), "{tool}");
}

#[test]
fn an_allowed_file_command_runs() {
    let mut h = Harness::new();
    install(&mut h, vec![calls(&[("c1", "execute", box_args())]), calls(&[("c2", "new_design", json!({}))]), say("Fresh design.")]);
    send_ok(&mut h, "start over");
    for _ in 0..4000 {
        h.frame();
        if !h.app.assistant.confirms.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let r = h.call("ui.assistant", json!({"action": "confirm", "allow": true}));
    assert_eq!(r["ok"], true, "{r}");
    wait_idle(&mut h);
    assert_eq!(bodies(&mut h), 0);
}

#[test]
fn a_file_outside_the_assistants_folders_is_refused_even_when_allowed() {
    let mut h = Harness::new();
    let outside = std::env::temp_dir().join(format!("solidcraft-assistant-outside-{}.step", std::process::id()));
    let _ = std::fs::remove_file(&outside);
    install(
        &mut h,
        vec![
            calls(&[("c1", "execute", box_args())]),
            calls(&[("c2", "export", json!({"path": outside.to_string_lossy(), "format": "step"}))]),
            say("done"),
        ],
    );
    send_ok(&mut h, "export it");
    for _ in 0..4000 {
        h.frame();
        if !h.app.assistant.confirms.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(summary(&h.app)["confirms"][0][0].as_str().unwrap().split(':').next(), Some("file.export"));
    h.call("ui.assistant", json!({"action": "confirm", "allow": true}));
    wait_idle(&mut h);
    assert!(!outside.exists(), "the file was written outside the assistant's folders");
    let s = summary(&h.app);
    let export = s["items"].as_array().unwrap().iter().filter(|i| i["kind"] == "tool").nth(1).unwrap().clone();
    assert_eq!(export["result"]["is_error"], true, "{export}");
}

#[test]
fn raw_control_methods_and_stale_turns_are_refused() {
    let mut h = Harness::new();
    // The turn stays alive: the model's `new_design` waits for a yes.
    install(&mut h, vec![calls(&[("c1", "new_design", json!({}))]), say("ok")]);
    let tx = h.app.assistant.inlet_tx.clone();
    // No turn is running: everything is refused.
    let (req, reply) = ControlRequest::new("engine.execute", json!({"command": "solid.box", "_turn": 1}));
    tx.send(req).unwrap();
    h.frames(2);
    assert_eq!(reply.try_recv().unwrap()["ok"], false);
    assert_eq!(bodies(&mut h), 0);
    // With a turn running: a method outside the allowlist is refused, and a forged `_turn` too.
    send_ok(&mut h, "hi");
    for _ in 0..4000 {
        h.frame();
        if !h.app.assistant.confirms.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let id = h.app.assistant.active.as_ref().map(|a| a.id).expect("the turn is waiting for a yes");
    for (method, params) in [
        ("app.quit", json!({"_turn": id})),
        ("ui.prefs", json!({"_turn": id})),
        ("engine.script", json!({"_turn": id, "commands": []})),
        ("document.inspect", json!({"_turn": id + 5})),
    ] {
        let (req, reply) = ControlRequest::new(method, params);
        tx.send(req).unwrap();
        let mut answered = false;
        for _ in 0..200 {
            h.frame();
            if let Ok(v) = reply.try_recv() {
                assert_eq!(v["ok"], false, "{method}: {v}");
                answered = true;
                break;
            }
        }
        assert!(answered, "{method} was not answered");
    }
    assert!(!h.app.quit_requested);
    h.call("ui.assistant", json!({"action": "stop"}));
    wait_idle(&mut h);
}

#[test]
fn stopping_ends_the_turn_and_declines_what_waited() {
    let mut h = Harness::new();
    // The model asks to replace the design; the request waits for a yes.
    install(&mut h, vec![calls(&[("c1", "new_design", json!({}))]), say("never shown")]);
    send_ok(&mut h, "go");
    for _ in 0..4000 {
        h.frame();
        if !h.app.assistant.confirms.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(h.app.assistant.confirms.len(), 1);
    let r = h.call("ui.assistant", json!({"action": "stop"}));
    assert_eq!(r["ok"], true);
    assert!(h.app.assistant.confirms.is_empty());
    wait_idle(&mut h);
    let s = summary(&h.app);
    let items = s["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["kind"] == "notice" && i["text"] == "Stopped."), "{s}");
    assert!(!items.iter().any(|i| i["kind"] == "assistant"), "{s}");
}

#[test]
fn missing_settings_say_what_to_do() {
    let mut h = Harness::new();
    let mut s = Settings { backend: BackendKind::Anthropic, ..Settings::default() };
    s.anthropic.api_key = solidcraft_assistant::settings::Secret::default();
    h.app.assistant.set_settings(s);
    h.app.assistant.data_root = Some(h.dir.join("assistant"));
    install_http_only(&mut h);
    // SAFETY of the test: the environment may hold a key; only assert when it does not.
    if std::env::var("ANTHROPIC_API_KEY").is_err() && solidcraft_engine::geom::env::var("ANTHROPIC_API_KEY").is_none() {
        let r = h.call("ui.assistant", json!({"action": "send", "text": "hi"}));
        assert_eq!(r["ok"], false);
        assert!(r["error"].as_str().unwrap().contains("API key"), "{r}");
    }
    let r = h.call("ui.assistant", json!({"action": "send", "text": "   "}));
    assert_eq!(r["ok"], false);
    let r = h.call("ui.assistant", json!({"action": "send", "text": "/nonsense"}));
    assert_eq!(r["ok"], false);
}

fn install_http_only(h: &mut Harness) {
    let http: HttpStream = Arc::new(|_r: &HttpRequest, _c: &Cancel, _s: &mut dyn FnMut(&[u8]) -> bool| Err(HttpError::Transport("offline".into())));
    h.app.services.assistant_http = Some(http);
}

#[test]
fn the_web_build_has_no_transport() {
    let mut h = Harness::new();
    let mut s = Settings { backend: BackendKind::OpenAi, ..Settings::default() };
    s.openai.model = "m".into();
    h.app.assistant.set_settings(s);
    h.app.services.assistant_http = None;
    let r = h.call("ui.assistant", json!({"action": "send", "text": "hi"}));
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap().contains("desktop app"), "{r}");
}

#[test]
fn quick_actions_without_a_model() {
    let mut h = Harness::new();
    install(&mut h, vec![]);
    let r = h.call("ui.assistant", json!({"action": "send", "text": "/help"}));
    assert_eq!(r["ok"], true, "{r}");
    let items = r["result"]["items"].as_array().unwrap().clone();
    assert!(items.iter().any(|i| i["kind"] == "notice" && i["text"].as_str().unwrap().contains("/explain")), "{r}");
    let r = h.call("ui.assistant", json!({"action": "send", "text": "/new"}));
    assert_eq!(r["result"]["items"].as_array().unwrap().len(), 0);
}

#[test]
fn the_toggle_is_a_ui_state() {
    let mut h = Harness::new();
    assert_eq!(h.call("ui.assistant", json!({"action": "state"}))["result"]["open"], false);
    assert_eq!(h.call("ui.assistant", json!({"action": "toggle"}))["result"]["open"], true);
    assert_eq!(h.call("ui.assistant", json!({"action": "close"}))["result"]["open"], false);
    assert_eq!(h.call("ui.assistant", json!({"action": "bogus"}))["ok"], false);
}

#[test]
fn conversations_are_saved_per_design_and_come_back() {
    let mut h = Harness::new();
    install(&mut h, vec![calls(&[("c1", "execute", box_args())]), say("Made the box.")]);
    send_ok(&mut h, "make a box");
    wait_idle(&mut h);
    let saved = saved_conversations(&h.app);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].title, "make a box");
    // A new chat is empty; the saved one comes back with its transcript.
    h.call("ui.assistant", json!({"action": "new_chat"}));
    assert_eq!(summary(&h.app)["items"].as_array().unwrap().len(), 0);
    assert!(load_conversation(&mut h.app, &saved[0].id));
    let s = summary(&h.app);
    let kinds: Vec<&str> = s["items"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["user", "tool", "assistant"], "{s}");
    assert_eq!(s["items"][1]["result"]["is_error"], false);
    // The model has the conversation: a follow-up carries it.
    assert!(export_text(&h.app).contains("## Assistant\n\nMade the box."));
}

#[test]
fn a_picture_goes_to_a_vision_model() {
    let mut h = Harness::new();
    let svc = install(&mut h, vec![say("It is a box.")]);
    h.app.assistant.settings.openai.vision = true;
    attach_view(&mut h.app);
    assert!(h.app.assistant.picture.is_some());
    send_ok(&mut h, "what is this?");
    wait_idle(&mut h);
    let body = svc.seen.lock().unwrap()[0].clone();
    let user = body["messages"].as_array().unwrap().iter().find(|m| m["role"] == "user").unwrap().to_string();
    assert!(user.contains("data:image/png;base64,"), "{}", &user[..200.min(user.len())]);
    assert!(h.app.assistant.picture.is_none());
}

// ---- external agents -----------------------------------------------------------------------

use std::io::{BufRead, Write};
use std::net::{TcpListener, TcpStream};

/// A stand-in for the app's agent endpoint: a loopback listener that checks the token, tags each
/// request with the running turn and forwards it into the inlet, as the desktop app does.
fn test_endpoint() -> StartEndpoint {
    Arc::new(|spec: EndpointSpec| {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let token = "0123456789abcdef0123456789abcdef".to_string();
        let expect = token.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (inlet, turn, ctx, expect) = (spec.inlet.clone(), spec.turn.clone(), spec.ctx.clone(), expect.clone());
                std::thread::spawn(move || {
                    let Ok(read) = stream.try_clone() else { return };
                    let mut out = stream;
                    for line in std::io::BufReader::new(read).lines().map_while(Result::ok) {
                        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
                        if msg["token"] != expect.as_str() {
                            let _ = writeln!(out, "{}", json!({"ok": false, "error": "unauthorized"}));
                            return;
                        }
                        let mut params = msg["params"].clone();
                        if let Some(o) = params.as_object_mut() {
                            o.insert("_turn".into(), json!(turn.load(Ordering::SeqCst)));
                        }
                        let (req, rx) = ControlRequest::new(msg["method"].as_str().unwrap_or(""), params);
                        if inlet.send(req).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                        let mut reply = rx.recv_timeout(Duration::from_secs(60)).unwrap_or_else(|_| json!({"ok": false, "error": "timeout"}));
                        reply["id"] = msg["id"].clone();
                        if writeln!(out, "{reply}").is_err() {
                            return;
                        }
                    }
                });
            }
        });
        Ok(Endpoint::new(port, token, || {}))
    })
}

/// The agent side of the test. The test binary starts itself with `SOLIDCRAFT_MOCK_ACP` set and
/// this test selected: it speaks ACP on stdin and stdout and models through the endpoint
/// named in the MCP server it is given, as SolidCraft's own MCP server would for a real agent.
#[test]
fn mock_acp_agent() {
    if std::env::var("SOLIDCRAFT_MOCK_ACP").is_err() {
        return;
    }
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    // libtest has written "test … ... " without a line break: start a line of our own.
    writeln!(out).unwrap();
    let mut say = |v: Value| {
        writeln!(out, "{v}").unwrap();
        out.flush().unwrap();
    };
    let (mut port, mut token) = (0u16, String::new());
    for line in stdin.lock().lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let id = v["id"].clone();
        match v["method"].as_str().unwrap_or("") {
            "initialize" => say(json!({"jsonrpc": "2.0", "id": id, "result": {"protocolVersion": 1, "agentCapabilities": {}, "authMethods": []}})),
            "session/new" => {
                let server = &v["params"]["mcpServers"][0];
                token = server["env"][0]["value"].as_str().unwrap_or("").to_string();
                port = server["args"][2].as_str().unwrap_or("").rsplit(':').next().unwrap_or("0").parse().unwrap_or(0);
                say(json!({"jsonrpc": "2.0", "id": id, "result": {"sessionId": "mock"}}));
            }
            "session/prompt" => {
                let mut conn = TcpStream::connect(("127.0.0.1", port)).unwrap();
                let mut reader = std::io::BufReader::new(conn.try_clone().unwrap());
                let mut n = 0;
                let mut call = |method: &str, params: Value, token: &str| -> Value {
                    n += 1;
                    writeln!(conn, "{}", json!({"id": n, "method": method, "params": params, "token": token})).unwrap();
                    let mut l = String::new();
                    reader.read_line(&mut l).unwrap();
                    serde_json::from_str(&l).unwrap()
                };
                say(
                    json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "mock", "update": {"sessionUpdate": "tool_call", "toolCallId": "k1", "title": "solidcraft: execute", "kind": "other", "status": "pending", "rawInput": {"command": "solid.box"}}}}),
                );
                // A wrong token: refused, and the connection is closed on it (its own connection).
                let bad = {
                    let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
                    writeln!(c, "{}", json!({"id": 1, "method": "engine.execute", "params": {"command": "solid.box"}, "token": "wrong-token"}))
                        .unwrap();
                    let mut l = String::new();
                    std::io::BufReader::new(c).read_line(&mut l).unwrap();
                    serde_json::from_str::<Value>(&l).unwrap_or(json!({"ok": false}))
                };
                let made = call("engine.execute", json!({"command": "solid.box", "params": {"length": 10, "width": 20, "height": 30}}), &token);
                say(
                    json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "mock", "update": {"sessionUpdate": "tool_call_update", "toolCallId": "k1", "status": "completed", "content": [{"type": "content", "content": {"type": "text", "text": "box made"}}]}}}),
                );
                let quit = call("app.quit", json!({}), &token);
                let new = call("engine.execute", json!({"command": "file.new", "params": {}}), &token);
                let text = format!(
                    "bad token: {}; box: {}; quit: {}; file.new: {}",
                    bad["ok"],
                    made["ok"],
                    quit["ok"],
                    new["error"].as_str().unwrap_or("ran")
                );
                say(
                    json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "mock", "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}}}),
                );
                say(json!({"jsonrpc": "2.0", "id": id, "result": {"stopReason": "end_turn"}}));
            }
            _ => {}
        }
    }
}

fn agent_profile() -> solidcraft_assistant::settings::AgentProfile {
    let exe = std::env::current_exe().unwrap().to_string_lossy().into_owned();
    solidcraft_assistant::settings::AgentProfile {
        name: "Mock agent".into(),
        command: exe,
        args: vec!["--exact".into(), "assistant_ui::tests::mock_acp_agent".into(), "--nocapture".into(), "--test-threads=1".into()],
        env: vec![("SOLIDCRAFT_MOCK_ACP".into(), solidcraft_assistant::settings::Secret::new("1"))],
        mode: solidcraft_assistant::settings::AgentMode::Acp,
    }
}

#[cfg(unix)]
#[test]
fn an_external_agent_models_through_the_gate() {
    let mut h = Harness::new();
    let mut s = Settings { backend: BackendKind::Agent, ..Settings::default() };
    s.agents.push(agent_profile());
    s.cli = "/usr/bin/solidcraft-cli".into();
    h.app.assistant.set_settings(s);
    h.app.assistant.data_root = Some(h.dir.join("assistant"));
    h.app.services.start_agent_endpoint = Some(test_endpoint());
    send_ok(&mut h, "make a box and then start over");
    // The agent's file.new is held until the person answers; decline it.
    let mut answered = false;
    for _ in 0..8000 {
        h.frame();
        if !answered && !h.app.assistant.confirms.is_empty() {
            let s = summary(&h.app);
            assert_eq!(s["confirms"][0][0], "file.new");
            let r = h.call("ui.assistant", json!({"action": "confirm", "index": 0, "allow": false}));
            assert_eq!(r["ok"], true, "{r}");
            answered = true;
        }
        if !h.app.assistant.busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(answered, "file.new never waited for the person: {}", summary(&h.app));
    assert!(!h.app.assistant.busy(), "{}", summary(&h.app));
    let s = summary(&h.app);
    assert_eq!(bodies(&mut h), 1, "{s}");
    assert!(!h.app.quit_requested);
    let items = s["items"].as_array().unwrap();
    let text = items.iter().filter(|i| i["kind"] == "assistant").map(|i| i["text"].as_str().unwrap().to_string()).collect::<String>();
    assert!(
        text.contains("bad token: false")
            && text.contains("box: true")
            && text.contains("quit: false")
            && text.contains("file.new: the user declined"),
        "{text}"
    );
    assert!(items.iter().any(|i| i["kind"] == "tool" && i["title"] == "solidcraft: execute" && i["result"]["text"] == "box made"), "{s}");
    // The turn changed the design, so it can be undone as one step.
    assert!(s["undoable"].is_object(), "{s}");
    let r = h.call("ui.assistant", json!({"action": "undo_turn"}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(bodies(&mut h), 0);
}

#[test]
fn an_agent_that_is_not_installed_says_so() {
    let mut h = Harness::new();
    let mut s = Settings { backend: BackendKind::Agent, ..Settings::default() };
    s.agents.push(solidcraft_assistant::settings::AgentProfile {
        name: "Missing".into(),
        command: "solidcraft-no-such-agent-xyz".into(),
        args: vec![],
        env: vec![],
        mode: solidcraft_assistant::settings::AgentMode::Acp,
    });
    s.cli = "/usr/bin/solidcraft-cli".into();
    h.app.assistant.set_settings(s);
    h.app.assistant.data_root = Some(h.dir.join("assistant"));
    h.app.services.start_agent_endpoint = Some(test_endpoint());
    send_ok(&mut h, "hi");
    wait_idle(&mut h);
    let s = summary(&h.app);
    let notice = s["items"].as_array().unwrap().iter().find(|i| i["kind"] == "notice").unwrap().clone();
    assert!(notice["text"].as_str().unwrap().contains("was not found"), "{notice}");
    assert_eq!(notice["error"], true);
}

#[test]
fn agents_need_the_desktop_app() {
    let mut h = Harness::new();
    let mut s = Settings { backend: BackendKind::Agent, ..Settings::default() };
    s.agents.push(agent_profile());
    h.app.assistant.set_settings(s);
    let r = h.call("ui.assistant", json!({"action": "send", "text": "hi"}));
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap().contains("desktop app"), "{r}");
}

#[test]
fn a_stuck_service_does_not_keep_the_sidebar_busy_after_stop() {
    let mut h = Harness::new();
    // A service that never answers and ignores the stop, as a stalled connection would.
    let http: HttpStream = Arc::new(|_r: &HttpRequest, _c: &Cancel, _s: &mut dyn FnMut(&[u8]) -> bool| {
        std::thread::sleep(Duration::from_secs(8));
        Err(HttpError::Transport("stalled".into()))
    });
    let mut s = Settings { backend: BackendKind::OpenAi, ..Settings::default() };
    s.openai.base_url = "http://127.0.0.1:9/v1".into();
    s.openai.model = "m".into();
    h.app.assistant.set_settings(s);
    h.app.assistant.data_root = Some(h.dir.join("assistant"));
    h.app.services.assistant_http = Some(http);
    send_ok(&mut h, "hi");
    assert!(h.app.assistant.busy());
    h.call("ui.assistant", json!({"action": "stop"}));
    let began = std::time::Instant::now();
    wait_idle(&mut h);
    assert!(began.elapsed() < Duration::from_secs(7), "{:?}", began.elapsed());
    assert!(h.app.assistant.history.is_empty());
    assert!(summary(&h.app)["items"].as_array().unwrap().iter().any(|i| i["kind"] == "notice" && i["text"] == "Stopped."));
    // The conversation is usable again.
    assert!(!h.app.assistant.busy());
}

#[test]
fn the_sidebar_takes_space_from_the_viewport() {
    let mut h = Harness::new();
    h.frames(2);
    let before = h.app.viewport.rect.expect("the viewport was laid out");
    h.call("ui.assistant", json!({"action": "open"}));
    h.frames(3);
    let panel = crate::scenario::rect_of("assistant:panel").expect("the panel was drawn");
    let after = h.app.viewport.rect.expect("the viewport was laid out");
    assert!(after.right() <= panel.left() + 0.5, "the viewport {after:?} reaches under the panel {panel:?}");
    assert!(after.width() < before.width());
    assert!(panel.width() >= super::panel::MIN_WIDTH - 1.0 && panel.width() <= super::panel::MAX_WIDTH + 1.0);
    // Closing gives the space back.
    h.call("ui.assistant", json!({"action": "close"}));
    h.frames(3);
    assert!((h.app.viewport.rect.map_or(0.0, |r| r.width()) - before.width()).abs() < 1.0);
}
