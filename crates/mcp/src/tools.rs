//! MCP tool definitions and dispatch. Every tool is a thin wrapper over engine commands
//! (`engine.execute`) and the inspection / rendering control methods.

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Map, Value, json};

use crate::backend::Backend;

/// Most commands in one `batch` call.
const MAX_BATCH: usize = 10_000;
/// Largest image returned by `screenshot`.
const MAX_IMAGE_BYTES: u64 = 32 << 20;

const VIEWS: &[&str] = &["iso", "home", "front", "back", "top", "bottom", "left", "right", "fit"];

fn obj(props: Value, required: &[&str]) -> Value {
    let mut o = json!({"type": "object", "properties": props, "additionalProperties": false});
    if !required.is_empty() {
        o["required"] = json!(required);
    }
    o
}

fn tool(name: &str, title: &str, desc: &str, schema: Value, read_only: bool, destructive: bool) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": desc,
        "inputSchema": schema,
        "annotations": {"title": title, "readOnlyHint": read_only, "destructiveHint": destructive, "idempotentHint": read_only, "openWorldHint": false},
    })
}

fn string(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}

/// Every tool, with its JSON schema. The same list serves the headless and the bridged backend.
pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool(
            "list_commands",
            "List commands",
            "The command catalog: id, label, toolbar tab/panel, shortcut, one-line parameter docs and whether the command is \
             enabled right now. Ids are Fusion's command ids where Fusion has the command (Extrude, SketchCreate, \
             FusionFilletEdgesCommand…), dotted ids otherwise (sketch.inspect, timeline.rollback). Optional case-insensitive \
             substring `filter` over id, label, panel and params.",
            obj(json!({"filter": string("Substring to match, e.g. \"sketch\", \"fillet\", \"pattern\"")}), &[]),
            true,
            false,
        ),
        tool(
            "execute",
            "Execute a command",
            "Run one command with JSON parameters, exactly as the toolbar, palette and scripts do. Never opens a dialog. \
             Examples: SketchCreate {plane: \"XY\"}; ShapeRectangleTwoPoint {p0: [0,0], p1: [40,30]}; CircleCenterRadius \
             {center: [20,15], radius: 5}; SketchDimension {entities: [curve id], value: \"width\"}; SketchStop {}; Extrude \
             {distance: 20, operation: \"cut\"}; FusionFilletEdgesCommand {edges: [[0,0,10]], radius: 3}; PrimitiveBox {length, \
             width, height}. Lengths are mm; expressions may use parameters and units (\"width / 2\", \"30 deg\"). Returns \
             the command's result; failures leave the design unchanged.",
            obj(
                json!({
                    "command": string("Command id (see list_commands)"),
                    "params": {"type": "object", "description": "The command's parameters"},
                }),
                &["command"],
            ),
            false,
            false,
        ),
        tool(
            "batch",
            "Run several commands",
            "Run commands in order, stopping at the first error. Each step is {command, params}. On an error the steps \
             already run are undone (unless rollback: false), so the batch is all-or-nothing; the reply names the failing \
             step (index from 0), its error, and every result before it. The usual way to build a part: sketch, draw, \
             dimension, finish, extrude, fillet… in one call.",
            obj(
                json!({
                    "commands": {
                        "type": "array",
                        "description": "Steps: [{\"command\": id, \"params\": {...}}, ...]",
                        "items": {"type": "object", "properties": {"command": {"type": "string"}, "params": {"type": "object"}}, "required": ["command"]},
                    },
                    "rollback": {"type": "boolean", "description": "Undo completed steps when one fails (default true)"},
                }),
                &["commands"],
            ),
            false,
            false,
        ),
        tool(
            "inspect_design",
            "Inspect the design",
            "The whole design: user parameters (expression, value, errors), the timeline (features with type, suppressed, \
             rolled back, error, warning, timing), bodies (volume, area, centre of mass, bounding box, face/edge/vertex \
             counts), sketches (curves with ids and coordinates, points, constraints, profiles, degrees of freedom, solve \
             status), the active sketch, selection and undo depth.",
            obj(
                json!({
                    "measure": {"type": "boolean", "description": "Measure bodies (default true)"},
                    "sketches": {"type": "boolean", "description": "Full sketch contents (default true; false = summary only)"},
                }),
                &[],
            ),
            true,
            false,
        ),
        tool(
            "measure",
            "Measure bodies",
            "Volume (mm³), surface area (mm²), centre of mass, bounding box and face/edge/vertex counts of one body or all \
             bodies, with totals.",
            obj(json!({"body": string("Body name (default: all bodies)")}), &[]),
            true,
            false,
        ),
        tool(
            "body_topology",
            "List edges and faces",
            "The edges (index, midpoint, length, endpoints) and faces (index, area, centroid, plane normal) of a body. Use \
             an edge midpoint as the [x,y,z] point that picks the edge for fillet and chamfer.",
            obj(json!({"body": string("Body name, e.g. \"Body1\"")}), &["body"]),
            true,
            false,
        ),
        tool(
            "set_parameter",
            "Set a parameter",
            "Create or change a user parameter and re-evaluate the timeline. `value` is a number (mm) or an expression \
             (\"40 mm\", \"width * 2\", \"30 deg\"). Returns the new value, the features recomputed and any features that now fail.",
            obj(
                json!({
                    "name": string("Parameter name"),
                    "value": {"type": ["number", "string"], "description": "Number or expression"},
                    "unit": string("mm, deg or \"\" (unitless); default from the expression"),
                    "comment": string("Comment shown in the parameters table"),
                }),
                &["name", "value"],
            ),
            false,
            false,
        ),
        tool(
            "screenshot",
            "Look at the design",
            "Render the design and return it as a PNG image. Headless: a CPU render of the model, fitted to the view. \
             Connected to the app: a screenshot of the window (source \"window\", the default) or a render of the model \
             with the app's camera (source \"model\"). `view` first turns the camera to a standard view.",
            obj(
                json!({
                    "view": {"type": "string", "enum": VIEWS, "description": "Standard view (default: keep the current one; headless starts at iso)"},
                    "path": string("Also keep the PNG at this path"),
                    "width": {"type": "integer", "description": "Render width in pixels (model renders; default 960)"},
                    "height": {"type": "integer", "description": "Render height in pixels (model renders; default 600)"},
                    "source": {"type": "string", "enum": ["window", "model"], "description": "window = app screenshot, model = model render"},
                }),
                &[],
            ),
            true,
            false,
        ),
        tool(
            "export",
            "Export",
            "Export bodies to STEP (.step/.stp), binary STL (.stl), ASCII STL (format stla) or OBJ. The format comes from \
             the extension unless given.",
            obj(
                json!({
                    "path": string("Output file"),
                    "format": {"type": "string", "enum": ["step", "stl", "stla", "obj"]},
                    "bodies": {"type": "array", "items": {"type": "string"}, "description": "Body names (default: all)"},
                }),
                &["path"],
            ),
            false,
            false,
        ),
        tool("undo", "Undo", "Undo the last change to the design.", obj(json!({}), &[]), false, false),
        tool("redo", "Redo", "Redo the last undone change.", obj(json!({}), &[]), false, false),
        tool(
            "new_design",
            "New design",
            "Start a new, empty design (unsaved changes to the current one are discarded).",
            obj(json!({"name": string("Design name (default Untitled)")}), &[]),
            false,
            true,
        ),
        tool(
            "open",
            "Open a design",
            "Open a .solvecraft design file (replaces the current design).",
            obj(json!({"path": string("Path to a .solvecraft file")}), &["path"]),
            false,
            true,
        ),
        tool(
            "save",
            "Save the design",
            "Save the design as a .solvecraft file (to `path`, or to the file it came from).",
            obj(json!({"path": string("Path to save to (default: the current file)")}), &[]),
            false,
            false,
        ),
    ]
}

fn type_matches(t: &str, v: &Value) -> bool {
    match t {
        "string" => v.is_string(),
        "number" => v.is_number(),
        "integer" => v.is_i64() || v.is_u64(),
        "boolean" => v.is_boolean(),
        "array" => v.is_array(),
        "object" => v.is_object(),
        "null" => v.is_null(),
        _ => true,
    }
}

/// Validates `args` against the tool's schema (top level): unknown tool, missing required
/// arguments, unknown arguments, type and enum mismatches.
pub(crate) fn check_arguments(name: &str, args: &Value) -> Result<(), String> {
    let defs = tool_definitions();
    let def = defs.iter().find(|d| d["name"] == name).ok_or_else(|| format!("unknown tool `{name}`"))?;
    let schema = &def["inputSchema"];
    let Some(a) = args.as_object() else { return Err(format!("{name}: arguments must be an object")) };
    for r in schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if a.get(r).is_none_or(Value::is_null) {
            return Err(format!("{name}: missing required argument `{r}`"));
        }
    }
    let props = schema["properties"].as_object();
    for (k, v) in a {
        let Some(p) = props.and_then(|p| p.get(k)) else {
            return Err(format!("{name}: unknown argument `{k}`"));
        };
        if v.is_null() {
            continue;
        }
        let ok = match &p["type"] {
            Value::String(t) => type_matches(t, v),
            Value::Array(ts) => ts.iter().filter_map(Value::as_str).any(|t| type_matches(t, v)),
            _ => true,
        };
        if !ok {
            return Err(format!("{name}: argument `{k}` must be of type {}", p["type"]));
        }
        if let Some(allowed) = p["enum"].as_array()
            && !allowed.contains(v)
        {
            return Err(format!("{name}: `{k}` must be one of {}", Value::Array(allowed.clone())));
        }
    }
    Ok(())
}

/// A tool result: content blocks plus the error flag.
fn text_result(v: &Value, is_error: bool) -> Value {
    let text = match v {
        Value::String(s) => s.clone(),
        v => serde_json::to_string_pretty(v).unwrap_or_default(),
    };
    let mut r = json!({"content": [{"type": "text", "text": text}]});
    if is_error {
        r["isError"] = json!(true);
    }
    r
}

/// Run a tool. Failures are tool results with `isError: true` (the model sees the message);
/// nothing here panics on hostile arguments.
pub fn call_tool(b: &mut dyn Backend, name: &str, args: &Value) -> Value {
    let empty = Map::new();
    let a = args.as_object().unwrap_or(&empty);
    let r = match name {
        "screenshot" => return screenshot(b, a),
        "batch" => return batch(b, a),
        _ => dispatch(b, name, a),
    };
    match r {
        Ok(v) => text_result(&v, false),
        Err(e) => text_result(&Value::String(e), true),
    }
}

fn exec(b: &mut dyn Backend, command: &str, params: Value) -> Result<Value, String> {
    b.call("engine.execute", json!({"command": command, "params": params}))
}

fn pick(a: &Map<String, Value>, keys: &[&str]) -> Value {
    Value::Object(keys.iter().filter_map(|k| a.get(*k).filter(|v| !v.is_null()).map(|v| (k.to_string(), v.clone()))).collect())
}

fn str_arg<'a>(a: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str)
}

fn dispatch(b: &mut dyn Backend, name: &str, a: &Map<String, Value>) -> Result<Value, String> {
    match name {
        "list_commands" => {
            let all = b.call("engine.commands", json!({}))?;
            let Some(f) = str_arg(a, "filter").map(str::to_lowercase).filter(|f| !f.is_empty()) else { return Ok(all) };
            let hit = |c: &Value| {
                ["id", "label", "panel", "tab", "params"]
                    .iter()
                    .any(|k| c.get(*k).and_then(Value::as_str).is_some_and(|s| s.to_lowercase().contains(&f)))
            };
            Ok(Value::Array(all.as_array().map(|v| v.iter().filter(|c| hit(c)).cloned().collect()).unwrap_or_default()))
        }
        "execute" => {
            let id = str_arg(a, "command").ok_or("`command` is required")?;
            exec(b, id, a.get("params").cloned().filter(Value::is_object).unwrap_or(json!({})))
        }
        "inspect_design" => {
            let measure = a.get("measure").and_then(Value::as_bool).unwrap_or(true);
            let mut d = b.call("document.inspect", json!({"measure": measure}))?;
            if a.get("sketches").and_then(Value::as_bool).unwrap_or(true) {
                let ids: Vec<Value> = d["sketches"].as_array().map(|v| v.iter().map(|s| s["id"].clone()).collect()).unwrap_or_default();
                let full: Vec<Value> = ids
                    .into_iter()
                    .map(|id| exec(b, "sketch.inspect", json!({"sketch": id.clone()})).unwrap_or_else(|e| json!({"id": id, "error": e})))
                    .collect();
                d["sketches"] = Value::Array(full);
            }
            Ok(d)
        }
        "measure" => {
            let p = match str_arg(a, "body") {
                Some(body) => json!({"bodies": [body]}),
                None => json!({}),
            };
            exec(b, "MeasureCommand", p)
        }
        "body_topology" => {
            let p = pick(a, &["body"]);
            let edges = exec(b, "model.edges", p.clone())?;
            let faces = exec(b, "model.faces", p)?;
            Ok(json!({"body": a.get("body"), "edges": edges["edges"], "faces": faces["faces"]}))
        }
        "set_parameter" => {
            let mut p = pick(a, &["name", "unit", "comment"]);
            p["expression"] = a.get("value").cloned().unwrap_or(Value::Null);
            exec(b, "ChangeParameterCommand", p)
        }
        "export" => exec(b, "ExportCommand", pick(a, &["path", "format", "bodies"])),
        "undo" => exec(b, "UndoCommand", json!({})),
        "redo" => exec(b, "RedoCommand", json!({})),
        "new_design" => exec(b, "NewDocumentCommand", pick(a, &["name"])),
        "open" => exec(b, "doc.open", pick(a, &["path"])),
        "save" => exec(b, "SaveDocumentCommand", pick(a, &["path"])),
        other => Err(format!("unknown tool `{other}`")),
    }
}

fn undo_depth(b: &mut dyn Backend) -> Option<u64> {
    b.call("document.inspect", json!({})).ok().and_then(|d| d["undo"].as_u64())
}

/// `batch`: run steps in order, stop at the first failure and (by default) undo what ran.
fn batch(b: &mut dyn Backend, a: &Map<String, Value>) -> Value {
    let Some(steps) = a.get("commands").and_then(Value::as_array) else {
        return text_result(&json!("`commands` must be an array of {command, params}"), true);
    };
    if steps.len() > MAX_BATCH {
        return text_result(&json!(format!("batch too long ({} steps, at most {MAX_BATCH})", steps.len())), true);
    }
    let rollback = a.get("rollback").and_then(Value::as_bool).unwrap_or(true);
    let before = if rollback { undo_depth(b) } else { None };
    let mut results = Vec::with_capacity(steps.len());
    for (i, step) in steps.iter().enumerate() {
        let command = step.get("command").or_else(|| step.get("id")).and_then(Value::as_str);
        let r = match command {
            Some(id) => exec(b, id, step.get("params").cloned().filter(|p| !p.is_null()).unwrap_or(json!({}))),
            None => Err("missing `command`".to_string()),
        };
        match r {
            Ok(v) => results.push(v),
            Err(e) => {
                let mut undone = 0u64;
                if let (Some(before), Some(after)) = (before, rollback.then(|| undo_depth(b)).flatten()) {
                    for _ in before..after {
                        if exec(b, "UndoCommand", json!({})).is_err() {
                            break;
                        }
                        undone += 1;
                    }
                }
                return text_result(
                    &json!({
                        "ok": false,
                        "completed": i,
                        "failed": {"index": i, "command": command, "error": e},
                        "rolled_back": rollback,
                        "undone": undone,
                        "results": results,
                    }),
                    true,
                );
            }
        }
    }
    text_result(&json!({"ok": true, "completed": results.len(), "results": results}), false)
}

static SHOT: AtomicU64 = AtomicU64::new(0);

/// `screenshot`: render or capture to a file, return it as an MCP image content block.
fn screenshot(b: &mut dyn Backend, a: &Map<String, Value>) -> Value {
    match capture(b, a) {
        Ok((png, info)) => json!({"content": [
            {"type": "image", "data": base64(&png), "mimeType": "image/png"},
            {"type": "text", "text": info.to_string()},
        ]}),
        Err(e) => text_result(&Value::String(e), true),
    }
}

fn capture(b: &mut dyn Backend, a: &Map<String, Value>) -> Result<(Vec<u8>, Value), String> {
    if let Some(v) = str_arg(a, "view") {
        b.call("ui.view", json!({"view": v}))?;
        // Turning to a standard view also frames the model (like a view cube click).
        if v != "fit" && v != "home" {
            b.call("ui.view", json!({"view": "fit"}))?;
        }
    }
    let keep = str_arg(a, "path").filter(|p| !p.trim().is_empty()).map(str::to_string);
    let path = keep.clone().unwrap_or_else(|| {
        let n = SHOT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("solvecraft-mcp-{}-{n}.png", std::process::id())).to_string_lossy().to_string()
    });
    let window = match str_arg(a, "source") {
        Some("model") => false,
        Some("window") => true,
        _ => b.has_ui(),
    };
    let size = |k: &str, d: u64| a.get(k).and_then(Value::as_u64).unwrap_or(d).clamp(16, 4096);
    let render = |b: &mut dyn Backend| b.call("ui.render", json!({"path": path, "width": size("width", 960), "height": size("height", 600)}));
    let (info, source) = if window {
        match b.call("ui.screenshot", json!({"path": path})) {
            Ok(v) => (v, "window"),
            // No presented frame (minimised or hidden window): fall back to a model render.
            Err(_) => (render(b)?, "model"),
        }
    } else {
        (render(b)?, "model")
    };
    let read = std::fs::metadata(&path)
        .map_err(|e| format!("{path}: {e}"))
        .and_then(|m| if m.len() > MAX_IMAGE_BYTES { Err(format!("{path}: image too large")) } else { Ok(()) })
        .and_then(|()| std::fs::read(&path).map_err(|e| format!("{path}: {e} (is the app on another machine?)")));
    if keep.is_none() {
        let _ = std::fs::remove_file(&path);
    }
    let png = read?;
    let (w, h) = match source {
        "model" => (json!(size("width", 960)), json!(size("height", 600))),
        _ => (info.get("width").cloned().unwrap_or(Value::Null), info.get("height").cloned().unwrap_or(Value::Null)),
    };
    let bytes = png.len();
    Ok((png, json!({"source": source, "path": keep, "width": w, "height": h, "bytes": bytes})))
}

/// Standard base64 with padding (RFC 4648).
pub(crate) fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(T[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}
