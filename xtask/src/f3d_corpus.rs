//! `cargo xtask f3d-corpus [DIR]`: open every `.f3d`/`.f3z` under DIR via the io crate test hook.
//!
//! Uses `F3D_ONE` + `fusion_open_one_file` so each file gets its own process (and you can wrap
//! this command in `timeout` per file on CI hosts). Default DIR when omitted:
//! `tests/f3d-offline-corpus` if it exists, else prints usage.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn run(workspace: &Path, dir: Option<&Path>, per_file_secs: u64) -> Result<(), String> {
    let dir = dir
        .map(Path::to_path_buf)
        .or_else(default_dir)
        .ok_or_else(|| "pass a directory of .f3d files, or create tests/f3d-offline-corpus".to_string())?;
    if !dir.is_dir() {
        return Err(format!("not a directory: {}", dir.display()));
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("read {}: {e}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("f3d") || e.eq_ignore_ascii_case("f3z")))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("no .f3d/.f3z under {}", dir.display()));
    }

    eprintln!("f3d-corpus: {} files under {}", files.len(), dir.display());
    let mut ok_solid = 0usize;
    let mut ok_mesh = 0usize;
    let mut failed = 0usize;
    let timeout = Duration::from_secs(per_file_secs.max(30));

    for path in &files {
        let label = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        eprintln!("\n--- {label} ---");
        let start = Instant::now();
        let mut child = Command::new("cargo")
            .args(["test", "-p", "solidcraft-io", "tests::fusion_open_one_file", "--", "--exact", "--nocapture"])
            .env("F3D_ONE", path)
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn cargo test: {e}"))?;

        let output = loop {
            if start.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait();
                eprintln!("RESULT TIMEOUT {label} (>{per_file_secs}s)");
                failed += 1;
                break None;
            }
            match child.try_wait() {
                Ok(Some(_)) => break Some(child.wait_with_output().map_err(|e| format!("wait: {e}"))?),
                Ok(None) => std::thread::sleep(Duration::from_millis(500)),
                Err(e) => return Err(format!("try_wait: {e}")),
            }
        };
        let Some(output) = output else { continue };
        let combined = String::from_utf8_lossy(&output.stderr).to_string() + &String::from_utf8_lossy(&output.stdout);
        for line in combined.lines() {
            if line.contains("RESULT ") {
                eprintln!("{line}");
                if line.contains("RESULT OK solid") {
                    ok_solid += 1;
                } else if line.contains("RESULT OK mesh") {
                    ok_mesh += 1;
                } else if line.contains("RESULT FAIL") {
                    failed += 1;
                }
                break;
            }
        }
        if !combined.contains("RESULT ") {
            eprintln!("RESULT FAIL {label}: no RESULT line (exit {:?})", output.status);
            failed += 1;
        }
    }

    eprintln!("\n=== summary: {} solid, {} mesh, {} failed / {} ===", ok_solid, ok_mesh, failed, ok_solid + ok_mesh + failed);
    Ok(())
}

fn default_dir() -> Option<PathBuf> {
    let p = PathBuf::from("tests/f3d-offline-corpus");
    p.is_dir().then_some(p)
}
