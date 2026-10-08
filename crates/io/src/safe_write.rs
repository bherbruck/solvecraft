//! Crash-safe file writes: the new content goes to a temporary file next to the target, is
//! flushed to disk, and replaces the target in one rename. A crash or power cut at any moment
//! leaves the old file or the new one, never a torn mix. The previous version can be kept as
//! `<file>.bak` (itself written the same way).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The backup of a file written with `backup: true`.
pub fn backup_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".bak");
    PathBuf::from(s)
}

fn temp_beside(path: &Path, tag: &str) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    path.with_file_name(format!(".{name}.{tag}.{}.{nanos}.tmp", std::process::id()))
}

fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

/// Flush the directory entry (the rename) to disk where the platform allows it.
fn sync_dir(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty())
        && let Ok(d) = fs::File::open(dir)
    {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Steps of a save, so tests can stop one part way (as a crash would).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Step {
    TempWritten,
    BackupWritten,
    #[allow(dead_code)] // stopping after the last step is a whole save (tests name it)
    Replaced,
}

fn write_steps(path: &Path, bytes: &[u8], backup: bool, stop: Option<Step>) -> std::io::Result<()> {
    let tmp = temp_beside(path, "new");
    if let Err(e) = write_synced(&tmp, bytes) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    if stop == Some(Step::TempWritten) {
        return Ok(());
    }
    if backup && path.is_file() {
        // Copy (not move) the old file, so the target exists at every moment.
        let old = fs::read(path);
        let bak_tmp = temp_beside(path, "bak");
        let r = old.and_then(|old| write_synced(&bak_tmp, &old)).and_then(|_| fs::rename(&bak_tmp, backup_path(path)));
        if let Err(e) = r {
            let _ = fs::remove_file(&bak_tmp);
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
    }
    if stop == Some(Step::BackupWritten) {
        return Ok(());
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    sync_dir(path);
    // Temporary files of saves that crashed part way (a minute old or more) go.
    for t in stale_temps(path) {
        if fs::metadata(&t).and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).is_some_and(|age| age.as_secs() >= 60) {
            let _ = fs::remove_file(t);
        }
    }
    Ok(())
}

/// Write `bytes` to `path` atomically; with `backup`, the file being replaced is kept as
/// `<path>.bak` first.
pub fn write_atomic(path: &Path, bytes: &[u8], backup: bool) -> std::io::Result<()> {
    // In the browser the host stores files (one write is already atomic there).
    if cfg!(target_arch = "wasm32") {
        let _ = backup;
        return crate::vfs::write(&path.to_string_lossy(), bytes);
    }
    write_steps(path, bytes, backup, None)
}

/// Leftover temporary files of saves that never finished (a crash part way), next to `path`.
pub fn stale_temps(path: &Path) -> Vec<PathBuf> {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else { return Vec::new() };
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let prefix = format!(".{name}.");
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().map(|n| n.to_string_lossy().to_string()).is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".tmp")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("solvecraft-safe-write-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn replaces_and_keeps_a_backup() {
        let d = dir("bak");
        let p = d.join("a.solvecraft");
        write_atomic(&p, b"one", true).unwrap();
        assert!(!backup_path(&p).exists(), "nothing to back up the first time");
        write_atomic(&p, b"two", true).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(fs::read(backup_path(&p)).unwrap(), b"one");
        write_atomic(&p, b"three", false).unwrap();
        assert_eq!(fs::read(backup_path(&p)).unwrap(), b"one");
        assert!(stale_temps(&p).is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    /// A save stopped after any step leaves the old file whole (or the new one).
    #[test]
    fn a_save_stopped_part_way_keeps_the_old_file() {
        let d = dir("stop");
        let p = d.join("a.solvecraft");
        write_atomic(&p, b"old", true).unwrap();
        for stop in [Step::TempWritten, Step::BackupWritten, Step::Replaced] {
            write_steps(&p, b"new content", true, Some(stop)).unwrap();
            let now = fs::read(&p).unwrap();
            assert!(now == b"old" || now == b"new content", "{stop:?}: {now:?}");
            if stop < Step::Replaced {
                assert_eq!(now, b"old");
                assert!(!stale_temps(&p).is_empty(), "the unfinished save's temporary file is left over");
                for t in stale_temps(&p) {
                    fs::remove_file(t).unwrap();
                }
            }
            write_atomic(&p, b"old", false).unwrap();
        }
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failed_write_leaves_the_target() {
        let d = dir("fail");
        let p = d.join("missing-dir").join("a.solvecraft");
        assert!(write_atomic(&p, b"x", true).is_err());
        assert!(!p.exists());
        let _ = fs::remove_dir_all(&d);
    }
}
