//! Where MCP tool calls end up: a control-channel method call (`docs/control-protocol.md`).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::{Value, json};
use solvecraft_engine::Session;
use solvecraft_engine::render::{StandardView, render_png};

/// Something that answers control-channel methods: `engine.execute`, `engine.commands`,
/// `document.inspect`, `ui.view`, `ui.render`, `ui.screenshot`.
pub trait Backend {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String>;
    /// True when a real window is attached (`ui.screenshot` can capture it).
    fn has_ui(&self) -> bool;
    fn describe(&self) -> String;
}

/// Longest reply line accepted from the app.
const MAX_REPLY: u64 = 64 << 20;

/// A running SolveCraft app reached through its loopback control port
/// (`solvecraft --control PORT`).
pub struct Remote {
    addr: String,
    conn: Option<(BufReader<TcpStream>, TcpStream)>,
    next_id: u64,
}

impl Remote {
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let mut r = Self { addr: addr.to_string(), conn: None, next_id: 1 };
        r.reconnect()?;
        Ok(r)
    }

    fn reconnect(&mut self) -> std::io::Result<()> {
        self.conn = None;
        let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("cannot resolve {}", self.addr));
        for sa in self.addr.to_socket_addrs()? {
            match TcpStream::connect_timeout(&sa, Duration::from_millis(800)) {
                Ok(s) => {
                    s.set_nodelay(true).ok();
                    // Kernel operations (booleans, fillets) on big parts can take a while.
                    s.set_read_timeout(Some(Duration::from_secs(300))).ok();
                    let read = s.try_clone()?;
                    self.conn = Some((BufReader::new(read), s));
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn roundtrip(&mut self, line: &str) -> std::io::Result<String> {
        if self.conn.is_none() {
            self.reconnect()?;
        }
        let Some((reader, writer)) = self.conn.as_mut() else { return Err(std::io::Error::other("not connected")) };
        let sent = writer.write_all(line.as_bytes()).and_then(|()| writer.write_all(b"\n")).and_then(|()| writer.flush());
        if let Err(e) = sent {
            self.conn = None;
            return Err(e);
        }
        let mut reply = String::new();
        let n = match reader.by_ref().take(MAX_REPLY).read_line(&mut reply) {
            Ok(n) => n,
            Err(e) => {
                // A timed-out read leaves a late reply on the stream: start afresh next time.
                self.conn = None;
                return Err(e);
            }
        };
        if n == 0 || !reply.ends_with('\n') {
            self.conn = None;
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "connection closed"));
        }
        Ok(reply)
    }
}

impl Backend for Remote {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"id": id, "method": method, "params": params}).to_string();
        let reply = match self.roundtrip(&line) {
            Ok(r) => r,
            // One retry on a fresh connection (the app may have restarted).
            Err(_) => self.roundtrip(&line).map_err(|e| format!("SolveCraft at {}: {e}", self.addr))?,
        };
        let v: Value = serde_json::from_str(&reply).map_err(|e| format!("bad reply from the app: {e}"))?;
        if v.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(v.get("result").cloned().unwrap_or(Value::Null))
        } else {
            Err(v.get("error").and_then(Value::as_str).unwrap_or("error").to_string())
        }
    }
    fn has_ui(&self) -> bool {
        true
    }
    fn describe(&self) -> String {
        format!("connected to the SolveCraft app at {} (the user watches the viewport)", self.addr)
    }
}

/// An in-process headless session (no window); renders with the CPU rasterizer.
pub struct Headless {
    pub session: Session,
    /// Orientation for renders; the camera is refitted to the model every time.
    pub view: StandardView,
}

impl Default for Headless {
    fn default() -> Self {
        Headless::new(Session::default())
    }
}

impl Headless {
    pub fn new(session: Session) -> Self {
        Headless { session, view: StandardView::Iso }
    }
}

fn dim(p: &Value, k: &str, default: f64) -> usize {
    p.get(k).and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(default).clamp(16.0, 4096.0) as usize
}

impl Backend for Headless {
    fn call(&mut self, method: &str, p: Value) -> Result<Value, String> {
        let s = &mut self.session;
        let str_p = |k: &str| p.get(k).and_then(Value::as_str);
        match method {
            "engine.execute" | "command" => {
                let id = str_p("command").ok_or("missing `command`")?;
                let params = p.get("params").cloned().filter(|v| !v.is_null()).unwrap_or(json!({}));
                s.execute(id, &params).map_err(|e| e.to_string())
            }
            "engine.script" => s.run_script(&p).map(Value::from).map_err(|e| e.to_string()),
            "engine.commands" => s.execute("engine.commands", &json!({})).map_err(|e| e.to_string()),
            "document.inspect" => s.execute("document.inspect", &p).map_err(|e| e.to_string()),
            "ui.view" => {
                let v = str_p("view").unwrap_or("home");
                self.view = match v {
                    "fit" => self.view,
                    _ => StandardView::parse(v).ok_or_else(|| format!("unknown view `{v}`"))?,
                };
                Ok(json!({"view": self.view}))
            }
            "ui.render" => {
                let (w, h) = (dim(&p, "width", 1280.0), dim(&p, "height", 800.0));
                let mut cam = solvecraft_engine::view::home_camera(s);
                cam.set_view(self.view);
                let scene = solvecraft_engine::view::scene(s, &cam);
                let png = render_png(&scene, &cam, w, h).ok_or("render failed")?;
                let path = str_p("path").ok_or("missing `path`")?;
                std::fs::write(path, &png).map_err(|e| format!("{path}: {e}"))?;
                Ok(json!({"path": path, "bytes": png.len(), "width": w, "height": h}))
            }
            "ui.screenshot" => Err("no window in headless mode; use ui.render".into()),
            other => Err(format!("unknown method `{other}`")),
        }
    }
    fn has_ui(&self) -> bool {
        false
    }
    fn describe(&self) -> String {
        "headless in-process SolveCraft session".into()
    }
}
