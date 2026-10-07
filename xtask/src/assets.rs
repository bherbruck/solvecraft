//! `cargo xtask assets`: every non-code asset in the repository must have a row in
//! ATTRIBUTION.md (author, source, licence). Icons are drawn in code and need no file.

use std::path::Path;

const ASSET_EXT: &[&str] =
    &["png", "jpg", "jpeg", "svg", "ico", "icns", "ttf", "otf", "woff", "woff2", "gif", "bmp", "webp", "wav", "mp3", "stl", "step", "stp", "obj"];
const SKIP_DIRS: &[&str] = &["target", ".git", "plan", "dist"];

fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                walk(&p, root, out);
            }
        } else if let Some(ext) = p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase())
            && ASSET_EXT.contains(&ext.as_str())
            && let Ok(rel) = p.strip_prefix(root)
        {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

pub fn run(root: &Path) -> Result<(), String> {
    let attribution = std::fs::read_to_string(root.join("ATTRIBUTION.md")).map_err(|e| format!("ATTRIBUTION.md: {e}"))?;
    let mut files = Vec::new();
    walk(root, root, &mut files);
    files.sort();
    let missing: Vec<&String> = files.iter().filter(|f| !attribution.contains(f.as_str())).collect();
    if missing.is_empty() {
        println!("assets: {} asset file(s), all attributed", files.len());
        Ok(())
    } else {
        Err(format!("assets missing from ATTRIBUTION.md:\n{}", missing.iter().map(|m| format!("  {m}")).collect::<Vec<_>>().join("\n")))
    }
}
