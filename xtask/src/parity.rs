//! `cargo xtask parity`: Fusion Design-workspace command parity.
//!
//! `xtask/data/fusion-catalog.tsv` lists the toolbar commands of Fusion's Design workspace as
//! `tab \t panel \t command id \t command name` (ids and names only, nothing else). It is
//! derived from the local, uncommitted `plan/fusion/menu-tree.json` with `--refresh`. A catalog
//! entry is live when SolveCraft registers a command with the same id.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// Design workspace tabs in the catalog (menu-tree tab id, display name).
const TABS: &[(&str, &str)] = &[
    ("SolidTab", "SOLID"),
    ("SketchTab", "SKETCH"),
    ("SurfaceTab", "SURFACE"),
    ("ParaMeshOuterTab", "MESH"),
    ("FormTab", "FORM"),
    ("SheetMetalTab", "SHEET METAL"),
    ("PlasticTab", "PLASTIC"),
    ("AssemblyTab", "ASSEMBLY"),
    ("ManageTab", "MANAGE"),
    ("ToolsTab", "UTILITIES"),
];

/// The tabs the headline number counts (now: SOLID and SKETCH).
const HEADLINE: &[&str] = &["SOLID", "SKETCH"];

/// The owner's scope: these tabs (and the PLASTIC subset below) count toward the in-scope
/// figure; the rest of Fusion is deferred and listed separately.
const IN_SCOPE_TABS: &[&str] = &["SOLID", "SKETCH", "ASSEMBLY", "SHEET METAL"];
/// The PLASTIC commands in scope (enclosure features).
const PLASTIC_SUBSET: &[&str] = &["FusionBossCommand", "FusionRibCommand", "FusionWebCommand", "FusionLipCommand", "FusionSnapFitCommand"];

fn in_scope(e: &Entry) -> bool {
    IN_SCOPE_TABS.contains(&e.tab.as_str()) || (e.tab == "PLASTIC" && PLASTIC_SUBSET.contains(&e.id.as_str()))
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub tab: String,
    pub panel: String,
    pub id: String,
    pub name: String,
}

fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ").trim().to_string()
}

fn walk(controls: &[Value], defs: &[Value], tab: &str, panel: &str, out: &mut Vec<Entry>) {
    for c in controls {
        match c["type"].as_str() {
            Some("CommandControl") => {
                let id = c["command"]["id"].as_str().or(c["id"].as_str()).unwrap_or("");
                if id.is_empty() {
                    continue;
                }
                let name = c["command"]["name"]
                    .as_str()
                    .filter(|n| !n.is_empty())
                    .or_else(|| defs.iter().find(|d| d["id"].as_str() == Some(id)).and_then(|d| d["name"].as_str()))
                    .unwrap_or(id);
                if !out.iter().any(|e| e.tab == tab && e.id == id) {
                    out.push(Entry { tab: tab.into(), panel: panel.into(), id: clean(id), name: clean(name) });
                }
            }
            Some("DropDownControl") => walk(c["controls"].as_array().map(Vec::as_slice).unwrap_or(&[]), defs, tab, panel, out),
            _ => {}
        }
    }
}

/// Catalog entries from Fusion's menu-tree JSON.
pub fn from_menu_tree(v: &Value) -> Vec<Entry> {
    let defs = v["commandDefinitions"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let mut out = Vec::new();
    for (tid, tname) in TABS {
        let tab = &v["tabs"][format!("DesignProductType/{tid}")];
        for p in tab["panels"].as_array().into_iter().flatten() {
            let panel = clean(p["name"].as_str().unwrap_or("")).to_uppercase();
            walk(p["controls"].as_array().map(Vec::as_slice).unwrap_or(&[]), defs, tname, &panel, &mut out);
        }
    }
    out
}

pub fn parse_catalog(s: &str) -> Vec<Entry> {
    s.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut it = l.split('\t');
            Some(Entry { tab: it.next()?.into(), panel: it.next()?.into(), id: it.next()?.into(), name: it.next().unwrap_or("").into() })
        })
        .collect()
}

pub fn run(root: &Path, refresh: bool) -> Result<(), String> {
    let cat_path = root.join("xtask/data/fusion-catalog.tsv");
    if refresh {
        let mt = root.join("plan/fusion/menu-tree.json");
        let s = std::fs::read_to_string(&mt).map_err(|e| format!("{}: {e}", mt.display()))?;
        let v: Value = serde_json::from_str(&s).map_err(|e| format!("menu-tree.json: {e}"))?;
        let entries = from_menu_tree(&v);
        let mut out = String::from(
            "# Fusion Design workspace toolbar: tab, panel, command id, command name (ids and names only).\n# Regenerate with `cargo xtask parity --refresh` (needs the local plan/fusion/menu-tree.json).\n",
        );
        for e in &entries {
            out += &format!("{}\t{}\t{}\t{}\n", e.tab, e.panel, e.id, e.name);
        }
        std::fs::create_dir_all(root.join("xtask/data")).map_err(|e| e.to_string())?;
        std::fs::write(&cat_path, out).map_err(|e| format!("{}: {e}", cat_path.display()))?;
        println!("parity: catalog refreshed with {} entries", entries.len());
    }
    let catalog = parse_catalog(&std::fs::read_to_string(&cat_path).map_err(|e| format!("{}: {e}", cat_path.display()))?);
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root)
        .args(["run", "-q", "--release", "-p", "solvecraft-cli", "--", "commands"])
        .output()
        .map_err(|e| format!("solvecraft-cli: {e}"))?;
    let cmds: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("commands JSON: {e}"))?;
    let ids: Vec<String> = cmds.as_array().into_iter().flatten().filter_map(|c| c["id"].as_str().map(str::to_string)).collect();
    let live = |e: &Entry| ids.contains(&e.id);
    let pct = |a: usize, b: usize| if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 };

    let head: Vec<&Entry> = catalog.iter().filter(|e| HEADLINE.contains(&e.tab.as_str())).collect();
    let head_live = head.iter().filter(|e| live(e)).count();
    let all_live = catalog.iter().filter(|e| live(e)).count();
    let scope: Vec<&Entry> = catalog.iter().filter(|e| in_scope(e)).collect();
    let scope_live = scope.iter().filter(|e| live(e)).count();
    let mut md = format!(
        "# Fusion command parity\n\nGenerated by `cargo xtask parity` from `xtask/data/fusion-catalog.tsv` (the toolbar commands of Fusion's Design workspace, ids and names only) and the live command registry (`solvecraft-cli commands`). A command counts as live when SolveCraft registers the same command id. This measures breadth, not depth; see ROADMAP.md and docs/oracle.md for depth.\n\n**In scope (SOLID, SKETCH, ASSEMBLY, SHEET METAL, PLASTIC enclosure subset): {scope_live} / {} commands live ({:.0}%).** SOLID + SKETCH: {head_live} / {} ({:.0}%). Registered commands: {}.\n\nDeferred until further notice (not in the in-scope figure): MESH, FORM, SURFACE beyond what solids need, PCB, render, animation, simulation, manufacture, drawings, MANAGE, UTILITIES and the rest of PLASTIC. All Design tabs together: {all_live} / {} ({:.0}%).\n\n| Tab | Panel | Live | Total | % |\n|---|---|---:|---:|---:|\n",
        scope.len(),
        pct(scope_live, scope.len()),
        head.len(),
        pct(head_live, head.len()),
        ids.len(),
        catalog.len(),
        pct(all_live, catalog.len()),
    );
    let mut groups: Vec<(String, String, usize, usize)> = Vec::new();
    for e in &catalog {
        let l = usize::from(live(e));
        match groups.iter_mut().find(|g| g.0 == e.tab && g.1 == e.panel) {
            Some(g) => {
                g.2 += l;
                g.3 += 1;
            }
            None => groups.push((e.tab.clone(), e.panel.clone(), l, 1)),
        }
    }
    for (t, p, l, n) in groups.iter().filter(|g| IN_SCOPE_TABS.contains(&g.0.as_str())) {
        md += &format!("| {t} | {p} | {l} | {n} | {:.0}% |\n", pct(*l, *n));
    }
    let plastic: Vec<&Entry> = catalog.iter().filter(|e| e.tab == "PLASTIC" && PLASTIC_SUBSET.contains(&e.id.as_str())).collect();
    md += &format!(
        "| PLASTIC | enclosure subset | {} | {} | {:.0}% |\n",
        plastic.iter().filter(|e| live(e)).count(),
        plastic.len(),
        pct(plastic.iter().filter(|e| live(e)).count(), plastic.len())
    );
    md += "\n### Deferred tabs\n\n| Tab | Panel | Live | Total | % |\n|---|---|---:|---:|---:|\n";
    for (t, p, l, n) in groups.iter().filter(|g| !IN_SCOPE_TABS.contains(&g.0.as_str())) {
        md += &format!("| {t} | {p} | {l} | {n} | {:.0}% |\n", pct(*l, *n));
    }
    md += "\n## Live (SOLID, SKETCH)\n\n";
    for e in head.iter().filter(|e| live(e)) {
        md += &format!("- {} › {} › {} (`{}`)\n", e.tab, e.panel, e.name, e.id);
    }
    md += "\n## Not yet (SOLID, SKETCH)\n\n";
    for e in head.iter().filter(|e| !live(e)) {
        md += &format!("- {} › {} › {} (`{}`)\n", e.tab, e.panel, e.name, e.id);
    }
    std::fs::write(root.join("docs/parity.md"), md).map_err(|e| format!("docs/parity.md: {e}"))?;
    println!(
        "parity: in scope {scope_live}/{} ({:.0}%), SOLID+SKETCH {head_live}/{} ({:.0}%), all tabs {all_live}/{} → docs/parity.md",
        scope.len(),
        pct(scope_live, scope.len()),
        head.len(),
        pct(head_live, head.len()),
        catalog.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_from_menu_tree() {
        let v: Value = serde_json::from_str(
            r#"{"commandDefinitions":[{"id":"B","name":"Bee"}],"tabs":{"DesignProductType/SolidTab":{"panels":[{"name":"Create","controls":[
                {"type":"CommandControl","id":"A","command":{"id":"A","name":"Aye"}},
                {"type":"SeparatorControl","id":"s"},
                {"type":"DropDownControl","id":"d","controls":[{"type":"CommandControl","id":"B","command":{"id":"B"}}]}]}]}}}"#,
        )
        .unwrap();
        let e = from_menu_tree(&v);
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].tab.as_str(), e[0].panel.as_str(), e[0].id.as_str(), e[0].name.as_str()), ("SOLID", "CREATE", "A", "Aye"));
        assert_eq!(e[1].name, "Bee");
        let tsv = "# c\nSOLID\tCREATE\tA\tAye\n";
        assert_eq!(parse_catalog(tsv).len(), 1);
    }
}
