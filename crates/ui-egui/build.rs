//! Build information for Help › About: the git commit and the build date (UTC). Both fall back to
//! "unknown" when git is missing (a source tarball). Also one test per UI scenario file
//! (`tests/scenarios/*.json`), so adding a scenario needs no Rust edit.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

/// Days since 1970-01-01 to a civil date (proleptic Gregorian).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn main() {
    let commit = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some();
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (y, m, d) = civil(secs.div_euclid(86_400));
    println!("cargo:rustc-env=SOLVECRAFT_COMMIT={commit}{}", if dirty { "+" } else { "" });
    println!("cargo:rustc-env=SOLVECRAFT_BUILD_DATE={y:04}-{m:02}-{d:02}");
    // A new commit in this checkout (or worktree) restamps the build.
    if let Some(dir) = git(&["rev-parse", "--git-dir"]) {
        println!("cargo:rerun-if-changed={dir}/logs/HEAD");
    }
    println!("cargo:rerun-if-changed=build.rs");
    scenario_tests();
}

/// `$OUT_DIR/scenario_tests.rs`: a `#[test]` per scenario file, named after it; a scenario with
/// a `{"pending": "why"}` step is `#[ignore]`d with that reason (it waits for a fix).
fn scenario_tests() {
    let dir = std::path::Path::new("tests/scenarios");
    println!("cargo:rerun-if-changed=tests/scenarios");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "json")).collect())
        .unwrap_or_default();
    files.sort();
    let mut out = String::new();
    for f in files {
        let Some(stem) = f.file_stem().and_then(|s| s.to_str()) else { continue };
        let mut name: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
        if !name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
            name.insert_str(0, "s_");
        }
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        let pending = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.as_array().and_then(|a| a.iter().find_map(|s| s.get("pending").map(|p| p.as_str().unwrap_or("pending").to_string()))));
        if let Some(why) = pending {
            out += &format!("#[test]\n#[ignore = {why:?}]\nfn {name}() {{\n    run({stem:?});\n}}\n\n");
        } else {
            out += &format!("#[test]\nfn {name}() {{\n    run({stem:?});\n}}\n\n");
        }
    }
    let path = std::path::Path::new(&std::env::var("OUT_DIR").unwrap_or_else(|_| ".".into())).join("scenario_tests.rs");
    let _ = std::fs::write(path, out);
}
