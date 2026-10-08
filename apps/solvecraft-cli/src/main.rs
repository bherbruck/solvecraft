//! SolveCraft headless command line.
//!
//! ```text
//! solvecraft-cli run <script.json|design.solvecraft|part.step|mesh.3mf> [--in design|part.step|mesh.3mf] [--out FILE]... [--save FILE] [--quiet]
//! solvecraft-cli eval <script.json|design.solvecraft|part.step> (alias: inspect) measurements as JSON
//! solvecraft-cli snapshot <script|design> --out shot.png [--width W] [--height H] [--view iso|front|top|…]
//! solvecraft-cli exec <command> [json-params]                 run one command on an empty design
//! solvecraft-cli commands                                     the command registry as JSON
//! solvecraft-cli recipe <recipe.json>                         translate an oracle recipe to a script
//! solvecraft-cli oracle <case-dir>...                         replay recipes and compare with measure.json
//! solvecraft-cli step-corpus <case-dir>...                    import part.step and compare with measure.json
//! solvecraft-cli mcp [--in design|script] [--connect HOST:PORT] MCP server on stdio (docs/mcp.md)
//! ```
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod oracle;
mod recipe;
mod seams;

use std::process::ExitCode;

use serde_json::{Value, json};
use solvecraft_engine::Session;
use solvecraft_engine::render::{StandardView, render_png};

const USAGE: &str = "usage:
  solvecraft-cli run <script.json|design.solvecraft|part.step> [--in design.solvecraft|part.step] [--out FILE]... [--save FILE] [--quiet] [--results]
  solvecraft-cli run --in <design.solvecraft|part.step> [--out FILE]...
  solvecraft-cli eval <script.json|design.solvecraft|part.step>     (alias: inspect; or --in FILE)
  solvecraft-cli snapshot <script|design> --out shot.png [--width W] [--height H] [--view iso|front|back|top|bottom|left|right]
  solvecraft-cli exec <command> [json-params]
  solvecraft-cli commands
  solvecraft-cli recipe <recipe.json>
  solvecraft-cli oracle <case-dir>... [--json]
  solvecraft-cli step-corpus <case-dir>... [--json]
  solvecraft-cli mcp [--in design.solvecraft|script.json] [--connect 127.0.0.1:PORT]
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("run") => cmd_run(&args[1..]),
        Some("eval" | "inspect") => cmd_eval(&args[1..]),
        Some("snapshot") => cmd_snapshot(&args[1..]),
        Some("exec") => cmd_exec(&args[1..]),
        Some("commands") => {
            let mut s = Session::default();
            s.execute("engine.commands", &json!({})).map(|v| println!("{}", pretty(&v))).map_err(|e| e.to_string())
        }
        Some("recipe") => args.get(1).ok_or_else(|| USAGE.to_string()).and_then(|p| {
            let v = read_json(p)?;
            recipe::to_script(&v).map(|s| println!("{}", pretty(&s)))
        }),
        Some("oracle") => oracle::run(&args[1..]),
        Some("step-corpus") => oracle::step_corpus(&args[1..]),
        Some("mcp") => cmd_mcp(&args[1..]),
        // Hidden, for the kill-mid-save test: save a large design over and over.
        Some("save-stress") => cmd_save_stress(&args[1..]),
        // Hidden: time recomputes after parameter edits (docs/perf.md).
        Some("bench-edit") => cmd_bench_edit(&args[1..]),
        Some("--version" | "-V") => {
            println!("solvecraft-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err(USAGE.to_string()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

pub fn read_json(path: &str) -> Result<Value, String> {
    let s = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_str(&s).map_err(|e| format!("{path}: {e}"))
}

/// A session from a design file, a STEP/3MF/STL file, a command script, or an oracle recipe.
pub fn load(path: &str) -> Result<Session, String> {
    let mut s = Session::default();
    if solvecraft_engine::io::is_step_path(path) || solvecraft_engine::io::is_mesh_path(path) {
        s.execute("doc.open", &json!({"path": path})).map_err(|e| e.to_string())?;
        return Ok(s);
    }
    let v = read_json(path)?;
    if v.get("format").and_then(Value::as_str).is_some_and(|f| f.starts_with("solvecraft")) {
        s.execute("doc.open", &json!({"path": path})).map_err(|e| e.to_string())?;
    } else if v.get("features").is_some() && v.get("commands").is_none() {
        let script = recipe::to_script(&v)?;
        s.run_script(&script).map_err(|e| e.to_string())?;
    } else {
        s.run_script(&v).map_err(|e| e.to_string())?;
    }
    Ok(s)
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn flags<'a>(args: &'a [String], name: &str) -> Vec<&'a str> {
    args.windows(2).filter(|w| w[0] == name).map(|w| w[1].as_str()).collect()
}

/// The first argument that is not an option or an option's value.
fn positional(args: &[String]) -> Option<&String> {
    let mut i = 0;
    while let Some(a) = args.get(i) {
        if a.starts_with("--") {
            i += if matches!(a.as_str(), "--quiet" | "--results") { 1 } else { 2 };
            continue;
        }
        return Some(a);
    }
    None
}

fn cmd_run(args: &[String]) -> Result<(), String> {
    let base = flag(args, "--in");
    let path = positional(args);
    if path.is_none() && base.is_none() {
        return Err(USAGE.to_string());
    }
    // `--in` starts from a design or STEP file; the positional script then runs on top of it.
    let mut s = match base {
        Some(b) => load(b)?,
        None => Session::default(),
    };
    if args.iter().any(|a| a == "--results") {
        // Print every command's result (scripts only).
        let v = read_json(path.ok_or("--results needs a script")?)?;
        let r = s.run_script(&v).map_err(|e| e.to_string())?;
        println!("{}", pretty(&Value::Array(r)));
        return Ok(());
    }
    match (base, path) {
        (Some(_), Some(p)) => {
            let v = read_json(p)?;
            s.run_script(&v).map_err(|e| e.to_string())?;
        }
        (None, Some(p)) => s = load(p)?,
        _ => {}
    }
    for out in flags(args, "--out") {
        s.execute("ExportCommand", &json!({"path": out})).map_err(|e| e.to_string())?;
    }
    if let Some(p) = flag(args, "--save") {
        s.execute("SaveDocumentAsCommand", &json!({"path": p})).map_err(|e| e.to_string())?;
    }
    if !args.iter().any(|a| a == "--quiet") {
        let m = s.execute("MeasureCommand", &json!({})).map_err(|e| e.to_string())?;
        println!("{}", pretty(&m));
    }
    Ok(())
}

fn cmd_eval(args: &[String]) -> Result<(), String> {
    let path = flag(args, "--in").or_else(|| positional(args).map(String::as_str)).ok_or_else(|| USAGE.to_string())?;
    let mut s = load(path)?;
    let m = s.execute("MeasureCommand", &json!({})).map_err(|e| e.to_string())?;
    let d = s.execute("document.inspect", &json!({})).map_err(|e| e.to_string())?;
    let out = json!({
        "body_count": m["body_count"],
        "bodies": m["bodies"],
        "total": m["total"],
        "timeline": d["timeline"],
        "params": d["params"],
        "sketches": d["sketches"],
    });
    println!("{}", pretty(&out));
    Ok(())
}

fn cmd_snapshot(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or_else(|| USAGE.to_string())?;
    let out = flag(args, "--out").ok_or("snapshot needs --out FILE.png")?;
    let w = flag(args, "--width").and_then(|x| x.parse().ok()).unwrap_or(1280usize);
    let h = flag(args, "--height").and_then(|x| x.parse().ok()).unwrap_or(800usize);
    let s = load(path)?;
    let mut cam = solvecraft_engine::view::home_camera(&s);
    if let Some(v) = flag(args, "--view") {
        cam.set_view(StandardView::parse(v).ok_or_else(|| format!("unknown view `{v}`"))?);
    }
    let scene = solvecraft_engine::view::scene(&s, &cam);
    let png = render_png(&scene, &cam, w, h).ok_or("render failed")?;
    std::fs::write(out, &png).map_err(|e| format!("{out}: {e}"))?;
    println!("{}", pretty(&json!({"path": out, "width": w, "height": h, "bytes": png.len()})));
    Ok(())
}

fn cmd_exec(args: &[String]) -> Result<(), String> {
    let id = args.first().ok_or_else(|| USAGE.to_string())?;
    let p: Value = match args.get(1) {
        Some(t) => serde_json::from_str(t).map_err(|e| format!("params: {e}"))?,
        None => json!({}),
    };
    let mut s = Session::default();
    let v = s.execute(id, &p).map_err(|e| e.to_string())?;
    println!("{}", pretty(&v));
    Ok(())
}

/// MCP server on stdio: headless (optionally starting from a design or script) or bridged to
/// a running app's control port. stdout carries only protocol messages.
fn cmd_mcp(args: &[String]) -> Result<(), String> {
    let backend: Box<dyn solvecraft_mcp::Backend> = match (flag(args, "--connect"), flag(args, "--in")) {
        (Some(_), Some(_)) => return Err("mcp: use either --connect or --in, not both".into()),
        (Some(addr), None) => Box::new(solvecraft_mcp::Remote::connect(addr).map_err(|e| format!("cannot connect to {addr}: {e}"))?),
        (None, Some(path)) => Box::new(solvecraft_mcp::Headless::new(load(path)?)),
        (None, None) => Box::new(solvecraft_mcp::Headless::default()),
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    solvecraft_mcp::Server::new(backend).serve(stdin.lock(), stdout.lock()).map_err(|e| e.to_string())
}

/// `save-stress <path> <rounds>`: save a design of a few hundred kilobytes `rounds` times, its
/// parameter `save_round` set to the round number each time.
fn cmd_save_stress(args: &[String]) -> Result<(), String> {
    let (Some(path), Some(rounds)) = (args.first(), args.get(1).and_then(|n| n.parse::<u64>().ok())) else {
        return Err("usage: solvecraft-cli save-stress <path> <rounds>".into());
    };
    let mut s = Session::default();
    let filler = (0..3000).map(|i| solvecraft_engine::doc::Param {
        name: format!("p{i}"),
        expr: format!("{i} mm + 0.5 mm"),
        unit: "mm".into(),
        comment: "filler to make the file large".into(),
        model: false,
    });
    s.doc_mut().params.extend(filler);
    for r in 0..rounds {
        s.doc_mut().set_param("save_round", &r.to_string(), None, None).map_err(|e| e.to_string())?;
        s.execute("SaveDocumentCommand", &json!({ "path": path })).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// `bench-edit <script.json|recipe.json> <param> <expr>`: build the design, then time a
/// parameter edit, setting it back, undo and redo (ms and features recomputed each).
fn cmd_bench_edit(args: &[String]) -> Result<(), String> {
    let (Some(path), Some(param), Some(expr)) = (args.first(), args.get(1), args.get(2)) else {
        return Err("usage: solvecraft-cli bench-edit <script.json|recipe.json> <param> <expr>".into());
    };
    let v = read_json(path)?;
    let script = if v.get("features").is_some() { recipe::to_script(&v)? } else { v };
    let mut s = Session::default();
    let t = std::time::Instant::now();
    s.run_script(&script).map_err(|e| e.to_string())?;
    let build = t.elapsed().as_secs_f64() * 1000.0;
    let old = s.doc.all_param_exprs().into_iter().find(|(n, _, _)| n == param).map(|(_, e, _)| e).ok_or(format!("no parameter `{param}`"))?;
    let mut out = vec![json!({"step": "build", "ms": build, "features": s.doc.features.len()})];
    let mut time = |s: &mut Session, step: &str, id: &str, p: Value| -> Result<(), String> {
        let t = std::time::Instant::now();
        let r = s.execute(id, &p).map_err(|e| format!("{step}: {e}"))?;
        let n = r.get("recomputed").cloned().unwrap_or_else(|| json!(s.model.last_recomputed));
        out.push(json!({"step": step, "ms": t.elapsed().as_secs_f64() * 1000.0, "recomputed": n}));
        Ok(())
    };
    let expr = expr.replace("{old}", &old);
    time(&mut s, "edit", "ChangeParameterCommand", json!({"name": param, "expression": expr}))?;
    time(&mut s, "set back", "ChangeParameterCommand", json!({"name": param, "expression": old}))?;
    time(&mut s, "undo", "UndoCommand", json!({}))?;
    time(&mut s, "redo", "RedoCommand", json!({}))?;
    println!("{}", pretty(&json!(out)));
    Ok(())
}
