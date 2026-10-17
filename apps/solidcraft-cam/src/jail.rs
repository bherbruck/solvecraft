//! Path-jail helpers: ensure every file access stays within a bounded temp workspace.
//!
//! # Security
//! `check_in_jail` canonicalises both paths before comparing, so symlinks and
//! `..` traversals are blocked. The jail directory itself must already exist.

use std::path::{Path, PathBuf};

use crate::error::CamError;

/// Verify that `path` lies strictly within `jail` after canonicalisation.
///
/// Returns `Err(CamError::PathJailEscape)` if the path escapes.
pub fn check_in_jail(path: &Path, jail: &Path) -> Result<(), CamError> {
    let canonical_jail = jail.canonicalize()?;
    let canonical_path = path.canonicalize()?;
    if !canonical_path.starts_with(&canonical_jail) {
        return Err(CamError::PathJailEscape);
    }
    Ok(())
}

/// Build a child path inside `jail`, then verify it stays inside.
///
/// `name` must be a plain filename (no path separators). Returns the full path.
pub fn jail_child(jail: &Path, name: &str) -> Result<PathBuf, CamError> {
    // Reject names that contain any path separator or parent traversal component.
    if name.contains('/') || name.contains('\\') || name == ".." || name == "." {
        return Err(CamError::PathJailEscape);
    }
    let candidate = jail.join(name);
    // The file might not exist yet (we're about to create it), so check the parent.
    let parent = candidate.parent().ok_or(CamError::PathJailEscape)?;
    check_in_jail(parent, jail)?;
    Ok(candidate)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn valid_child_passes() {
        let dir = TempDir::new().unwrap();
        let child = jail_child(dir.path(), "model.step").unwrap();
        assert!(child.starts_with(dir.path()));
    }

    #[test]
    fn dotdot_in_name_is_rejected() {
        let dir = TempDir::new().unwrap();
        assert!(matches!(jail_child(dir.path(), ".."), Err(CamError::PathJailEscape)));
    }

    #[test]
    fn slash_in_name_is_rejected() {
        let dir = TempDir::new().unwrap();
        assert!(matches!(jail_child(dir.path(), "sub/escape"), Err(CamError::PathJailEscape)));
    }

    #[test]
    fn backslash_in_name_is_rejected() {
        let dir = TempDir::new().unwrap();
        assert!(matches!(jail_child(dir.path(), "sub\\escape"), Err(CamError::PathJailEscape)));
    }

    #[test]
    fn check_in_jail_rejects_outside_path() {
        let dir = TempDir::new().unwrap();
        // The system temp root is typically the parent of `dir`.
        let outside = dir.path().parent().unwrap_or(Path::new("/"));
        assert!(matches!(check_in_jail(outside, dir.path()), Err(CamError::PathJailEscape) | Err(CamError::Io(_))));
    }
}
