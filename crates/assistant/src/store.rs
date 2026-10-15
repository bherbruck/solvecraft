//! Saved conversations: one JSON file per conversation, grouped by design, in the assistant's
//! data folder. They are never written into a design file, so sharing a design never shares a
//! chat.
//!
//! Files are private to the user (they hold what was said about the design). Loading is
//! lenient: a damaged or hostile file is skipped, never fatal, and sizes are capped.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::model::{Block, Message, Part, Role, Usage};

/// Conversations kept per design; the oldest are deleted.
pub const MAX_PER_DESIGN: usize = 50;

/// Largest saved conversation. A longer one loses its oldest turns when saved.
pub const MAX_FILE: usize = 2 * 1024 * 1024;

/// A conversation as saved.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    pub id: String,
    pub title: String,
    /// Seconds since the Unix epoch.
    pub updated: u64,
    pub messages: Vec<Message>,
    pub usage: Usage,
}

/// What a listing shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub updated: u64,
}

/// The folder name for a design: a hash of its path, so no file name leaks into a listing, and
/// `unsaved` for a design without a file.
pub fn key_for(design_path: Option<&str>) -> String {
    match design_path {
        None => "unsaved".into(),
        Some(p) => {
            let digest = Sha256::digest(p.as_bytes());
            digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
        }
    }
}

/// A new conversation id: the time in milliseconds, in hex, with a counter against clashes.
pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let ms = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    format!("c{ms:x}-{:x}", N.fetch_add(1, Ordering::Relaxed))
}

pub fn now() -> u64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Where conversations of `key` live.
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// The store in the assistant's data folder.
    pub fn default_location() -> Option<Store> {
        Some(Store { root: crate::settings::data_folder()?.join("conversations") })
    }

    pub fn at(root: impl Into<PathBuf>) -> Store {
        Store { root: root.into() }
    }

    fn dir(&self, key: &str) -> Option<PathBuf> {
        safe_name(key).then(|| self.root.join(key))
    }

    fn file(&self, key: &str, id: &str) -> Option<PathBuf> {
        if !safe_name(id) {
            return None;
        }
        Some(self.dir(key)?.join(format!("{id}.json")))
    }

    /// Save a conversation, dropping its oldest turns while it is over [`MAX_FILE`], then delete
    /// the oldest conversations beyond [`MAX_PER_DESIGN`].
    pub fn save(&self, key: &str, conv: &Saved) -> Result<(), String> {
        let path = self.file(key, &conv.id).ok_or("invalid conversation name")?;
        let mut conv = conv.clone();
        let text = loop {
            let t = serde_json::to_vec(&to_json(&conv)).map_err(|e| e.to_string())?;
            if t.len() <= MAX_FILE {
                break t;
            }
            // Drop the oldest turn (up to the second user text).
            let starts: Vec<usize> = conv
                .messages
                .iter()
                .enumerate()
                .filter(|(_, m)| m.role == Role::User && m.blocks.iter().any(|b| matches!(b, Block::Text(_))))
                .map(|(i, _)| i)
                .collect();
            let Some(&second) = starts.get(1) else { return Err("the conversation is too large to save".into()) };
            conv.messages.drain(..second);
        };
        solidcraft_engine::control_auth::write_private_file(&path, &text).map_err(|e| format!("cannot save the conversation: {e}"))?;
        self.prune(key);
        Ok(())
    }

    fn prune(&self, key: &str) {
        let mut all = self.list(key);
        if all.len() <= MAX_PER_DESIGN {
            return;
        }
        all.sort_by_key(|s| std::cmp::Reverse(s.updated));
        for old in all.into_iter().skip(MAX_PER_DESIGN) {
            self.delete(key, &old.id);
        }
    }

    /// The conversations of `key`, newest first.
    pub fn list(&self, key: &str) -> Vec<Summary> {
        let Some(dir) = self.dir(key) else { return Vec::new() };
        let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
        let mut out: Vec<Summary> = rd
            .flatten()
            .take(1000)
            .filter_map(|e| {
                let p = e.path();
                let id = p.file_stem()?.to_str()?.to_string();
                if p.extension()? != "json" || !safe_name(&id) {
                    return None;
                }
                let v = read_json(&p)?;
                Some(Summary {
                    id,
                    title: v.get("title").and_then(Value::as_str).unwrap_or("").chars().take(120).collect(),
                    updated: v.get("updated").and_then(Value::as_u64).unwrap_or(0),
                })
            })
            .collect();
        out.sort_by_key(|s| std::cmp::Reverse(s.updated));
        out
    }

    pub fn load(&self, key: &str, id: &str) -> Option<Saved> {
        let v = read_json(&self.file(key, id)?)?;
        from_json(id, &v)
    }

    pub fn delete(&self, key: &str, id: &str) {
        if let Some(p) = self.file(key, id) {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// A file or folder name made of letters, digits, `-` and `_`: nothing that could leave the
/// store's folder.
fn safe_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn read_json(path: &Path) -> Option<Value> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > (MAX_FILE as u64) * 2 {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn to_json(c: &Saved) -> Value {
    json!({
        "version": 1,
        "title": c.title,
        "updated": c.updated,
        "usage": {"input": c.usage.input, "output": c.usage.output, "cache_read": c.usage.cache_read, "cache_write": c.usage.cache_write},
        "messages": c.messages.iter().map(message_json).collect::<Vec<_>>(),
    })
}

fn message_json(m: &Message) -> Value {
    json!({
        "role": match m.role { Role::User => "user", Role::Assistant => "assistant" },
        "blocks": m.blocks.iter().map(block_json).collect::<Vec<_>>(),
    })
}

/// Pictures are not kept: they are large, and the conversation reads without them.
fn block_json(b: &Block) -> Value {
    match b {
        Block::Text(t) => json!({"t": "text", "text": t}),
        Block::Image { .. } => json!({"t": "text", "text": "(a picture of the view was attached)"}),
        Block::ToolUse { id, name, input, error } => json!({"t": "tool_use", "id": id, "name": name, "input": input, "error": error}),
        Block::ToolResult { id, parts, is_error } => json!({
            "t": "tool_result", "id": id, "is_error": is_error,
            "parts": parts.iter().map(|p| match p {
                Part::Text(t) => json!({"t": "text", "text": t}),
                Part::Image { .. } => json!({"t": "text", "text": "(a picture)"}),
            }).collect::<Vec<_>>(),
        }),
    }
}

fn from_json(id: &str, v: &Value) -> Option<Saved> {
    let messages: Vec<Message> = v.get("messages")?.as_array()?.iter().take(5000).filter_map(message_from).collect();
    let u = v.get("usage");
    let n = |k: &str| u.and_then(|u| u.get(k)).and_then(Value::as_u64).unwrap_or(0);
    Some(Saved {
        id: id.to_string(),
        title: v.get("title").and_then(Value::as_str).unwrap_or("").chars().take(120).collect(),
        updated: v.get("updated").and_then(Value::as_u64).unwrap_or(0),
        messages,
        usage: Usage { input: n("input"), output: n("output"), cache_read: n("cache_read"), cache_write: n("cache_write") },
    })
}

fn message_from(v: &Value) -> Option<Message> {
    let role = match v.get("role")?.as_str()? {
        "user" => Role::User,
        "assistant" => Role::Assistant,
        _ => return None,
    };
    let blocks: Vec<Block> = v.get("blocks")?.as_array()?.iter().take(512).filter_map(block_from).collect();
    Some(Message { role, blocks })
}

fn text_of(v: &Value) -> Option<String> {
    v.get("text").and_then(Value::as_str).map(str::to_string)
}

fn block_from(v: &Value) -> Option<Block> {
    match v.get("t")?.as_str()? {
        "text" => Some(Block::Text(text_of(v)?)),
        "tool_use" => Some(Block::ToolUse {
            id: v.get("id")?.as_str()?.to_string(),
            name: v.get("name")?.as_str()?.to_string(),
            input: v.get("input").filter(|i| i.is_object()).cloned().unwrap_or_else(|| Value::Object(Map::new())),
            error: v.get("error").and_then(Value::as_str).map(str::to_string),
        }),
        "tool_result" => Some(Block::ToolResult {
            id: v.get("id")?.as_str()?.to_string(),
            is_error: v.get("is_error").and_then(Value::as_bool).unwrap_or(false),
            parts: v
                .get("parts")
                .and_then(Value::as_array)
                .map(|a| a.iter().take(64).filter_map(|p| text_of(p).map(Part::Text)).collect())
                .unwrap_or_default(),
        }),
        _ => None,
    }
}

/// A conversation's title: the first words the person said.
pub fn title_of(messages: &[Message]) -> String {
    for m in messages {
        if m.role != Role::User {
            continue;
        }
        for b in &m.blocks {
            if let Block::Text(t) = b
                && !t.starts_with(crate::context::CONTEXT_OPEN)
            {
                let line = t.lines().next().unwrap_or("").trim();
                return line.chars().take(60).collect();
            }
        }
    }
    String::new()
}

/// The conversation as Markdown text, for export. Design pictures and tool bodies are left
/// out: tool calls are listed by name.
pub fn export_markdown(messages: &[Message]) -> String {
    let mut out = String::from("# SolidCraft assistant conversation\n\n");
    for m in messages {
        match m.role {
            Role::User => {
                let text: Vec<&str> = m
                    .blocks
                    .iter()
                    .filter_map(|b| if let Block::Text(t) = b { Some(t.as_str()) } else { None })
                    .filter(|t| !t.starts_with(crate::context::CONTEXT_OPEN))
                    .collect();
                if !text.is_empty() {
                    out.push_str("## You\n\n");
                    out.push_str(&text.join("\n\n"));
                    out.push_str("\n\n");
                }
            }
            Role::Assistant => {
                out.push_str("## Assistant\n\n");
                for b in &m.blocks {
                    match b {
                        Block::Text(t) => {
                            out.push_str(t);
                            out.push_str("\n\n");
                        }
                        Block::ToolUse { name, input, .. } => {
                            let title = crate::tool_loop::call_title(name, input);
                            out.push_str(&format!("> `{title}`\n\n"));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("solidcraft-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn conv(id: &str, updated: u64, n: usize) -> Saved {
        let mut messages = Vec::new();
        for i in 0..n {
            messages.push(Message::user_text(format!("question {i}")));
            messages.push(Message::assistant(vec![Block::ToolUse {
                id: format!("t{i}"),
                name: "execute".into(),
                input: json!({"command": "solid.box"}),
                error: None,
            }]));
            messages.push(Message {
                role: Role::User,
                blocks: vec![Block::ToolResult {
                    id: format!("t{i}"),
                    parts: vec![Part::Text("ok".into()), Part::Image { mime: "image/png".into(), data: "AAAA".into() }],
                    is_error: false,
                }],
            });
            messages.push(Message::assistant(vec![Block::Text(format!("answer {i}"))]));
        }
        Saved { id: id.into(), title: title_of(&messages), updated, messages, usage: Usage { input: 5, output: 6, cache_read: 1, cache_write: 0 } }
    }

    #[test]
    fn a_conversation_round_trips_without_pictures() {
        let s = Store::at(dir("roundtrip"));
        let c = conv("c1", 100, 2);
        s.save("abc", &c).unwrap();
        let back = s.load("abc", "c1").unwrap();
        assert_eq!(back.title, "question 0");
        assert_eq!(back.usage, c.usage);
        assert_eq!(back.messages.len(), c.messages.len());
        // The picture became a note, and tool pairs are intact.
        match &back.messages[2].blocks[0] {
            Block::ToolResult { parts, .. } => assert_eq!(parts[1], Part::Text("(a picture)".into())),
            other => panic!("{other:?}"),
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let p = s.file("abc", "c1").unwrap();
            assert_eq!(std::fs::metadata(p).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn listing_is_newest_first_and_skips_damaged_files() {
        let root = dir("list");
        let s = Store::at(&root);
        s.save("k", &conv("a", 10, 1)).unwrap();
        s.save("k", &conv("b", 30, 1)).unwrap();
        s.save("k", &conv("c", 20, 1)).unwrap();
        std::fs::write(root.join("k/bad.json"), "{ nope").unwrap();
        std::fs::write(root.join("k/notes.txt"), "x").unwrap();
        std::fs::write(root.join("k/we ird.json"), "{}").unwrap();
        let ids: Vec<String> = s.list("k").into_iter().map(|x| x.id).collect();
        assert_eq!(&ids[..3], ["b", "c", "a"]);
        assert!(s.load("k", "bad").is_none());
        assert!(s.list("missing").is_empty());
    }

    #[test]
    fn names_cannot_leave_the_store() {
        let s = Store::at(dir("names"));
        for bad in ["../x", "a/b", "a\\b", "", "..", ".hidden", &"a".repeat(65), "a b"] {
            assert!(s.save("k", &conv(bad, 1, 1)).is_err(), "{bad:?}");
            assert!(s.load("k", bad).is_none());
            assert!(s.list(bad).is_empty());
        }
        assert_eq!(key_for(None), "unsaved");
        assert_eq!(key_for(Some("/a/b.solidcraft")).len(), 16);
        assert_ne!(key_for(Some("/a")), key_for(Some("/b")));
        assert!(safe_name(&key_for(Some("../../etc"))));
    }

    #[test]
    fn only_the_newest_are_kept() {
        let s = Store::at(dir("prune"));
        for i in 0..(MAX_PER_DESIGN + 5) {
            s.save("k", &conv(&format!("c{i}"), i as u64, 1)).unwrap();
        }
        let ids = s.list("k");
        assert_eq!(ids.len(), MAX_PER_DESIGN);
        assert!(ids.iter().all(|x| x.updated >= 5));
    }

    #[test]
    fn a_huge_conversation_loses_its_oldest_turns() {
        let s = Store::at(dir("big"));
        let mut c = conv("big", 1, 0);
        for i in 0..40 {
            c.messages.push(Message::user_text(format!("turn {i}")));
            c.messages.push(Message::assistant(vec![Block::Text("x".repeat(100_000))]));
        }
        s.save("k", &c).unwrap();
        let back = s.load("k", "big").unwrap();
        assert!(back.messages.len() < c.messages.len());
        assert!(matches!(&back.messages[0].blocks[0], Block::Text(t) if t.starts_with("turn ")));
        assert!(matches!(&back.messages.last().unwrap().blocks[0], Block::Text(t) if t.len() == 100_000));
    }

    #[test]
    fn hostile_files_load_as_nothing_or_less() {
        let root = dir("hostile");
        let s = Store::at(&root);
        std::fs::create_dir_all(root.join("k")).unwrap();
        for (name, body) in [
            ("a", r#"{"messages": "x"}"#),
            ("b", r#"{"messages": [1, null, {"role": "robot", "blocks": []}, {"role": "user", "blocks": [{"t": "tool_use"}, {"t": "text"}, 5]}]}"#),
            ("c", r#"[]"#),
        ] {
            std::fs::write(root.join(format!("k/{name}.json")), body).unwrap();
            let _ = s.load("k", name);
        }
        let b = s.load("k", "b").unwrap();
        assert_eq!(b.messages.len(), 1);
        assert!(b.messages[0].blocks.is_empty());
    }

    #[test]
    fn export_reads_like_a_chat() {
        let c = conv("x", 1, 1);
        let md = export_markdown(&c.messages);
        assert!(md.contains("## You\n\nquestion 0"));
        assert!(md.contains("> `execute solid.box`"));
        assert!(md.contains("## Assistant\n\nanswer 0"));
        assert!(!md.contains("ok\n"), "tool output is not exported");
    }
}
