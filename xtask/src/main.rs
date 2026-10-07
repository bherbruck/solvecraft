//! Workspace tooling: `cargo xtask <command>`.
//!
//! Pure Rust (std + serde_json). External tools (`cargo`) are invoked through
//! `std::process::Command`.

mod assets;
mod layers;
mod parity;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  layers          enforce the crate dependency layering (plan/architecture.md §3)
  assets          check that every non-code asset file is attributed in ATTRIBUTION.md
  parity [--refresh]
                  recompute Fusion command parity in docs/parity.md (--refresh rebuilds
                  xtask/data/fusion-catalog.tsv from plan/fusion/menu-tree.json)
  oracle          replay plan/fusion/oracle/*/recipe.json and compare with measure.json;
                  writes docs/oracle.md
  step-corpus [DIR]
                  import every DIR/*/part.step (default plan/fusion/oracle) and compare
                  with measure.json; writes docs/step-import.md
  wasm            cargo check every library crate and the web app for wasm32-unknown-unknown
  ci              fmt --check, clippy -D warnings, test, assets, layers, wasm (stops at first failure)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let result = match args.first().map(String::as_str) {
        Some("layers") => cmd_layers(),
        Some("assets") => assets::run(&root()),
        Some("parity") => parity::run(&root(), rest.contains(&"--refresh")),
        Some("oracle") => cmd_oracle(),
        Some("step-corpus") => cmd_step_corpus(rest.first().copied()),
        Some("ci") => cmd_ci(),
        Some("wasm") => cmd_wasm(),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Workspace root (parent of the xtask crate).
pub fn root() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent().unwrap_or(here).to_path_buf()
}

pub fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("$ {what}");
    let status = cmd.status().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what}: exited with {status}")) }
}

pub fn metadata() -> Result<serde_json::Value, String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))
}

fn cmd_layers() -> Result<(), String> {
    let crates = layers::from_metadata(&metadata()?)?;
    println!("Dependency layering (plan/architecture.md §3)\n");
    println!("{:<28} {:<14} workspace deps", "crate", "layer");
    for c in &crates {
        let ws: Vec<String> = c
            .deps
            .iter()
            .filter(|d| d.workspace)
            .map(|d| {
                let k = match d.kind {
                    layers::DepKind::Normal => "",
                    layers::DepKind::Dev => " (dev)",
                    layers::DepKind::Build => " (build)",
                };
                format!("{}{k}", layers::short_name(&d.name))
            })
            .collect();
        println!("{:<28} {:<14} {}", c.name, layers::describe(layers::classify(&c.name)), ws.join(", "));
    }
    let violations = layers::check(&crates);
    println!();
    if violations.is_empty() {
        println!("OK: {} crates, no layering violations.", crates.len());
        Ok(())
    } else {
        println!("{} violation(s):", violations.len());
        for v in &violations {
            println!("  - {v}");
        }
        Err(format!("{} layering violation(s)", violations.len()))
    }
}

/// Run the CLI's oracle replay over every case directory and write docs/oracle.md.
fn cmd_oracle() -> Result<(), String> {
    let dir = root().join("plan/fusion/oracle");
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e} (the oracle data is local, in plan/fusion/oracle)", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("recipe.json").is_file() && p.join("measure.json").is_file())
        .collect();
    cases.sort();
    if cases.is_empty() {
        return Err("no oracle cases with recipe.json and measure.json".into());
    }
    let mut cmd = cargo();
    cmd.args(["run", "-q", "--release", "-p", "solvecraft-cli", "--", "oracle", "--json"]);
    for c in &cases {
        cmd.arg(c);
    }
    let out = cmd.output().map_err(|e| format!("solvecraft-cli: {e}"))?;
    let results: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("oracle JSON: {e}\n{}", String::from_utf8_lossy(&out.stderr)))?;
    let list = results.as_array().cloned().unwrap_or_default();
    let pass = list.iter().filter(|r| r["pass"].as_bool().unwrap_or(false)).count();
    let skipped = list.iter().filter(|r| r["skipped"].as_bool().unwrap_or(false)).count();
    let mut md = format!(
        "# Fusion oracle\n\nGenerated by `cargo xtask oracle`. Each case is a part built in Fusion (recipe and measurements kept locally in `plan/fusion/oracle/`, not committed). SolveCraft replays the recipe through its command engine and compares body count, volume and area (relative tolerance 1e-3) and face/edge/vertex counts (exact).\n\n**{pass} / {} cases pass** ({skipped} need features not implemented yet).\n\n| Case | Result | Notes |\n|---|---|---|\n",
        list.len()
    );
    for r in &list {
        let status = if r["pass"].as_bool().unwrap_or(false) {
            "pass"
        } else if r["skipped"].as_bool().unwrap_or(false) {
            "not yet"
        } else {
            "FAIL"
        };
        let mut notes: Vec<String> = r["checks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| !c["ok"].as_bool().unwrap_or(false))
            .map(|c| format!("{}: got {} want {}", c["check"].as_str().unwrap_or(""), c["got"], c["want"]))
            .collect();
        if let Some(e) = r["error"].as_str() {
            notes.push(e.replace('|', "/"));
        }
        md += &format!("| {} | {status} | {} |\n", r["case"].as_str().unwrap_or(""), notes.join("; "));
    }
    std::fs::write(root().join("docs/oracle.md"), md).map_err(|e| format!("docs/oracle.md: {e}"))?;
    println!("oracle: {pass}/{} cases pass ({skipped} not yet supported) → docs/oracle.md", list.len());
    Ok(())
}

/// Never break wasm: every layered crate (everything below the apps) and the web app must
/// build for the browser.
fn cmd_wasm() -> Result<(), String> {
    let crates = layers::from_metadata(&metadata()?)?;
    let mut set: Vec<String> =
        crates.iter().filter(|c| matches!(layers::classify(&c.name), Some(layers::Class::Layer(_)))).map(|c| c.name.clone()).collect();
    set.push("solvecraft-web".into());
    let mut c = cargo();
    c.args(["check", "--target", "wasm32-unknown-unknown"]);
    for p in &set {
        c.args(["-p", p]);
    }
    run(c, &format!("cargo check --target wasm32-unknown-unknown ({} crates)", set.len()))?;
    println!("wasm: {} crates build for wasm32-unknown-unknown", set.len());
    Ok(())
}

/// Import every oracle STEP file (exported by Fusion) and compare with Fusion's measurements;
/// write docs/step-import.md.
fn cmd_step_corpus(dir: Option<&str>) -> Result<(), String> {
    let dir = dir.map(PathBuf::from).unwrap_or_else(|| root().join("plan/fusion/oracle"));
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e} (the oracle data is local, in plan/fusion/oracle)", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("part.step").is_file() && p.join("measure.json").is_file())
        .collect();
    cases.sort();
    if cases.is_empty() {
        return Err(format!("no cases with part.step and measure.json in {}", dir.display()));
    }
    let mut cmd = cargo();
    cmd.args(["run", "-q", "--release", "-p", "solvecraft-cli", "--", "step-corpus", "--json"]);
    for c in &cases {
        cmd.arg(c);
    }
    let out = cmd.output().map_err(|e| format!("solvecraft-cli: {e}"))?;
    let results: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("step-corpus JSON: {e}\n{}", String::from_utf8_lossy(&out.stderr)))?;
    let list = results.as_array().cloned().unwrap_or_default();
    let pass = list.iter().filter(|r| r["pass"].as_bool().unwrap_or(false)).count();
    let check =
        |r: &serde_json::Value, name: &str| r["checks"].as_array().and_then(|c| c.iter().find(|c| c["check"] == name).cloned()).unwrap_or_default();
    let num = |v: &serde_json::Value| v.as_f64().map(|x| format!("{x:.3}")).unwrap_or_else(|| "–".into());
    let mut md = format!(
        "# STEP import\n\n\
SolveCraft reads STEP (ISO 10303-21; AP203, AP214 and AP242 geometry) with its own reader in `crates/kernel/src/step_in` \
(written from the public standards; no third-party STEP code). A STEP file becomes an **Import** base feature on the \
timeline: it holds the file's B-rep (the STEP text is kept in the design, so the design reopens without the file) and \
later features — fillets, holes, cuts sketched on its faces, shells — build on its bodies.\n\n\
- **Open**: File → Open (`doc.open`) accepts `.step`/`.stp` in any case and starts a new design; `solvecraft part.step`; \
`solvecraft-cli eval part.step`, `solvecraft-cli run script.json --in part.step`; the MCP `open` tool.\n\
- **Insert** into the current design: File → Insert STEP… or SOLID → INSERT (`FusionImportCommandFromToolbar {{path}}`).\n\
- **Drag and drop** a STEP file on the window: inserted when the design has features, opened otherwise.\n\n\
Read: manifold solid breps (with voids), faceted breps, shell-based surface models; planes, cylinders, cones, spheres, \
tori, B-spline and rational B-spline surfaces (with knots, Bézier, uniform, quasi-uniform), surfaces of revolution and \
linear extrusion, trimmed surfaces; lines, circles, ellipses, B-spline curves, polylines, trimmed/surface/seam curves; \
length and angle units (SI prefixes, conversion-based units such as inches); product names (body names come from the \
solid's name, else the product's); surface colours; assemblies (next-assembly-usage occurrences placed by item-defined \
transformations, and mapped items) — the product tree is kept in the feature for the components milestone. Files are \
untrusted: sizes, entity counts and nesting are capped and every kernel call is guarded. What cannot be read becomes a \
warning on the feature (a face left out gives an open body); not read yet: offset surfaces, pcurve-only edges.\n\n\
## Fusion corpus\n\nGenerated by `cargo xtask step-corpus`. Each case is a part exported from Fusion as STEP (AP214; kept locally in `plan/fusion/oracle/`, never committed). SolveCraft opens `part.step` exactly as File → Open does (an Import base feature) and compares with Fusion's own measurements: body count, volume and area (relative tolerance 1e-3) and the number of faces read from the file.\n\n**{pass} / {} files pass.**\n\n| Case | Result | Bodies | Volume mm³ (got / Fusion) | Area mm² (got / Fusion) | Faces (got / Fusion) | ms | Notes |\n|---|---|---|---|---|---|---|---|\n",
        list.len()
    );
    for r in &list {
        let status = if r["pass"].as_bool().unwrap_or(false) { "pass" } else { "FAIL" };
        let (v, a, f) = (check(r, "volume_mm3"), check(r, "area_mm2"), check(r, "faces"));
        let bodies: Vec<String> = r["bodies"].as_array().into_iter().flatten().filter_map(|b| b.as_str().map(str::to_string)).collect();
        let mut notes: Vec<String> = r["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str().map(|w| w.replace('|', "/"))).collect();
        if let Some(e) = r["error"].as_str() {
            notes.push(e.replace('|', "/"));
        }
        md += &format!(
            "| {} | {status} | {} | {} / {} | {} / {} | {} / {} | {:.0} | {} |\n",
            r["case"].as_str().unwrap_or(""),
            bodies.join(", "),
            num(&v["got"]),
            num(&v["want"]),
            num(&a["got"]),
            num(&a["want"]),
            f["got"].as_f64().map(|x| format!("{x}")).unwrap_or_else(|| "–".into()),
            f["want"].as_f64().map(|x| format!("{x}")).unwrap_or_else(|| "–".into()),
            r["ms"].as_f64().unwrap_or(0.0),
            notes.join("; ")
        );
    }
    std::fs::write(root().join("docs/step-import.md"), md).map_err(|e| format!("docs/step-import.md: {e}"))?;
    println!("step-corpus: {pass}/{} files pass → docs/step-import.md", list.len());
    if pass == list.len() { Ok(()) } else { Err(format!("{} STEP file(s) failed", list.len() - pass)) }
}

fn cmd_ci() -> Result<(), String> {
    let mut c = cargo();
    c.args(["fmt", "--all", "--", "--check"]);
    run(c, "cargo fmt --check")?;
    let mut c = cargo();
    c.args(["clippy", "--workspace", "--all-targets", "--release", "--", "-D", "warnings"]);
    run(c, "cargo clippy -D warnings")?;
    let mut c = cargo();
    c.args(["test", "--workspace", "--release"]);
    run(c, "cargo test")?;
    assets::run(&root())?;
    cmd_layers()?;
    cmd_wasm()?;
    eprintln!("ci: all gates passed");
    Ok(())
}
