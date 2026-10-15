//! What an assistant may ask the app to do, and what it may only do after the user agrees.
//!
//! The tools of the in-app assistant and the external agents that connect to it all end up as
//! control-channel requests. The app runs each one through [`classify`] on the UI thread, so one
//! rule covers every way in: raw control methods outside the small allowlist are refused (an
//! agent must not click the UI, change preferences or quit the app), and commands that read,
//! write or replace files wait for the user's click.

use serde_json::Value;

/// Control methods the assistant may use: running commands, listing them, reading the design,
/// moving the camera and rendering a picture.
pub const ALLOWED_METHODS: &[&str] = &["engine.execute", "engine.batch", "engine.commands", "document.inspect", "ui.view", "ui.render"];

/// Parameter names that carry a file name.
const PATH_KEYS: &[&str] = &["path", "file", "filename", "output", "out", "dest", "destination", "dir", "folder"];

/// Extensions that make a string value a file name even without a separator.
const FILE_EXTENSIONS: &[&str] =
    &["solidcraft", "json", "csv", "step", "stp", "stl", "3mf", "obj", "dxf", "png", "jpg", "jpeg", "urdf", "txt", "svg", "py", "toml"];

/// The decision for one request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    /// Needs the user's yes; each line says what would happen.
    Confirm(Vec<String>),
    /// Never allowed.
    Refuse(String),
}

/// Decide what to do with the control-channel request `method` with `params`.
pub fn classify(method: &str, params: &Value) -> Verdict {
    match method {
        "engine.execute" | "engine.command" => {
            let id = params.get("command").and_then(Value::as_str).unwrap_or("");
            let p = params.get("params").unwrap_or(&Value::Null);
            confirm_lines(std::iter::once((id, p)))
        }
        "engine.batch" => {
            let steps = params.get("commands").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
            confirm_lines(steps.iter().map(|s| (s.get("command").and_then(Value::as_str).unwrap_or(""), s.get("params").unwrap_or(&Value::Null))))
        }
        m if ALLOWED_METHODS.contains(&m) => Verdict::Allow,
        other => Verdict::Refuse(format!("the assistant may not use `{other}`")),
    }
}

fn confirm_lines<'a>(steps: impl Iterator<Item = (&'a str, &'a Value)>) -> Verdict {
    let lines: Vec<String> = steps.filter_map(|(id, p)| describe_if_risky(id, p)).collect();
    if lines.is_empty() { Verdict::Allow } else { Verdict::Confirm(lines) }
}

/// `Some(description)` for a command that touches files or replaces the design.
fn describe_if_risky(id: &str, params: &Value) -> Option<String> {
    let file = file_param(params);
    let by_id = id.starts_with("file.")
        || id.starts_with("doc.")
        || id.starts_with("library.export")
        || id.ends_with(".export_script")
        || id == "view.save_file";
    if !by_id && file.is_none() {
        return None;
    }
    Some(match file {
        Some(f) => format!("{id}: {f}"),
        None => id.to_string(),
    })
}

/// The first string parameter that names a file (a separator or a known extension), looking one
/// level into objects and arrays.
fn file_param(v: &Value) -> Option<String> {
    fn walk(v: &Value, depth: u8) -> Option<String> {
        match v {
            Value::Object(o) => {
                for (k, val) in o {
                    if PATH_KEYS.contains(&k.to_ascii_lowercase().as_str())
                        && let Some(s) = val.as_str()
                        && looks_like_file(s)
                    {
                        return Some(s.to_string());
                    }
                }
                if depth < 3 { o.values().find_map(|x| walk(x, depth + 1)) } else { None }
            }
            Value::Array(a) if depth < 3 => a.iter().take(64).find_map(|x| walk(x, depth + 1)),
            _ => None,
        }
    }
    walk(v, 0)
}

fn looks_like_file(s: &str) -> bool {
    if s.is_empty() || s.len() > 4096 {
        return false;
    }
    if s.contains('/') || s.contains('\\') || s.starts_with('~') || s.starts_with('.') {
        return true;
    }
    s.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && FILE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn modelling_commands_run_freely() {
        assert_eq!(classify("engine.execute", &json!({"command": "solid.extrude", "params": {"distance": 20}})), Verdict::Allow);
        assert_eq!(
            classify("engine.execute", &json!({"command": "solid.sweep", "params": {"path": ["c1", "c2"], "path_sketch": "S"}})),
            Verdict::Allow
        );
        assert_eq!(
            classify("engine.execute", &json!({"command": "construct.plane.along_path", "params": {"path": "edge-3", "t": 0.5}})),
            Verdict::Allow
        );
        assert_eq!(classify("engine.commands", &json!({})), Verdict::Allow);
        assert_eq!(classify("document.inspect", &json!({"measure": true})), Verdict::Allow);
        assert_eq!(classify("ui.render", &json!({"path": "/tmp/x.png", "width": 100})), Verdict::Allow);
    }

    #[test]
    fn file_commands_wait_for_the_user() {
        for id in ["file.new", "file.save", "file.save_as", "file.export", "doc.open", "doc.recover", "doc.recovery_discard", "file.insert_step"] {
            assert!(matches!(classify("engine.execute", &json!({"command": id, "params": {}})), Verdict::Confirm(_)), "{id}");
        }
        let v = classify("engine.execute", &json!({"command": "file.export", "params": {"path": "/home/a/out.step"}}));
        assert_eq!(v, Verdict::Confirm(vec!["file.export: /home/a/out.step".into()]));
    }

    #[test]
    fn a_file_name_anywhere_in_the_parameters_needs_a_yes() {
        for (id, p) in [
            ("sketch.export_dxf", json!({"path": "/tmp/s.dxf"})),
            ("parameters.export", json!({"path": "p.json"})),
            ("motion.export", json!({"study": "a", "path": "out.csv"})),
            ("parameters.import", json!({"path": "~/p.csv"})),
            ("canvas.insert", json!({"nested": {"file": "..\\x.png"}})),
            ("library.apply", json!({"targets": [{"output": "x.json"}]})),
        ] {
            assert!(matches!(classify("engine.execute", &json!({"command": id, "params": p})), Verdict::Confirm(_)), "{id}");
        }
    }

    #[test]
    fn a_batch_asks_once_for_all_risky_steps() {
        let v = classify(
            "engine.batch",
            &json!({"commands": [
                {"command": "solid.box", "params": {"length": 10}},
                {"command": "file.save", "params": {}},
                {"command": "file.export", "params": {"path": "a.step"}},
            ]}),
        );
        assert_eq!(v, Verdict::Confirm(vec!["file.save".into(), "file.export: a.step".into()]));
        assert_eq!(classify("engine.batch", &json!({"commands": [{"command": "solid.box"}]})), Verdict::Allow);
        assert_eq!(classify("engine.batch", &json!({})), Verdict::Allow);
    }

    #[test]
    fn everything_else_is_refused() {
        for m in ["app.quit", "ui.click", "ui.prefs", "engine.script", "ui.file", "ui.set", "ui.key", "ui.capture", "ui.assistant", "nonsense", ""] {
            assert!(matches!(classify(m, &json!({})), Verdict::Refuse(_)), "{m}");
        }
    }

    #[test]
    fn hostile_shapes_do_not_panic() {
        for p in [
            json!(null),
            json!([]),
            json!("x"),
            json!({"command": 5, "params": 7}),
            json!({"commands": "x"}),
            json!({"commands": [1, null, {"command": []}]}),
        ] {
            let _ = classify("engine.execute", &p);
            let _ = classify("engine.batch", &p);
        }
        let mut deep = json!("/x/y");
        for _ in 0..500 {
            deep = json!({"a": deep});
        }
        let _ = classify("engine.execute", &json!({"command": "x", "params": deep}));
    }

    /// Every registered file command is covered, so a new one can't slip through by name.
    #[test]
    fn every_file_and_doc_command_in_the_registry_asks() {
        let mut n = 0;
        for spec in solidcraft_engine::command_specs() {
            if spec.id.starts_with("file.") || spec.id.starts_with("doc.") {
                n += 1;
                assert!(matches!(classify("engine.execute", &json!({"command": spec.id, "params": {}})), Verdict::Confirm(_)), "{}", spec.id);
            }
        }
        assert!(n >= 8, "found {n}");
    }
}
