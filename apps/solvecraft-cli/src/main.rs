//! SolveCraft headless command line.
//!
//! ```text
//! solvecraft-cli run <script.json|design.solvecraft> [--out FILE]... [--save FILE] [--quiet]
//! solvecraft-cli eval <script.json|design.solvecraft>        (alias: inspect) measurements as JSON
//! solvecraft-cli snapshot <script|design> --out shot.png [--width W] [--height H] [--view iso|front|top|…]
//! solvecraft-cli exec <command> [json-params]                 run one command on an empty design
//! solvecraft-cli commands                                     the command registry as JSON
//! solvecraft-cli recipe <recipe.json>                         translate an oracle recipe to a script
//! solvecraft-cli oracle <case-dir>...                         replay recipes and compare with measure.json
//! ```
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod oracle;
mod recipe;

use std::process::ExitCode;

use serde_json::{Value, json};
use solvecraft_engine::Session;
use solvecraft_engine::render::{StandardView, render_png};

const USAGE: &str = "usage:
  solvecraft-cli run <script.json|design.solvecraft> [--out FILE]... [--save FILE] [--quiet] [--results]
  solvecraft-cli eval <script.json|design.solvecraft>     (alias: inspect)
  solvecraft-cli snapshot <script|design> --out shot.png [--width W] [--height H] [--view iso|front|back|top|bottom|left|right]
  solvecraft-cli exec <command> [json-params]
  solvecraft-cli commands
  solvecraft-cli recipe <recipe.json>
  solvecraft-cli oracle <case-dir>... [--json]
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

/// A session from a design file, a command script, or an oracle recipe.
pub fn load(path: &str) -> Result<Session, String> {
    let v = read_json(path)?;
    let mut s = Session::default();
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

fn cmd_run(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or_else(|| USAGE.to_string())?;
    if args.iter().any(|a| a == "--results") {
        // Print every command's result (scripts only).
        let v = read_json(path)?;
        let mut s = Session::default();
        let r = s.run_script(&v).map_err(|e| e.to_string())?;
        println!("{}", pretty(&Value::Array(r)));
        return Ok(());
    }
    let mut s = load(path)?;
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
    let path = args.first().ok_or_else(|| USAGE.to_string())?;
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
