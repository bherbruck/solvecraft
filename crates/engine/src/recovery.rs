//! Autosave and crash recovery.
//!
//! A running app keeps one recovery entry in the recovery folder: `<id>.solvecraft` (the design
//! as of the last autosave), `<id>.json` (its file path, name and time) and `<id>.lock`, which
//! the app holds locked while it runs. Autosaves are written atomically, only when the design
//! has unsaved changes it hasn't written yet; a design with nothing unsaved has no entry. An
//! entry whose lock nobody holds belongs to an app that crashed (or closed with unsaved changes):
//! the next launch offers to recover it.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use solvecraft_doc::Document;

use crate::{EngineError, Result, Session};

/// The recovery folder: `SOLVECRAFT_RECOVERY_DIR`, else the platform's per-user data folder.
pub fn default_dir() -> Option<PathBuf> {
    // Tests never touch the user's real recovery folder.
    if cfg!(test) {
        return Some(std::env::temp_dir().join(format!("solvecraft-test-recovery-{}", std::process::id())));
    }
    if let Some(d) = std::env::var_os("SOLVECRAFT_RECOVERY_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    let env = |k: &str| std::env::var_os(k).filter(|d| !d.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("LOCALAPPDATA")?.join("SolveCraft")
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support/SolveCraft")
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local/share")))?.join("solvecraft")
    };
    Some(base.join("recovery"))
}

/// What an entry's `<id>.json` holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// The design's file, if it had one.
    pub path: Option<String>,
    pub name: String,
    /// When it was autosaved (seconds since 1970).
    pub saved_at: u64,
    pub features: usize,
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn file(dir: &Path, id: &str, ext: &str) -> PathBuf {
    dir.join(format!("{id}.{ext}"))
}

/// Entries left by apps that are no longer running, newest first.
pub fn orphans(dir: &Path) -> Vec<Entry> {
    let mut out: Vec<Entry> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                return None;
            }
            let entry: Entry = serde_json::from_slice(&fs::read(&p).ok()?).ok()?;
            if !valid_id(&entry.id) || !file(dir, &entry.id, "solvecraft").is_file() {
                return None;
            }
            // Held by a running app: not an orphan.
            let lock = File::open(file(dir, &entry.id, "lock"));
            if let Ok(l) = &lock
                && l.try_lock().is_err()
            {
                return None;
            }
            Some(entry)
        })
        .take(1000)
        .collect();
    out.sort_by_key(|e| std::cmp::Reverse(e.saved_at));
    out
}

/// Read an entry's design.
pub fn load(dir: &Path, id: &str) -> Result<(Document, Entry)> {
    if !valid_id(id) {
        return Err(EngineError::Other(format!("no recovered design `{id}`")));
    }
    let entry: Entry = fs::read(file(dir, id, "json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or_else(|| EngineError::Other(format!("no recovered design `{id}`")))?;
    let bytes = fs::read(file(dir, id, "solvecraft")).map_err(|e| EngineError::Other(format!("recovered design `{id}`: {e}")))?;
    Ok((solvecraft_io::read_design(&bytes)?, entry))
}

/// Delete an entry.
pub fn remove(dir: &Path, id: &str) {
    if valid_id(id) {
        for ext in ["json", "solvecraft", "lock"] {
            let _ = fs::remove_file(file(dir, id, ext));
        }
    }
}

/// One app's autosaver.
pub struct Autosaver {
    pub dir: PathBuf,
    pub id: String,
    /// Autosave this often while there are unsaved changes.
    pub interval: Duration,
    lock: Option<File>,
    last: Option<Arc<Document>>,
    last_at: Instant,
    has_entry: bool,
}

impl Autosaver {
    /// Start autosaving into `dir` (created if needed), holding this app's lock.
    pub fn new(dir: &Path, interval: Duration) -> Result<Autosaver> {
        let io = |e: std::io::Error| EngineError::Other(format!("{}: {e}", dir.display()));
        fs::create_dir_all(dir).map_err(io)?;
        // Locks of apps that ended with nothing to recover.
        for e in fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("lock")
                && !p.with_extension("json").exists()
                && File::open(&p).is_ok_and(|l| l.try_lock().is_ok())
            {
                let _ = fs::remove_file(&p);
            }
        }
        let id = format!("{}-{}", unix_now(), std::process::id());
        let lock = OpenOptions::new().create(true).truncate(false).write(true).open(file(dir, &id, "lock")).map_err(io)?;
        lock.lock().map_err(io)?;
        Ok(Autosaver { dir: dir.to_path_buf(), id, interval, lock: Some(lock), last: None, last_at: Instant::now(), has_entry: false })
    }

    /// Autosave now if the design has unsaved changes not written yet (and drop the entry when
    /// nothing is unsaved). Returns whether it wrote.
    pub fn save(&mut self, s: &Session) -> Result<bool> {
        self.last_at = Instant::now();
        if !s.is_dirty() {
            if self.has_entry {
                for ext in ["json", "solvecraft"] {
                    let _ = fs::remove_file(file(&self.dir, &self.id, ext));
                }
                self.has_entry = false;
            }
            self.last = None;
            return Ok(false);
        }
        if self.last.as_ref().is_some_and(|l| Arc::ptr_eq(l, &s.doc) || **l == *s.doc) {
            return Ok(false);
        }
        let io = |e: std::io::Error| EngineError::Other(format!("autosave: {e}"));
        solvecraft_io::write_atomic(&file(&self.dir, &self.id, "solvecraft"), &solvecraft_io::write_design(&s.doc), false).map_err(io)?;
        let entry =
            Entry { id: self.id.clone(), path: s.path.clone(), name: s.doc.name.clone(), saved_at: unix_now(), features: s.doc.features.len() };
        let meta = serde_json::to_vec_pretty(&entry).map_err(|e| EngineError::Other(e.to_string()))?;
        solvecraft_io::write_atomic(&file(&self.dir, &self.id, "json"), &meta, false).map_err(io)?;
        self.last = Some(s.doc.clone());
        self.has_entry = true;
        Ok(true)
    }

    /// Autosave when the interval has passed, or right away after a big operation.
    pub fn tick(&mut self, s: &Session, big: bool) -> Result<bool> {
        if big || self.last_at.elapsed() >= self.interval { self.save(s) } else { Ok(false) }
    }

    /// Closing the app: with nothing unsaved the entry goes; otherwise it stays for the next
    /// launch to offer. The lock is released either way.
    pub fn close(mut self, s: &Session) {
        let _ = self.save(s);
        if !self.has_entry {
            let _ = fs::remove_file(file(&self.dir, &self.id, "lock"));
        }
        self.lock = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("solvecraft-recovery-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn autosave_only_unsaved_changes_and_recover_after_a_crash() {
        let d = dir("crash");
        let mut s = Session::default();
        let mut a = Autosaver::new(&d, Duration::from_secs(3600)).unwrap();
        assert!(!a.save(&s).unwrap(), "nothing unsaved");
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        assert!(!a.tick(&s, false).unwrap(), "not due yet");
        assert!(a.tick(&s, true).unwrap(), "after a big operation");
        assert!(!a.save(&s).unwrap(), "unchanged since the last autosave");
        // Our own entry is locked: not offered.
        assert!(orphans(&d).is_empty());
        // The app dies without closing (the lock goes with the process).
        drop(a);
        let found = orphans(&d);
        assert_eq!(found.len(), 1);
        let (doc, entry) = load(&d, &found[0].id).unwrap();
        assert_eq!(doc, *s.doc);
        assert_eq!(entry.features, 1);
        remove(&d, &entry.id);
        assert!(orphans(&d).is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn saving_drops_the_entry_and_a_clean_close_leaves_nothing() {
        let d = dir("clean");
        let mut s = Session::default();
        let mut a = Autosaver::new(&d, Duration::ZERO).unwrap();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        assert!(a.tick(&s, false).unwrap());
        s.mark_saved();
        a.tick(&s, false).unwrap();
        assert!(!file(&d, &a.id, "solvecraft").exists());
        a.close(&s);
        assert_eq!(fs::read_dir(&d).unwrap().count(), 0);
        // Closing with unsaved changes keeps them for the next launch.
        let b = Autosaver::new(&d, Duration::ZERO).unwrap();
        s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5})).unwrap();
        b.close(&s);
        assert_eq!(orphans(&d).len(), 1);
        assert!(load(&d, "../etc").is_err());
        let _ = fs::remove_dir_all(&d);
    }
}
