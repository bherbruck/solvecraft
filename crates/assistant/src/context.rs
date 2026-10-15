//! What the model is told: the system prompt, a compact picture of the open design, and how the
//! history is kept inside the model's context window.

use serde_json::{Map, Value, json};

use crate::model::{Block, Message, Part, Role, ToolDef};

const MODELING: &str = include_str!("../../../skills/solidcraft-modeling/SKILL.md");
const EDITING: &str = include_str!("../../../skills/solidcraft-editing/SKILL.md");
const REFERENCE: &str = include_str!("../../../skills/solidcraft-modeling/reference.md");

/// Largest design picture attached to a message.
pub const MAX_CONTEXT: usize = 6 * 1024;

/// Marks the design picture inside a user message, so older ones can be removed again.
pub const CONTEXT_OPEN: &str = "<design_state>";
const CONTEXT_CLOSE: &str = "</design_state>";

const PREAMBLE: &str = "You are the assistant built into SolidCraft, a parametric 3D CAD application. You help the person \
who is working in it: you explain their design, find out why a feature fails, and build or change parts for them by \
calling SolidCraft's tools, which run the same commands as the toolbar. Everything you change appears in their window \
at once, and they can undo it.

Rules:
- Units are millimetres and radians internally; Z is up. Work the numbers out before you build.
- Check what you built with `measure` or `inspect_design` against values you computed, and say what you verified. \
Never claim success without checking.
- Prefer one `batch` of steps over many single calls.
- Before `new_design` or `open`, or before overwriting a file, say what you are about to do; the person is asked to \
confirm file operations.
- Each message of the person starts with the current design state in <design_state> tags. It is a summary: use \
`inspect_design` for details.
- Answer briefly. Use Markdown: short paragraphs, lists, `code` for command ids and parameter names.
- If a request is unclear or would throw away their work, ask first.

The guides below were written for agents using SolidCraft over MCP. They name the same tools you have; \
where they say \"read reference.md\", it follows, and you can skip files and scripts they mention.
";

/// The system prompt. The same bytes every time, so a provider can cache it.
pub fn system_prompt() -> String {
    format!(
        "{PREAMBLE}\n# Guide: modeling\n{}\n\n# Guide: editing\n{}\n\n# Reference\n{}\n",
        strip_front_matter(MODELING).trim(),
        strip_front_matter(EDITING).trim(),
        REFERENCE.trim(),
    )
}

/// `text` without a leading `---` … `---` block.
pub fn strip_front_matter(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("---\n") else { return text };
    match rest.find("\n---") {
        Some(i) => rest[i + 4..].trim_start_matches('\n'),
        None => text,
    }
}

/// The names of the tools in `defs`, for checking that the guides only name tools that exist.
pub fn tool_names(defs: &[ToolDef]) -> Vec<&str> {
    defs.iter().map(|d| d.name.as_str()).collect()
}

/// A compact picture of `document.inspect`: what is needed to talk about the design, with
/// everything that is at its default left out.
pub fn compact_inspect(v: &Value) -> Value {
    let mut out = Map::new();
    let get = |k: &str| v.get(k).filter(|x| !x.is_null());
    if let Some(n) = get("name") {
        out.insert("name".into(), n.clone());
    }
    if let Some(u) = get("units") {
        out.insert("units".into(), u.clone());
    }
    // The file name only: the folder is nobody else's business.
    if let Some(p) = get("path").and_then(Value::as_str) {
        out.insert("file".into(), json!(p.rsplit(['/', '\\']).next().unwrap_or(p)));
    }
    if let Some(a) = get("active_sketch") {
        out.insert("active_sketch".into(), a.clone());
    }
    let timeline_len = get("timeline").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
    if let Some(m) = get("marker").and_then(Value::as_u64).filter(|m| (*m as usize) < timeline_len) {
        out.insert("rolled_back_to".into(), json!(m));
    }
    let list = |key: &str, keep: &[&str], flags: &[&str]| -> Option<Value> {
        let items = get(key)?.as_array()?;
        Some(Value::Array(
            items
                .iter()
                .map(|it| {
                    let mut o = Map::new();
                    for k in keep {
                        if let Some(x) = it.get(*k).filter(|x| !x.is_null()) {
                            o.insert((*k).to_string(), x.clone());
                        }
                    }
                    // Flags are only mentioned when set.
                    for k in flags {
                        if let Some(x) = it.get(*k).filter(|x| !x.is_null() && **x != json!(false)) {
                            o.insert((*k).to_string(), x.clone());
                        }
                    }
                    Value::Object(o)
                })
                .collect(),
        ))
    };
    if let Some(p) = list("params", &["name", "expression", "unit", "value"], &["error"]) {
        out.insert("parameters".into(), p);
    }
    if let Some(t) = list("timeline", &["id", "name", "type"], &["suppressed", "rolled_back", "error", "warning"]) {
        out.insert("timeline".into(), t);
    }
    if let Some(b) = list("bodies", &["name", "feature", "volume", "bbox"], &[]) {
        out.insert("bodies".into(), b);
    }
    if let Some(s) = list("sketches", &["id", "name", "dof", "status"], &[]) {
        out.insert("sketches".into(), s);
    }
    if let Some(s) = get("selection").filter(|s| s.as_array().is_some_and(|a| !a.is_empty())) {
        out.insert("selection".into(), s.clone());
    }
    for (src, dst) in [("undo", "undo_steps"), ("redo", "redo_steps")] {
        if let Some(n) = get(src).and_then(Value::as_u64).filter(|n| *n > 0) {
            out.insert(dst.into(), json!(n));
        }
    }
    Value::Object(out)
}

/// The design picture as the text attached to a user message, at most [`MAX_CONTEXT`] bytes: long
/// lists keep their first and last entries.
pub fn context_block(inspect: &Value) -> String {
    let mut v = compact_inspect(inspect);
    let render = |v: &Value| format!("{CONTEXT_OPEN}\n{v}\n{CONTEXT_CLOSE}");
    for keep in [(60, 60), (30, 30), (12, 12), (5, 5), (2, 2)] {
        if render(&v).len() <= MAX_CONTEXT {
            break;
        }
        if let Some(o) = v.as_object_mut() {
            for key in ["parameters", "timeline", "bodies", "sketches"] {
                if let Some(a) = o.get_mut(key).and_then(Value::as_array_mut) {
                    shorten(a, keep.0, keep.1);
                }
            }
        }
    }
    let s = render(&v);
    if s.len() <= MAX_CONTEXT {
        return s;
    }
    // Still too large (huge names or selection): keep only the headline.
    let mut small = Map::new();
    for k in ["name", "units", "file"] {
        if let Some(x) = v.get(k) {
            small.insert(k.into(), x.clone());
        }
    }
    small.insert("note".into(), json!("too large to show; call inspect_design"));
    render(&Value::Object(small))
}

fn shorten(a: &mut Vec<Value>, head: usize, tail: usize) {
    if a.len() <= head + tail + 1 {
        return;
    }
    let omitted = a.len() - head - tail;
    let end = a.split_off(a.len() - tail);
    a.truncate(head);
    a.push(json!({"omitted": omitted}));
    a.extend(end);
}

/// A user message: the design picture, then the person's text (and picture).
pub fn user_message(text: &str, design: Option<&Value>, picture: Option<(String, String)>) -> Message {
    let mut blocks = Vec::new();
    if let Some(d) = design {
        blocks.push(Block::Text(context_block(d)));
    }
    blocks.push(Block::Text(text.to_string()));
    if let Some((mime, data)) = picture {
        blocks.push(Block::Image { mime, data });
    }
    Message { role: Role::User, blocks }
}

/// Remove the design pictures of every user message but the last, so the history does not
/// carry the design again with each turn.
pub fn strip_old_context(history: &mut [Message]) {
    let last = history.iter().rposition(|m| m.role == Role::User && has_context(m));
    for (i, m) in history.iter_mut().enumerate() {
        if Some(i) != last {
            m.blocks.retain(|b| !matches!(b, Block::Text(t) if t.starts_with(CONTEXT_OPEN)));
        }
    }
}

fn has_context(m: &Message) -> bool {
    m.blocks.iter().any(|b| matches!(b, Block::Text(t) if t.starts_with(CONTEXT_OPEN)))
}

/// A rough token count of `history` (3.5 bytes per token, which errs high for English and
/// JSON, so trimming starts early rather than late). Pictures count 1500 each.
pub fn estimate_tokens(history: &[Message]) -> u64 {
    let mut bytes = 0u64;
    let mut pictures = 0u64;
    for m in history {
        for b in &m.blocks {
            match b {
                Block::Text(t) => bytes += t.len() as u64,
                Block::Image { .. } => pictures += 1,
                Block::ToolUse { name, input, .. } => bytes += name.len() as u64 + input.to_string().len() as u64,
                Block::ToolResult { parts, .. } => {
                    for p in parts {
                        match p {
                            Part::Text(t) => bytes += t.len() as u64,
                            Part::Image { .. } => pictures += 1,
                        }
                    }
                }
            }
        }
    }
    bytes * 2 / 7 + pictures * 1500
}

/// Keep `history` plus `fixed` tokens (system prompt and tools) below 70 % of `window`: first the
/// bodies of the oldest tool results are replaced by a note, then the oldest whole turns go.
/// Tool calls and their results are never separated, and the last turn always stays.
pub fn trim_history(history: &mut Vec<Message>, window: u32, fixed: u64) -> Trimmed {
    let budget = (window as u64 * 7 / 10).saturating_sub(fixed).max(1000);
    let none = Trimmed { turns: 0, messages: 0 };
    if estimate_tokens(history) <= budget {
        return none;
    }
    // 1. Old tool output, oldest first.
    let n = history.len();
    for i in 0..n.saturating_sub(2) {
        if let Some(m) = history.get_mut(i) {
            for b in &mut m.blocks {
                if let Block::ToolResult { parts, .. } = b {
                    for p in parts.iter_mut() {
                        if matches!(p, Part::Text(t) if t.len() > 200) {
                            *p = Part::Text("[output omitted to save space]".into());
                        }
                    }
                }
            }
        }
        if estimate_tokens(history) <= budget {
            return none;
        }
    }
    // 2. Whole turns, oldest first, never the last one.
    let mut out = none;
    while estimate_tokens(history) > budget {
        let starts: Vec<usize> = history.iter().enumerate().filter(|(_, m)| is_turn_start(m)).map(|(i, _)| i).collect();
        let Some(&second) = starts.get(1) else { break };
        history.drain(..second);
        out.turns += 1;
        out.messages += second;
    }
    out
}

/// What [`trim_history`] removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trimmed {
    /// Whole turns dropped from the front.
    pub turns: usize,
    /// Messages those turns held.
    pub messages: usize,
}

/// The tokens of the system prompt and the tool schemas, which every request carries.
pub fn fixed_tokens(system: &str, tools: &[ToolDef]) -> u64 {
    let schemas: usize = tools.iter().map(|t| t.name.len() + t.description.len() + t.schema.to_string().len()).sum();
    (system.len() + schemas) as u64 * 2 / 7
}

/// A user message with the person's own words (not a bundle of tool results).
fn is_turn_start(m: &Message) -> bool {
    m.role == Role::User && m.blocks.iter().any(|b| matches!(b, Block::Text(t) if !t.starts_with(CONTEXT_OPEN)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_is_stable_and_names_only_real_tools() {
        let a = system_prompt();
        assert_eq!(a, system_prompt());
        assert!(a.contains("# Guide: modeling") && a.contains("# Reference"));
        assert!(!a.contains("\nname: solidcraft-modeling"), "front matter is stripped");
        // Every `tool_name` the guides call with backticks and a plausible tool shape must exist.
        let tools = crate::tools::definitions(true);
        let names = tool_names(&tools);
        for known in
            ["batch", "execute", "measure", "inspect_design", "screenshot", "set_parameter", "list_commands", "body_topology", "new_design", "undo"]
        {
            assert!(names.contains(&known), "{known}");
        }
        // The editing guide sends the model to crash reports; the tool is there.
        assert!(a.contains("crash_reports") && names.contains(&"crash_reports"));
        assert!(a.len() < 40_000, "{} bytes", a.len());
    }

    #[test]
    fn front_matter_is_removed() {
        assert_eq!(strip_front_matter("---\nname: x\n---\n\n# Title\nbody"), "# Title\nbody");
        assert_eq!(strip_front_matter("# No front matter"), "# No front matter");
        assert_eq!(strip_front_matter("---\nunterminated"), "---\nunterminated");
    }

    fn big_inspect(n: usize) -> Value {
        json!({
            "name": "Bracket", "units": "mm", "path": "/home/me/secret-project/bracket.solidcraft", "dirty": true, "revision": 77,
            "active_sketch": null, "marker": n, "hidden_origin": false,
            "params": (0..n).map(|i| json!({"name": format!("p{i}"), "expression": "10", "unit": "mm", "value": 10.0, "model": null, "comment": null, "error": null})).collect::<Vec<_>>(),
            "timeline": (0..n).map(|i| json!({"id": i, "name": format!("Feature {i}"), "type": "extrude", "suppressed": false, "rolled_back": false, "error": null, "warning": null, "ms": 12, "boolean_diagnostics": []})).collect::<Vec<_>>(),
            "bodies": [{"name": "Body1", "feature": 3, "visible": true}],
            "sketches": [{"id": 1, "name": "Sketch1", "dof": 0, "status": "ok", "curves": 4, "profiles": 1, "visible": true}],
            "selection": [], "undo": 3, "redo": 0,
        })
    }

    #[test]
    fn the_design_picture_is_compact_and_private() {
        let c = compact_inspect(&big_inspect(2));
        assert_eq!(c["file"], "bracket.solidcraft");
        assert!(c.get("path").is_none() && c.get("revision").is_none() && c.get("hidden_origin").is_none());
        assert!(c.get("selection").is_none() && c.get("redo_steps").is_none());
        assert_eq!(c["undo_steps"], 3);
        assert_eq!(c["timeline"][0], json!({"id": 0, "name": "Feature 0", "type": "extrude"}));
        assert!(c["timeline"][0].get("ms").is_none());
        let flagged =
            compact_inspect(&json!({"timeline": [{"id": 1, "name": "F", "type": "fillet", "error": "radius too large", "suppressed": true}]}));
        assert_eq!(flagged["timeline"][0]["error"], "radius too large");
        assert_eq!(flagged["timeline"][0]["suppressed"], true);
    }

    #[test]
    fn a_big_design_is_shortened_not_cut_mid_json() {
        let block = context_block(&big_inspect(400));
        assert!(block.len() <= MAX_CONTEXT, "{}", block.len());
        assert!(block.starts_with(CONTEXT_OPEN) && block.ends_with("</design_state>"));
        let inner = block.trim_start_matches(CONTEXT_OPEN).trim_end_matches("</design_state>");
        let v: Value = serde_json::from_str(inner.trim()).unwrap();
        assert!(v["timeline"].as_array().unwrap().iter().any(|e| e.get("omitted").is_some()));
        // Pathological names: the headline survives.
        let mut v = big_inspect(2);
        v["params"] = json!([{"name": "x".repeat(MAX_CONTEXT * 2), "expression": "1"}]);
        let b = context_block(&v);
        assert!(b.len() <= MAX_CONTEXT && b.contains("inspect_design"));
    }

    #[test]
    fn only_the_latest_design_picture_stays() {
        let d = json!({"name": "A"});
        let mut h =
            vec![user_message("one", Some(&d), None), Message::assistant(vec![Block::Text("ok".into())]), user_message("two", Some(&d), None)];
        strip_old_context(&mut h);
        assert_eq!(h[0].blocks.len(), 1);
        assert_eq!(h[2].blocks.len(), 2);
    }

    fn turn(i: usize, size: usize) -> Vec<Message> {
        vec![
            Message::user_text(format!("question {i}")),
            Message::assistant(vec![Block::ToolUse { id: format!("t{i}"), name: "measure".into(), input: json!({}), error: None }]),
            Message {
                role: Role::User,
                blocks: vec![Block::ToolResult { id: format!("t{i}"), parts: vec![Part::Text("r".repeat(size))], is_error: false }],
            },
            Message::assistant(vec![Block::Text(format!("answer {i}"))]),
        ]
    }

    fn ids(h: &[Message]) -> (Vec<String>, Vec<String>) {
        let uses =
            h.iter().flat_map(|m| m.blocks.iter()).filter_map(|b| if let Block::ToolUse { id, .. } = b { Some(id.clone()) } else { None }).collect();
        let results = h
            .iter()
            .flat_map(|m| m.blocks.iter())
            .filter_map(|b| if let Block::ToolResult { id, .. } = b { Some(id.clone()) } else { None })
            .collect();
        (uses, results)
    }

    #[test]
    fn old_tool_output_goes_first() {
        let mut h: Vec<Message> = (0..6).flat_map(|i| turn(i, 20_000)).collect();
        assert!(estimate_tokens(&h) > 30_000);
        assert_eq!(trim_history(&mut h, 20_000, 3_000).turns, 0, "no turn had to go");
        assert!(estimate_tokens(&h) <= 20_000 * 7 / 10);
        assert_eq!(h.len(), 24);
        assert!(matches!(&h[2].blocks[0], Block::ToolResult { parts, .. } if matches!(&parts[0], Part::Text(t) if t.starts_with("[output omitted"))));
        let (u, r) = ids(&h);
        assert_eq!(u, r);
    }

    #[test]
    fn then_whole_turns_go_and_pairs_stay_together() {
        // Long assistant text cannot be shortened, so whole turns must go.
        let mut h: Vec<Message> = (0..8)
            .flat_map(|i| {
                let mut t = turn(i, 300);
                t[3] = Message::assistant(vec![Block::Text(format!("answer {i} {}", "w".repeat(9_000)))]);
                t
            })
            .collect();
        let before = h.len();
        let dropped = trim_history(&mut h, 20_000, 3_000);
        assert!(dropped.turns > 0 && dropped.messages == before - h.len());
        assert!(estimate_tokens(&h) <= 20_000 * 7 / 10, "{}", estimate_tokens(&h));
        assert!(is_turn_start(&h[0]));
        let (u, r) = ids(&h);
        assert_eq!(u, r);
        assert!(h.last().unwrap().text().starts_with("answer 7"));
    }

    #[test]
    fn a_short_history_is_left_alone() {
        let mut h = turn(0, 100);
        let copy = h.clone();
        assert_eq!(trim_history(&mut h, 200_000, 3_000).turns, 0);
        assert_eq!(h, copy);
    }

    #[test]
    fn one_huge_turn_cannot_loop_forever() {
        let mut h = turn(0, 5_000_000);
        trim_history(&mut h, 8_000, 1_000);
        assert_eq!(h.len(), 4);
    }
}
