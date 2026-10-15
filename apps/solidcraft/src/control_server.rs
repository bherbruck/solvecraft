//! Loopback JSON-lines control server: one request per line, one reply per line.
//! Agents and tests drive the app through it (adapted from CADCraft, MIT OR Apache-2.0).

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use serde_json::{Value, json};
use solidcraft_engine::MAX_LINE;
use solidcraft_engine::control_auth;
use solidcraft_ui_egui::ControlRequest;

/// Connections served at once, so a flood must not be able to spawn threads without end.
const MAX_CONNECTIONS: usize = 16;
/// Lines that are not JSON before the connection is closed, so garbage cannot loop forever.
const MAX_BAD_LINES: usize = 8;

static OPEN: AtomicUsize = AtomicUsize::new(0);
/// The token file this launch wrote, removed when the app ends.
static TOKEN_FILE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// A new random token: 32 bytes from the operating system, as hex.
fn new_token() -> Option<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Remove this launch's token file (at exit).
pub fn remove_token_file() {
    if let Some(p) = TOKEN_FILE.get() {
        let _ = std::fs::remove_file(p);
    }
}

/// The loopback listener the assistant's external agents connect to (through their MCP server,
/// `solidcraft-cli mcp --connect`). It is not the app's control port: a random port and token of
/// its own, no token file, and every request is tagged with the running assistant turn and sent
/// into the assistant's inlet, where the policy and the confirmations apply.
pub fn start_agent_endpoint(spec: solidcraft_ui_egui::assistant_ui::EndpointSpec) -> Result<solidcraft_ui_egui::assistant_ui::Endpoint, String> {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    let token = new_token().ok_or("no random token available")?;
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("cannot listen on the loopback address: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let shared_token = Arc::new(token.clone());
    let flag = stop.clone();
    let open = Arc::new(AtomicUsize::new(0));
    std::thread::Builder::new()
        .name("assistant-endpoint".into())
        .spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        if open.fetch_add(1, Ordering::Relaxed) >= MAX_CONNECTIONS {
                            open.fetch_sub(1, Ordering::Relaxed);
                            continue;
                        }
                        let (tx, ctx, token, turn, open, flag) =
                            (spec.inlet.clone(), spec.ctx.clone(), shared_token.clone(), spec.turn.clone(), open.clone(), flag.clone());
                        std::thread::spawn(move || {
                            serve(stream, &token, tx, ctx, Some((&turn, &flag)));
                            open.fetch_sub(1, Ordering::Relaxed);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
                    Err(_) => std::thread::sleep(Duration::from_millis(500)),
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(solidcraft_ui_egui::assistant_ui::Endpoint::new(port, token, move || stop.store(true, Ordering::Relaxed)))
}

pub fn start(port: u16, ctx: egui::Context) -> Receiver<ControlRequest> {
    let (tx, rx) = channel::<ControlRequest>();
    // Every connection must show the token before anything runs (docs/control-protocol.md).
    let Some(token) = control_auth::env_token().or_else(new_token) else {
        eprintln!("solidcraft: control server not started: no random token available");
        return rx;
    };
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("solidcraft: control server failed to bind 127.0.0.1:{port}: {e}");
            return rx;
        }
    };
    match control_auth::token_path(port) {
        Some(path) => match control_auth::write_token_file(&path, &token) {
            Ok(()) => {
                eprintln!("solidcraft: control server listening on 127.0.0.1:{port} (token in {})", path.display());
                let _ = TOKEN_FILE.set(path);
            }
            Err(e) => eprintln!(
                "solidcraft: control server listening on 127.0.0.1:{port}, but its token file {} could not be written ({e}); \
                 clients need SOLIDCRAFT_CONTROL_TOKEN",
                path.display()
            ),
        },
        None => {
            eprintln!("solidcraft: control server listening on 127.0.0.1:{port}; no data folder for its token, clients need SOLIDCRAFT_CONTROL_TOKEN")
        }
    }
    let token = std::sync::Arc::new(token);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if OPEN.fetch_add(1, Ordering::Relaxed) >= MAX_CONNECTIONS {
                OPEN.fetch_sub(1, Ordering::Relaxed);
                let mut refused = stream;
                let _ = writeln!(refused, "{}", json!({"ok": false, "error": "too many connections"}));
                continue;
            }
            let tx = tx.clone();
            let ctx = ctx.clone();
            let token = token.clone();
            std::thread::spawn(move || {
                serve(stream, &token, tx, ctx, None);
                OPEN.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });
    rx
}

/// One line without its newline, or `None` at the end of the stream. A client that sends
/// gigabytes with no newline would otherwise make this thread allocate until the process is
/// killed, so the line is refused as soon as it passes `max`.
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
        if done {
            return Ok(Some(line));
        }
        if line.len() > max {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "request too large"));
        }
    }
}

/// The reply to a connection that didn't show the token; the connection is then closed.
fn unauthorized() -> Value {
    json!({"ok": false, "error": "unauthorized: send the control token as \"token\" (see docs/control-protocol.md)"})
}

/// Refuse the connection: the reply, then the end of what we send, then what the client had
/// already sent read and dropped (a little, briefly). Closing a socket with unread input resets
/// the connection, and a reset can reach the client before it has read the refusal.
fn refuse(out: &mut TcpStream, reader: &mut BufReader<TcpStream>) {
    use std::io::Read;
    let _ = writeln!(out, "{}", unauthorized());
    let _ = out.shutdown(std::net::Shutdown::Write);
    let _ = reader.get_ref().set_read_timeout(Some(Duration::from_millis(500)));
    let _ = std::io::copy(&mut reader.by_ref().take(64 * 1024), &mut std::io::sink());
}

/// Serve one connection. `agent`: the turn counter requests are tagged with and the flag that
/// ends the endpoint (the assistant's agent endpoint); `None` for the app's own control port.
fn serve(
    stream: TcpStream,
    token: &str,
    tx: Sender<ControlRequest>,
    ctx: egui::Context,
    agent: Option<(&std::sync::atomic::AtomicU64, &std::sync::atomic::AtomicBool)>,
) {
    let Ok(read) = stream.try_clone() else { return };
    let mut out = stream;
    let mut reader = BufReader::new(read);
    let mut bad = 0usize;
    let mut authenticated = false;
    loop {
        let line = match read_line(&mut reader, MAX_LINE) {
            Ok(Some(l)) => String::from_utf8_lossy(&l).into_owned(),
            Ok(None) => break,
            Err(e) => {
                let _ = writeln!(out, "{}", json!({"ok": false, "error": e.to_string()}));
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => {
                // The first request must carry the token; later ones on the same connection may.
                if !authenticated {
                    match msg.get("token").and_then(Value::as_str) {
                        Some(t) if control_auth::tokens_match(t, token) => authenticated = true,
                        _ => {
                            refuse(&mut out, &mut reader);
                            break;
                        }
                    }
                }
                let id = msg.get("id").cloned().unwrap_or(Value::Null);
                let method = msg.get("method").and_then(Value::as_str).unwrap_or("").to_string();
                let mut params = msg.get("params").cloned().unwrap_or(json!({}));
                if let Some((turn, stopped)) = agent {
                    if stopped.load(Ordering::Relaxed) {
                        break;
                    }
                    // The turn the request belongs to: the gate refuses it when none is running.
                    if let Some(o) = params.as_object_mut() {
                        o.insert("_turn".into(), json!(turn.load(Ordering::SeqCst)));
                    }
                }
                // A batch replaces many short requests: the whole of it may take long (#743). An
                // agent's request may also wait for the person to confirm it.
                let wait = Duration::from_secs(if method == "engine.batch" || agent.is_some() { 3600 } else { 60 });
                let (req, rrx) = ControlRequest::new(method, params);
                if tx.send(req).is_err() {
                    break;
                }
                ctx.request_repaint();
                let mut r = rrx.recv_timeout(wait).unwrap_or_else(|_| json!({"ok": false, "error": "timeout"}));
                if let Some(o) = r.as_object_mut() {
                    o.insert("id".into(), id);
                }
                r
            }
            Err(_) if !authenticated => {
                refuse(&mut out, &mut reader);
                break;
            }
            Err(e) => {
                bad += 1;
                json!({"ok": false, "error": format!("bad JSON: {e}")})
            }
        };
        if writeln!(out, "{reply}").is_err() {
            break;
        }
        if bad >= MAX_BAD_LINES {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The assistant's agent endpoint: its own random token (wrong or missing ones are refused),
    /// every accepted request tagged with the running turn, and no more connections once the
    /// endpoint is dropped.
    #[test]
    fn the_agent_endpoint_tags_requests_with_the_turn_and_stops() {
        use solidcraft_ui_egui::assistant_ui::EndpointSpec;
        use std::io::Read;
        use std::sync::Arc;
        use std::sync::atomic::AtomicU64;
        let (tx, rx) = channel::<ControlRequest>();
        let turn = Arc::new(AtomicU64::new(7));
        let ep = start_agent_endpoint(EndpointSpec { inlet: tx, turn: turn.clone(), ctx: egui::Context::default() }).unwrap();
        assert_ne!(ep.port, 0);
        assert!(ep.token.len() >= 32);
        let seen: Arc<std::sync::Mutex<Vec<Value>>> = Arc::default();
        let log = seen.clone();
        std::thread::spawn(move || {
            for req in rx {
                log.lock().unwrap().push(json!({"method": req.method, "params": req.params}));
                let _ = req.reply.send(json!({"ok": true, "result": 1}));
            }
        });
        let talk = |lines: &[Value]| {
            let mut s = TcpStream::connect(("127.0.0.1", ep.port)).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            for l in lines {
                writeln!(s, "{l}").unwrap();
            }
            let _ = s.shutdown(std::net::Shutdown::Write);
            let mut text = String::new();
            let _ = s.read_to_string(&mut text);
            text.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).collect::<Vec<_>>()
        };
        let denied = talk(&[json!({"id": 1, "method": "engine.execute", "params": {}, "token": "nope"})]);
        assert_eq!(denied[0]["ok"], false);
        assert!(seen.lock().unwrap().is_empty());
        let ok = talk(&[json!({"id": 2, "method": "engine.execute", "params": {"command": "solid.box"}, "token": ep.token})]);
        assert_eq!(ok[0]["ok"], true);
        assert_eq!(ok[0]["id"], 2);
        let got = seen.lock().unwrap().clone();
        assert_eq!(got[0]["params"]["_turn"], 7);
        assert_eq!(got[0]["params"]["command"], "solid.box");
        // The turn number is read per request.
        turn.store(9, std::sync::atomic::Ordering::SeqCst);
        talk(&[json!({"id": 3, "method": "document.inspect", "params": {}, "token": ep.token})]);
        assert_eq!(seen.lock().unwrap()[1]["params"]["_turn"], 9);
        // No token file is left for anyone to read.
        assert!(control_auth::token_path(ep.port).is_none_or(|p| !p.exists()));
        // Dropping the endpoint ends it: nothing more is served, even with the right token.
        let (port, token) = (ep.port, ep.token.clone());
        drop(ep);
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let _ = writeln!(s, "{}", json!({"id": 4, "method": "document.inspect", "params": {}, "token": token}));
            let _ = s.shutdown(std::net::Shutdown::Write);
            let mut text = String::new();
            let _ = s.read_to_string(&mut text);
            assert!(text.is_empty(), "served after the endpoint ended: {text}");
        }
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    /// #205: a connection runs nothing until a request carries the launch's token; a wrong or
    /// missing token, or garbage, is refused and the connection closed. Once shown, the token
    /// need not be repeated on that connection.
    #[test]
    fn requests_run_only_after_the_token() {
        use std::io::Read;
        let token = "0123456789abcdef0123456789abcdef";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel::<ControlRequest>();
        let ran = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = ran.clone();
        std::thread::spawn(move || {
            for req in rx {
                seen.lock().unwrap().push(req.method.clone());
                let _ = req.reply.send(json!({"ok": true, "result": null}));
            }
        });
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                std::thread::spawn(move || serve(stream, token, tx, egui::Context::default(), None));
            }
        });
        let session = |lines: &[String]| {
            let mut s = TcpStream::connect(addr).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            for l in lines {
                let _ = writeln!(s, "{l}");
            }
            let _ = s.shutdown(std::net::Shutdown::Write);
            let mut text = String::new();
            let _ = s.read_to_string(&mut text);
            text.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).collect::<Vec<_>>()
        };
        for first in [
            json!({"id": 1, "method": "app.quit", "params": {"force": true}}).to_string(),
            json!({"id": 1, "method": "app.quit", "token": "0123456789abcdef0123456789abcdee"}).to_string(),
            json!({"id": 1, "method": "app.quit", "token": "0123"}).to_string(),
            json!({"id": 1, "method": "app.quit", "token": 7}).to_string(),
            "not json".to_string(),
        ] {
            let replies = session(&[first.clone(), json!({"id": 2, "method": "app.quit", "token": token}).to_string()]);
            assert_eq!(replies.len(), 1, "{first}: closed after refusing: {replies:?}");
            assert!(replies[0]["error"].as_str().is_some_and(|e| e.starts_with("unauthorized")), "{first}: {replies:?}");
        }
        assert!(ran.lock().unwrap().is_empty(), "nothing ran without the token");
        let replies = session(&[
            json!({"id": 1, "method": "ui.inspect", "token": token}).to_string(),
            json!({"id": 2, "method": "engine.commands"}).to_string(),
        ]);
        assert_eq!(replies.len(), 2, "{replies:?}");
        assert!(replies.iter().all(|r| r["ok"] == true), "{replies:?}");
        assert_eq!(*ran.lock().unwrap(), ["ui.inspect", "engine.commands"]);
    }

    /// A request longer than the cap is refused instead of allocated.
    #[test]
    fn lines_past_the_cap_are_refused() {
        let flood = vec![b'A'; MAX_LINE + 2];
        let mut r = BufReader::new(flood.as_slice());
        let e = read_line(&mut r, MAX_LINE).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
        assert!(e.to_string().contains("request too large"), "{e}");
    }

    /// Ordinary framing: lines split on newlines, blank lines kept, no trailing newline needed.
    #[test]
    fn lines_split_on_newlines() {
        let text = b"{\"a\":1}\n\n{\"b\":2}\nlast".to_vec();
        let mut r = BufReader::new(text.as_slice());
        let mut got = Vec::new();
        while let Some(line) = read_line(&mut r, MAX_LINE).unwrap() {
            got.push(String::from_utf8(line).unwrap());
        }
        assert_eq!(got, vec!["{\"a\":1}", "", "{\"b\":2}", "last"]);
        assert!(read_line(&mut r, MAX_LINE).unwrap().is_none());
    }

    /// A line that spans several reads of the underlying stream still arrives whole.
    #[test]
    fn lines_longer_than_one_buffer_arrive_whole() {
        let long = "x".repeat(200_000);
        let text = format!("{long}\n{long}\n").into_bytes();
        let mut r = BufReader::with_capacity(64, text.as_slice());
        assert_eq!(read_line(&mut r, MAX_LINE).unwrap().unwrap().len(), long.len());
        assert_eq!(read_line(&mut r, MAX_LINE).unwrap().unwrap().len(), long.len());
        assert!(read_line(&mut r, MAX_LINE).unwrap().is_none());
        // The same line against a smaller cap is refused.
        let mut r = BufReader::with_capacity(64, text.as_slice());
        assert!(read_line(&mut r, 1000).is_err());
    }
}
