//! `cargo xtask licences [--check]`: THIRD-PARTY-LICENSES.txt, the licences of every crate built
//! into the shipped programs (solvecraft, solvecraft-cli, solvecraft-web), read from
//! `cargo metadata` and the crates' own licence files. The programs embed it (Help ▸ About ▸
//! Licences, `solvecraft-cli licences`), so a release download needs no licence files beside it.
//! `--check` (part of `cargo xtask ci`) fails when the committed file is out of date, e.g. after a
//! dependency changed.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use serde_json::Value;

use crate::cargo;

pub const FILE: &str = "THIRD-PARTY-LICENSES.txt";
const ROOTS: &[&str] = &["solvecraft", "solvecraft-cli", "solvecraft-web"];

pub fn run(root: &Path, check: bool) -> Result<(), String> {
    let text = generate(root)?;
    let path = root.join(FILE);
    if check {
        let old = std::fs::read_to_string(&path).unwrap_or_default();
        if old.replace("\r\n", "\n") != text {
            return Err(format!("{FILE} is out of date (dependencies changed): run `cargo xtask licences` and commit it"));
        }
        println!("licences: {FILE} is up to date");
        return Ok(());
    }
    std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("licences: wrote {FILE} ({} KB)", text.len() / 1024);
    Ok(())
}

struct Crate {
    name: String,
    version: String,
    license: String,
    repository: String,
    /// (file name, text) of the licence files shipped in the crate.
    files: Vec<(String, String)>,
}

fn generate(root: &Path) -> Result<String, String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--locked"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    let meta: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata JSON: {e}"))?;
    let packages: BTreeMap<&str, &Value> = meta["packages"].as_array().into_iter().flatten().filter_map(|p| Some((p["id"].as_str()?, p))).collect();
    let nodes: BTreeMap<&str, &Value> =
        meta["resolve"]["nodes"].as_array().into_iter().flatten().filter_map(|n| Some((n["id"].as_str()?, n))).collect();
    let members: BTreeSet<&str> = meta["workspace_members"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();

    // Everything reachable from the shipped programs through normal (not dev) dependencies, on any
    // platform.
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut queue: VecDeque<&str> = packages
        .iter()
        .filter(|(id, p)| members.contains(*id) && ROOTS.contains(&p["name"].as_str().unwrap_or_default()))
        .map(|(id, _)| *id)
        .collect();
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        for d in nodes.get(id).and_then(|n| n["deps"].as_array()).into_iter().flatten() {
            let normal = d["dep_kinds"].as_array().into_iter().flatten().any(|k| k["kind"].is_null());
            if normal && let Some(pkg) = d["pkg"].as_str() {
                queue.push_back(pkg);
            }
        }
    }

    let mut crates = Vec::new();
    for id in seen {
        let Some(p) = packages.get(id) else { continue };
        if members.contains(id) {
            continue;
        }
        let s = |k: &str| p[k].as_str().unwrap_or_default().to_string();
        let dir = Path::new(p["manifest_path"].as_str().unwrap_or_default()).parent().map(Path::to_path_buf).unwrap_or_default();
        crates.push(Crate {
            name: s("name"),
            version: s("version"),
            license: s("license"),
            repository: s("repository"),
            files: licence_files(&dir, p),
        });
    }
    crates.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    let _ = root;
    Ok(render(&crates))
}

/// The licence and notice files at the top of a crate's package (and its `license-file`).
fn licence_files(dir: &Path, p: &Value) -> Vec<(String, String)> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            let u = n.to_ascii_uppercase();
            if e.path().is_file() && ["LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE", "COPYRIGHT"].iter().any(|k| u.starts_with(k)) {
                names.insert(n);
            }
        }
    }
    if let Some(f) = p["license_file"].as_str() {
        names.insert(f.to_string());
    }
    names
        .into_iter()
        .filter_map(|n| {
            let text = std::fs::read_to_string(dir.join(&n)).ok()?;
            let text = text.replace("\r\n", "\n").trim().to_string();
            (!text.is_empty()).then(|| (Path::new(&n).file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or(n), text))
        })
        .collect()
}

fn render(crates: &[Crate]) -> String {
    let rule = "=".repeat(80);
    let mut out = format!(
        "Third-party software in SolveCraft\n\nSolveCraft (the desktop app, solvecraft-cli and the web app) is built from the {} crates below.\n\
         Generated by `cargo xtask licences` from Cargo.lock and the crates' own licence files.\n\
         SolveCraft's own licence is in LICENSE-MIT and LICENSE-APACHE, and NOTICE.\n\n",
        crates.len()
    );
    for c in crates {
        out += &format!("{} {}  ({})\n", c.name, c.version, if c.license.is_empty() { "see its licence file" } else { &c.license });
    }
    // One copy of each distinct licence text, with the crates it covers.
    let mut by_text: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut without: Vec<&Crate> = Vec::new();
    for c in crates {
        if c.files.is_empty() {
            without.push(c);
        }
        for (_, text) in &c.files {
            by_text.entry(text.as_str()).or_default().push(format!("{} {}", c.name, c.version));
        }
    }
    let mut sections: Vec<(Vec<String>, &str)> = by_text.into_iter().map(|(t, v)| (v, t)).collect();
    sections.sort();
    for (names, text) in sections {
        out += &format!("\n{rule}\n{}\n{rule}\n\n{text}\n", wrap(&names.join(", ")));
    }
    if !without.is_empty() {
        out += &format!(
            "\n{rule}\nCrates whose package carries no licence file (their licence is named in Cargo.toml; the MIT and Apache-2.0\ntexts are as in SolveCraft's own LICENSE-MIT and LICENSE-APACHE)\n{rule}\n\n"
        );
        for c in without {
            out += &format!(
                "{} {}: {}{}\n",
                c.name,
                c.version,
                c.license,
                if c.repository.is_empty() { String::new() } else { format!(" ({})", c.repository) }
            );
        }
    }
    out
}

/// Wrap a long list of crate names at 100 columns.
fn wrap(s: &str) -> String {
    let mut out = String::new();
    let mut col = 0;
    for w in s.split(' ') {
        if col > 0 && col + w.len() + 1 > 100 {
            out.push('\n');
            col = 0;
        } else if col > 0 {
            out.push(' ');
            col += 1;
        }
        out += w;
        col += w.len();
    }
    out
}
