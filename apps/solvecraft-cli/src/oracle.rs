//! Replay oracle recipes and compare with Fusion's measurements; import oracle STEP files and
//! compare them with the same measurements.

use serde_json::{Value, json};

use crate::{read_json, recipe};

/// Relative tolerance for volume and area (our curved-face tessellation is within ~2e-4).
pub const REL_TOL: f64 = 1e-3;

fn check_case(dir: &str) -> Value {
    let name = std::path::Path::new(dir).file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
    let rp = format!("{dir}/recipe.json");
    let mp = format!("{dir}/measure.json");
    let (recipe, want) = match (read_json(&rp), read_json(&mp)) {
        (Ok(r), Ok(m)) => (r, m),
        (Err(e), _) | (_, Err(e)) => return json!({"case": name, "pass": false, "error": e}),
    };
    let script = match recipe::to_script(&recipe) {
        Ok(s) => s,
        Err(e) => return json!({"case": name, "pass": false, "skipped": true, "error": e}),
    };
    let mut s = solvecraft_engine::Session::default();
    if let Err(e) = s.run_script(&script) {
        return json!({"case": name, "pass": false, "error": e.to_string()});
    }
    let got = match s.execute("MeasureCommand", &json!({})) {
        Ok(m) => m,
        Err(e) => return json!({"case": name, "pass": false, "error": e.to_string()}),
    };
    let mut checks = Vec::new();
    let mut pass = true;
    let mut num = |what: &str, g: Option<f64>, w: Option<f64>, rel: f64| {
        let ok = match (g, w) {
            (Some(g), Some(w)) => (g - w).abs() <= rel * w.abs().max(1.0),
            _ => false,
        };
        pass &= ok;
        checks.push(json!({"check": what, "got": g, "want": w, "ok": ok}));
    };
    num("body_count", got["body_count"].as_f64(), want["body_count"].as_f64(), 0.0);
    num("volume_mm3", got["total"]["volume_mm3"].as_f64(), want["total"]["volume_mm3"].as_f64(), REL_TOL);
    num("area_mm2", got["total"]["area_mm2"].as_f64(), want["total"]["area_mm2"].as_f64(), REL_TOL);
    num("faces", got["total"]["faces"].as_f64(), want["total"]["faces"].as_f64(), 0.0);
    num("edges", got["total"]["edges"].as_f64(), want["total"]["edges"].as_f64(), 0.0);
    num("vertices", got["total"]["vertices"].as_f64(), want["total"]["vertices"].as_f64(), 0.0);
    json!({"case": name, "pass": pass, "checks": checks})
}

/// Import a case's `part.step` (as File → Open does) and compare with Fusion's measurements:
/// body count, volume and area (relative 1e-3) and the faces read from the file.
fn check_step_case(dir: &str) -> Value {
    let name = std::path::Path::new(dir).file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
    let sp = format!("{dir}/part.step");
    let want = match read_json(&format!("{dir}/measure.json")) {
        Ok(m) => m,
        Err(e) => return json!({"case": name, "pass": false, "error": e}),
    };
    let t0 = std::time::Instant::now();
    let mut s = solvecraft_engine::Session::default();
    let opened = match s.execute("doc.open", &json!({"path": sp})) {
        Ok(v) => v,
        Err(e) => return json!({"case": name, "pass": false, "error": e.to_string()}),
    };
    let got = match s.execute("MeasureCommand", &json!({})) {
        Ok(m) => m,
        Err(e) => return json!({"case": name, "pass": false, "error": e.to_string()}),
    };
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    // Faces read from the file (the kernel may split a face that wraps around a closed surface).
    let file_faces = std::fs::read(&sp)
        .ok()
        .and_then(|b| solvecraft_engine::kernel::step_import_shared(&String::from_utf8_lossy(&b)).ok())
        .map(|imp| imp.bodies.iter().map(|b| b.file_faces).sum::<usize>() as f64);
    let mut checks = Vec::new();
    let mut pass = true;
    let mut num = |what: &str, g: Option<f64>, w: Option<f64>, rel: f64| {
        let ok = match (g, w) {
            (Some(g), Some(w)) => (g - w).abs() <= rel * w.abs().max(1.0),
            _ => false,
        };
        pass &= ok;
        checks.push(json!({"check": what, "got": g, "want": w, "ok": ok}));
    };
    num("body_count", got["body_count"].as_f64(), want["body_count"].as_f64(), 0.0);
    num("volume_mm3", got["total"]["volume_mm3"].as_f64(), want["total"]["volume_mm3"].as_f64(), REL_TOL);
    num("area_mm2", got["total"]["area_mm2"].as_f64(), want["total"]["area_mm2"].as_f64(), REL_TOL);
    num("faces", file_faces, want["total"]["faces"].as_f64(), 0.0);
    json!({
        "case": name, "pass": pass, "checks": checks, "ms": ms,
        "bodies": opened["bodies"], "warnings": opened["warnings"],
        "info": {"kernel_faces_merged": got["total"]["faces"], "edges": got["total"]["edges"], "want_edges": want["total"]["edges"], "vertices": got["total"]["vertices"], "want_vertices": want["total"]["vertices"]},
    })
}

pub fn step_corpus(args: &[String]) -> Result<(), String> {
    let dirs: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if dirs.is_empty() {
        return Err("step-corpus: give one or more case directories (each with part.step and measure.json)".into());
    }
    let results: Vec<Value> = dirs.iter().map(|d| check_step_case(d)).collect();
    report(args, &results);
    let failed = results.iter().filter(|r| !r["pass"].as_bool().unwrap_or(false)).count();
    if failed > 0 { Err(format!("{failed} STEP case(s) failed")) } else { Ok(()) }
}

fn report(args: &[String], results: &[Value]) {
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&Value::Array(results.to_vec())).unwrap_or_default());
        return;
    }
    for r in results {
        let status = if r["pass"].as_bool().unwrap_or(false) {
            "PASS"
        } else if r["skipped"].as_bool().unwrap_or(false) {
            "SKIP"
        } else {
            "FAIL"
        };
        let detail: Vec<String> = r["checks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| !c["ok"].as_bool().unwrap_or(false))
            .map(|c| format!("{} got {} want {}", c["check"].as_str().unwrap_or(""), c["got"], c["want"]))
            .collect();
        let err = r["error"].as_str().map(|e| format!(" ({e})")).unwrap_or_default();
        println!(
            "{status} {}{err}{}",
            r["case"].as_str().unwrap_or(""),
            if detail.is_empty() { String::new() } else { format!(": {}", detail.join("; ")) }
        );
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    let dirs: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if dirs.is_empty() {
        return Err("oracle: give one or more case directories".into());
    }
    let results: Vec<Value> = dirs.iter().map(|d| check_case(d)).collect();
    report(args, &results);
    let failed = results.iter().filter(|r| !r["pass"].as_bool().unwrap_or(false) && !r["skipped"].as_bool().unwrap_or(false)).count();
    if failed > 0 { Err(format!("{failed} oracle case(s) failed")) } else { Ok(()) }
}
