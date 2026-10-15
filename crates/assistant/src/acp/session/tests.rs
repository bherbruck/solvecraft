//! The client against a scripted agent on in-memory pipes.

use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::*;

/// A scripted agent: reads our lines, answers with what `script` returns for each.
fn agent(script: impl Fn(&Value) -> Vec<Value> + Send + 'static) -> (Wire, Arc<Mutex<Vec<Value>>>) {
    let (client_reader, mut agent_writer) = std::io::pipe().unwrap();
    let (agent_reader, client_writer) = std::io::pipe().unwrap();
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        let mut lines = BufReader::new(agent_reader).lines();
        // A banner first, as real agents print.
        let _ = writeln!(agent_writer, "Agent v0.1 starting…");
        while let Some(Ok(line)) = lines.next() {
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            log.lock().unwrap().push(v.clone());
            for out in script(&v) {
                if writeln!(agent_writer, "{out}").is_err() {
                    return;
                }
            }
        }
    });
    (Wire::new(Box::new(client_reader), Box::new(client_writer)), seen)
}

/// The first message the agent has received that matches `f`, waiting for it to arrive.
fn wait_for(seen: &Arc<Mutex<Vec<Value>>>, f: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..200 {
        if let Some(m) = seen.lock().unwrap().iter().find(|m| f(m)) {
            return m.clone();
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the agent never received the expected message: {:?}", seen.lock().unwrap());
}

fn reply(v: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": v["id"], "result": result})
}

fn update(u: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1", "update": u}})
}

fn start(wire: Wire) -> AgentSession {
    let mcp = McpServer {
        name: "solidcraft".into(),
        command: "/bin/solidcraft-cli".into(),
        args: vec!["mcp".into(), "--connect".into(), "127.0.0.1:5000".into()],
        env: vec![("SOLIDCRAFT_CONTROL_TOKEN".into(), "tok".into())],
    };
    AgentSession::start(wire, None, Default::default(), "/work", Some(&mcp), &Cancel::new()).unwrap()
}

fn handshake(v: &Value) -> Option<Vec<Value>> {
    match v["method"].as_str()? {
        "initialize" => Some(vec![reply(v, json!({"protocolVersion": 1, "agentCapabilities": {}, "authMethods": []}))]),
        "session/new" => Some(vec![reply(v, json!({"sessionId": "s1"}))]),
        _ => None,
    }
}

#[test]
fn the_handshake_passes_the_mcp_server_with_its_token_in_env() {
    let (wire, seen) = agent(|v| handshake(v).unwrap_or_default());
    let s = start(wire);
    assert_eq!(s.session_id, "s1");
    let log = seen.lock().unwrap();
    assert_eq!(log[0]["params"]["protocolVersion"], 1);
    assert_eq!(log[0]["params"]["clientCapabilities"]["terminal"], false);
    assert_eq!(log[0]["params"]["clientCapabilities"]["fs"]["readTextFile"], false);
    let server = &log[1]["params"]["mcpServers"][0];
    assert_eq!(server["name"], "solidcraft");
    assert_eq!(server["command"], "/bin/solidcraft-cli");
    assert_eq!(server["env"][0], json!({"name": "SOLIDCRAFT_CONTROL_TOKEN", "value": "tok"}));
    // The token is never in the arguments, where other users could read it.
    assert!(!server["args"].to_string().contains("tok"));
    assert_eq!(log[1]["params"]["cwd"], "/work");
}

#[test]
fn a_turn_streams_text_tools_and_permissions() {
    let (wire, seen) = agent(|v| {
        if let Some(r) = handshake(v) {
            return r;
        }
        match v["method"].as_str() {
            Some("session/prompt") => vec![
                update(json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "thinking"}})),
                update(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Making "}})),
                update(
                    json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "solidcraft: execute", "kind": "other", "status": "pending", "rawInput": {"command": "solid.box"}}),
                ),
                json!({"jsonrpc": "2.0", "id": 900, "method": "session/request_permission", "params": {"sessionId": "s1", "toolCall": {"toolCallId": "t1", "title": "solidcraft: execute", "kind": "other"},
                    "options": [{"optionId": "yes", "name": "Allow", "kind": "allow_once"}, {"optionId": "no", "name": "Reject", "kind": "reject_once"}]}}),
                update(
                    json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed", "content": [{"type": "content", "content": {"type": "text", "text": "created Body1"}}]}),
                ),
                json!({"jsonrpc": "2.0", "id": 901, "method": "fs/read_text_file", "params": {"path": "/etc/passwd"}}),
                update(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "a box."}})),
                update(
                    json!({"sessionUpdate": "plan", "entries": [{"content": "Make box", "status": "completed"}, {"content": "Fillet", "status": "pending"}]}),
                ),
                update(json!({"sessionUpdate": "some_future_update", "x": 1})),
                reply(v, json!({"stopReason": "end_turn"})),
            ],
            _ => vec![],
        }
    });
    let mut s = start(wire);
    let mut events = Vec::new();
    let mut asked = 0;
    let out = s.prompt(
        "make a box",
        None,
        &Cancel::new(),
        true,
        &mut |_| {
            asked += 1;
            None
        },
        &mut |e| events.push(e),
    );
    assert_eq!(out.finish, Finish::Done);
    assert_eq!(out.tool_calls, 1);
    assert_eq!(asked, 0, "SolidCraft's own tools are allowed without asking");
    assert!(events.contains(&Event::Thinking("thinking".into())));
    let text: String = events.iter().filter_map(|e| if let Event::Text(t) = e { Some(t.as_str()) } else { None }).collect();
    assert_eq!(text, "Making a box.");
    assert!(
        events.iter().any(|e| matches!(e, Event::ToolCall { name, input, .. } if name == "solidcraft: execute" && input["command"] == "solid.box"))
    );
    assert!(events.iter().any(|e| matches!(e, Event::ToolResult { is_error: false, text, .. } if text == "created Body1")));
    assert!(events.iter().any(|e| matches!(e, Event::Notice(n) if n.contains("[x] Make box") && n.contains("[ ] Fillet"))));
    // The permission was answered with the first allow-once option, and the file read refused.
    let perm = wait_for(&seen, |m| m["id"] == 900);
    assert_eq!(perm["result"]["outcome"], json!({"outcome": "selected", "optionId": "yes"}));
    let fs = wait_for(&seen, |m| m["id"] == 901);
    assert_eq!(fs["error"]["code"], -32601);
}

#[test]
fn other_tools_ask_the_person_and_a_no_is_a_cancelled_outcome() {
    let (wire, seen) = agent(|v| {
        if let Some(r) = handshake(v) {
            return r;
        }
        match v["method"].as_str() {
            Some("session/prompt") => vec![
                json!({"jsonrpc": "2.0", "id": 5, "method": "session/request_permission", "params": {"sessionId": "s1", "toolCall": {"toolCallId": "t", "title": "Run rm -rf", "kind": "execute", "rawInput": {"command": "rm -rf ~"}},
                    "options": [{"optionId": "y", "name": "Allow", "kind": "allow_once"}]}}),
                reply(v, json!({"stopReason": "end_turn"})),
            ],
            _ => vec![],
        }
    });
    let mut s = start(wire);
    let mut asked: Vec<PermissionRequest> = Vec::new();
    let out = s.prompt(
        "hi",
        None,
        &Cancel::new(),
        true,
        &mut |r| {
            asked.push(r.clone());
            None
        },
        &mut |_| {},
    );
    assert_eq!(out.finish, Finish::Done);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].title, "Run rm -rf");
    assert!(!asked[0].is_solidcraft_tool());
    assert_eq!(wait_for(&seen, |m| m["id"] == 5)["result"]["outcome"], json!({"outcome": "cancelled"}));
}

#[test]
fn cancel_sends_session_cancel_and_ends_with_cancelled() {
    // Ids: initialize 1, session/new 2, prompt 3. The prompt is answered only after the cancel.
    let (wire, seen) = agent(|v| {
        if let Some(r) = handshake(v) {
            return r;
        }
        match v["method"].as_str() {
            Some("session/cancel") => vec![json!({"jsonrpc": "2.0", "id": 3, "result": {"stopReason": "cancelled"}})],
            _ => vec![],
        }
    });
    let mut s = start(wire);
    let cancel = Cancel::new();
    let c = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        c.cancel();
    });
    let out = s.prompt("go", None, &cancel, true, &mut |_| None, &mut |_| {});
    assert_eq!(out.finish, Finish::Cancelled);
    wait_for(&seen, |m| m["method"] == "session/cancel");
    assert!(s.usable(), "an agent that answered the cancel can be used again");
}

#[test]
fn an_agent_that_exits_mid_turn_is_reported() {
    let (wire, _) = agent(|v| {
        if let Some(r) = handshake(v) {
            return r;
        }
        vec![]
    });
    // The agent never answers the prompt and ignores the cancel.
    let mut s = start(wire);
    let out = {
        let cancel = Cancel::new();
        let c = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            c.cancel();
        });
        s.prompt("x", None, &cancel, true, &mut |_| None, &mut |_| {})
    };
    // No answer comes; the cancel grace (5 s) ends the turn and the session is unusable.
    assert!(matches!(out.finish, Finish::Cancelled | Finish::Error(_)), "{:?}", out.finish);
    assert!(!s.usable());
}

#[test]
fn a_handshake_failure_has_the_agents_words() {
    let (wire, _) = agent(|v| match v["method"].as_str() {
        Some("initialize") => vec![json!({"jsonrpc": "2.0", "id": v["id"], "error": {"code": -32000, "message": "unsupported protocol version"}})],
        _ => vec![],
    });
    let e = AgentSession::start(wire, None, Default::default(), "/w", None, &Cancel::new()).err().unwrap();
    assert!(e.contains("unsupported protocol version"), "{e}");
}
