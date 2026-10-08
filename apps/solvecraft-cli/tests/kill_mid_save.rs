//! Kill the process while it saves: the design file is always the old version or the new one,
//! whole, and opens.

use std::process::Command;
use std::time::Duration;

fn round_of(path: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let doc = solvecraft_engine::io::read_design(&bytes).ok()?;
    doc.params.iter().find(|p| p.name == "save_round").map(|p| p.expr.clone())
}

#[test]
fn killed_mid_save_the_old_file_survives() {
    let exe = env!("CARGO_BIN_EXE_solvecraft-cli");
    let dir = std::env::temp_dir().join(format!("solvecraft-kill-mid-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("design.solvecraft");
    let ok = Command::new(exe).args(["save-stress", &path.to_string_lossy(), "1"]).status().unwrap();
    assert!(ok.success());
    assert_eq!(round_of(&path).as_deref(), Some("0"));
    let mut kills = 0;
    let mut seen = std::collections::BTreeSet::new();
    for i in 0..25u64 {
        let mut child = Command::new(exe).args(["save-stress", &path.to_string_lossy(), "100000"]).spawn().unwrap();
        // Let it get into its save loop (building the design takes a moment), then kill it.
        std::thread::sleep(Duration::from_millis(150 + (i * 37) % 200));
        if child.try_wait().unwrap().is_none() {
            child.kill().unwrap();
            kills += 1;
        }
        let _ = child.wait();
        let round = round_of(&path);
        assert!(round.is_some(), "after kill {i} the design file is torn or missing");
        seen.insert(round);
        let bak = solvecraft_engine::io::backup_path(&path);
        if bak.exists() {
            assert!(round_of(&bak).is_some(), "after kill {i} the backup is torn");
        }
    }
    assert!(kills > 20, "the saver was killed while running ({kills})");
    assert!(seen.len() > 5, "the saver got to saving before it was killed ({seen:?})");
    let _ = std::fs::remove_dir_all(&dir);
}
