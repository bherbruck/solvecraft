//! JSON-RPC 2.0 over a pair of byte streams, one message per line.
//!
//! A reader thread parses what the agent writes and queues it; the caller takes messages from
//! the queue and answers. Everything from the agent is untrusted: lines are size-capped, lines
//! that are not JSON are ignored (agents print banners and logs), and the queue is bounded.

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

/// Longest message accepted from the agent.
pub const MAX_LINE: usize = 16 * 1024 * 1024;

/// Lines that are not protocol messages before the agent is considered broken.
const MAX_JUNK: usize = 10_000;

/// Messages waiting to be handled.
const QUEUE: usize = 4096;

/// A message from the agent.
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    Notification {
        method: String,
        params: Value,
    },
    /// The agent asks us for something; it waits for an answer with the same `id`.
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Response {
        id: Value,
        result: Result<Value, String>,
    },
    /// The stream ended or broke.
    Closed(String),
}

/// Writes messages to the agent. Cheap to clone; safe to use from several threads.
#[derive(Clone)]
pub struct Replier {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Replier {
    fn send(&self, v: &Value) -> Result<(), String> {
        let mut line = v.to_string();
        line.push('\n');
        let mut w = self.writer.lock().map_err(|_| "the agent connection is broken".to_string())?;
        w.write_all(line.as_bytes()).and_then(|()| w.flush()).map_err(|e| format!("cannot write to the agent: {e}"))
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    pub fn respond(&self, id: &Value, result: Value) -> Result<(), String> {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }

    pub fn respond_error(&self, id: &Value, code: i64, message: &str) -> Result<(), String> {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}))
    }
}

/// The connection to one agent.
pub struct Wire {
    replier: Replier,
    rx: Receiver<Incoming>,
    next_id: u64,
}

impl Wire {
    /// Start reading `reader` on its own thread; `writer` is where requests go.
    pub fn new(reader: Box<dyn Read + Send>, writer: Box<dyn Write + Send>) -> Wire {
        let (tx, rx) = sync_channel::<Incoming>(QUEUE);
        let spawned = std::thread::Builder::new().name("assistant-agent-reader".into()).spawn({
            let tx = tx.clone();
            move || read_loop(BufReader::new(reader), tx)
        });
        if spawned.is_err() {
            let _ = tx.send(Incoming::Closed("cannot start the reader".into()));
        }
        Wire { replier: Replier { writer: Arc::new(Mutex::new(writer)) }, rx, next_id: 1 }
    }

    pub fn replier(&self) -> Replier {
        self.replier.clone()
    }

    /// Send a request; its answer arrives later as [`Incoming::Response`] with the returned id.
    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = json!(self.next_id);
        self.next_id += 1;
        self.replier.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        Ok(id)
    }

    pub fn recv(&self, wait: Duration) -> Result<Incoming, RecvTimeoutError> {
        self.rx.recv_timeout(wait)
    }
}

fn read_loop(mut reader: BufReader<Box<dyn Read + Send>>, tx: SyncSender<Incoming>) {
    let mut junk = 0usize;
    loop {
        let line = match read_line(&mut reader, MAX_LINE) {
            Ok(Some(l)) => l,
            Ok(None) => {
                let _ = tx.send(Incoming::Closed("the agent closed its output".into()));
                return;
            }
            Err(e) => {
                let _ = tx.send(Incoming::Closed(e.to_string()));
                return;
            }
        };
        let text = String::from_utf8_lossy(&line);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let Some(msg) = parse_message(text) else {
            junk += 1;
            if junk > MAX_JUNK {
                let _ = tx.send(Incoming::Closed("the agent wrote too much that is not protocol".into()));
                return;
            }
            continue;
        };
        if tx.send(msg).is_err() {
            return;
        }
    }
}

/// One line, or `None` at the end of the stream; an error when it passes `max`.
fn read_line(r: &mut impl BufRead, max: usize) -> std::io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let (done, used) = {
            let available = r.fill_buf()?;
            if available.is_empty() {
                return Ok((!line.is_empty()).then_some(line));
            }
            match available.iter().position(|b| *b == b'\n') {
                Some(i) => {
                    line.extend_from_slice(available.get(..i).unwrap_or(&[]));
                    (true, i + 1)
                }
                None => {
                    line.extend_from_slice(available);
                    (false, available.len())
                }
            }
        };
        r.consume(used);
        if line.len() > max {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "the agent sent a message that is too large"));
        }
        if done {
            return Ok(Some(line));
        }
    }
}

/// A JSON-RPC message, or `None` for anything else.
pub fn parse_message(text: &str) -> Option<Incoming> {
    let v: Value = serde_json::from_str(text).ok()?;
    let o = v.as_object()?;
    if o.get("jsonrpc").and_then(Value::as_str).is_some_and(|j| j != "2.0") {
        return None;
    }
    let id = o.get("id").filter(|i| i.is_string() || i.is_number()).cloned();
    if let Some(method) = o.get("method").and_then(Value::as_str) {
        let params = o.get("params").cloned().unwrap_or(Value::Null);
        return Some(match id {
            Some(id) => Incoming::Request { id, method: method.to_string(), params },
            None => Incoming::Notification { method: method.to_string(), params },
        });
    }
    let id = id?;
    if let Some(e) = o.get("error") {
        let m = e.get("message").and_then(Value::as_str).unwrap_or("unknown error");
        return Some(Incoming::Response { id, result: Err(m.to_string()) });
    }
    o.contains_key("result").then(|| Incoming::Response { id, result: Ok(o.get("result").cloned().unwrap_or(Value::Null)) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_classified() {
        assert_eq!(
            parse_message(r#"{"jsonrpc":"2.0","method":"session/update","params":{"a":1}}"#),
            Some(Incoming::Notification { method: "session/update".into(), params: json!({"a": 1}) })
        );
        assert_eq!(
            parse_message(r#"{"jsonrpc":"2.0","id":7,"method":"x"}"#),
            Some(Incoming::Request { id: json!(7), method: "x".into(), params: Value::Null })
        );
        assert_eq!(
            parse_message(r#"{"jsonrpc":"2.0","id":"a","result":{"ok":true}}"#),
            Some(Incoming::Response { id: json!("a"), result: Ok(json!({"ok": true})) })
        );
        assert_eq!(
            parse_message(r#"{"jsonrpc":"2.0","id":1,"error":{"code":-1,"message":"nope"}}"#),
            Some(Incoming::Response { id: json!(1), result: Err("nope".into()) })
        );
        for junk in ["", "hello", "[1]", "{}", r#"{"jsonrpc":"1.0","method":"x"}"#, r#"{"id":[1],"result":1}"#, r#"{"id":1}"#, "null"] {
            assert_eq!(parse_message(junk), None, "{junk:?}");
        }
    }

    #[test]
    fn banners_and_junk_between_messages_are_ignored() {
        let (r, mut w) = std::io::pipe().unwrap();
        let wire = Wire::new(Box::new(r), Box::new(std::io::sink()));
        std::thread::spawn(move || {
            w.write_all(b"Welcome to Agent v1\n\n\x1b[32mready\x1b[0m\n").unwrap();
            w.write_all(br#"{"jsonrpc":"2.0","method":"ping"}"#).unwrap();
            w.write_all(b"\n").unwrap();
        });
        let m = wire.recv(Duration::from_secs(5)).unwrap();
        assert!(matches!(m, Incoming::Notification { ref method, .. } if method == "ping"));
        assert!(matches!(wire.recv(Duration::from_secs(5)).unwrap(), Incoming::Closed(_)));
    }

    #[test]
    fn an_oversized_line_closes_the_connection() {
        let mut data = vec![b'a'; 64];
        data.push(b'\n');
        let mut r = BufReader::new(&data[..]);
        assert!(read_line(&mut r, 16).is_err());
        let mut r = BufReader::new(&data[..]);
        assert_eq!(read_line(&mut r, 1024).unwrap().map(|l| l.len()), Some(64));
    }

    #[test]
    fn requests_get_increasing_ids_and_are_written_as_lines() {
        let (r, _w) = std::io::pipe().unwrap();
        let out = Arc::new(Mutex::new(Vec::<u8>::new()));
        struct Sink(Arc<Mutex<Vec<u8>>>);
        impl Write for Sink {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut wire = Wire::new(Box::new(r), Box::new(Sink(out.clone())));
        assert_eq!(wire.request("initialize", json!({"a": 1})).unwrap(), json!(1));
        assert_eq!(wire.request("session/new", json!({})).unwrap(), json!(2));
        wire.replier().respond(&json!(9), json!({"ok": 1})).unwrap();
        let text = String::from_utf8(out.lock().unwrap().clone()).unwrap();
        let lines: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["method"], "initialize");
        assert_eq!(lines[0]["jsonrpc"], "2.0");
        assert_eq!(lines[2]["result"]["ok"], 1);
    }
}
