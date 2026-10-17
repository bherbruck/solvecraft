//! Centralised modifier-key binding types for sketch and selection behaviours.
//!
//! [`ModifierCombo`] is a serialisable set of modifier keys.  Modifier state that
//! must cross the boundary between the viewport event-loop and rendering helpers
//! (gizmo, canvas) that do not carry `&SolveApp` is communicated through
//! per-frame thread-local flags — the same pattern as [`crate::drag_snap`] —
//! so that neither gizmo nor canvas needs access to application state.

use std::cell::Cell;

use egui::Modifiers;
use serde::{Deserialize, Serialize};

/// A set of modifier keys for a binding.
///
/// `ctrl` is platform-normalised: it means Ctrl on Windows/Linux and Cmd on macOS.
/// Matching helpers honour both `m.ctrl` and `m.command` for the `ctrl` flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ModifierCombo {
    pub shift: bool,
    /// Ctrl (Windows/Linux) or Cmd (macOS).
    pub ctrl: bool,
    pub alt: bool,
}

impl ModifierCombo {
    pub const NONE: Self = Self { shift: false, ctrl: false, alt: false };
    pub const SHIFT: Self = Self { shift: true, ctrl: false, alt: false };
    pub const CTRL: Self = Self { shift: false, ctrl: true, alt: false };
    pub const ALT: Self = Self { shift: false, ctrl: false, alt: true };
    pub const CTRL_ALT: Self = Self { shift: false, ctrl: true, alt: true };

    /// True if **exactly** these modifiers (and no others) are held.
    pub fn matches_exact(self, m: Modifiers) -> bool {
        let ctrl = m.ctrl || m.command;
        self.shift == m.shift && self.ctrl == ctrl && self.alt == m.alt
    }

    /// True if **at least** these modifiers are held (subset / "held" match).
    pub fn held(self, m: Modifiers) -> bool {
        let ctrl = m.ctrl || m.command;
        (!self.shift || m.shift) && (!self.ctrl || ctrl) && (!self.alt || m.alt)
    }

    /// Human-readable display label: `"Ctrl/Cmd"`, `"Shift+Alt"`, etc.
    pub fn label(self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.shift {
            parts.push("Shift");
        }
        if self.ctrl {
            parts.push(ctrl_name());
        }
        if self.alt {
            parts.push(alt_name());
        }
        if parts.is_empty() {
            return "None".into();
        }
        parts.join("+")
    }

    /// All single-key combos, in display order (for a picker widget).
    pub const SINGLE_KEYS: [Self; 3] = [Self::CTRL, Self::ALT, Self::SHIFT];
}

/// "Ctrl/Cmd" on Windows/Linux, "Cmd" on macOS.
pub fn ctrl_name() -> &'static str {
    if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl/Cmd" }
}

/// "Alt" on Windows/Linux, "Option" on macOS.
pub fn alt_name() -> &'static str {
    if cfg!(target_os = "macos") { "Option" } else { "Alt" }
}

thread_local! {
    /// Set once per frame from the viewport event-loop; read by gizmo and canvas
    /// helpers that do not receive `&SolveApp`.  Communicates whether the current
    /// modifier state enables free (unsnapped) manipulator drags.
    static MANIPULATOR_FREE: Cell<bool> = const { Cell::new(false) };
}

/// Set whether manipulator drags are free (no snap) this frame.
///
/// Call from the viewport *before* canvas / gizmo rendering, passing the result of
/// [`crate::prefs::manipulator_free_held`].
pub fn set_manipulator_free(on: bool) {
    MANIPULATOR_FREE.with(|c| c.set(on));
}

/// Whether manipulator drags are free this frame (set by [`set_manipulator_free`]).
pub fn manipulator_free() -> bool {
    MANIPULATOR_FREE.with(Cell::get)
}

// ---- tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(shift: bool, ctrl: bool, alt: bool) -> Modifiers {
        Modifiers { shift, ctrl, alt, ..Default::default() }
    }

    #[test]
    fn exact_match_requires_no_extra_keys() {
        let ctrl = ModifierCombo::CTRL;
        assert!(ctrl.matches_exact(mods(false, true, false)));
        assert!(!ctrl.matches_exact(mods(true, true, false)), "extra shift must fail");
        assert!(!ctrl.matches_exact(mods(false, false, false)));
    }

    #[test]
    fn held_match_allows_extra_keys() {
        let ctrl = ModifierCombo::CTRL;
        assert!(ctrl.held(mods(false, true, false)));
        assert!(ctrl.held(mods(true, true, false)), "shift+ctrl is fine for a ctrl binding");
        assert!(!ctrl.held(mods(false, false, false)));
    }

    #[test]
    fn cmd_normalises_to_ctrl() {
        let ctrl = ModifierCombo::CTRL;
        let cmd_mods = Modifiers { command: true, ..Default::default() };
        assert!(ctrl.held(cmd_mods), "Cmd on macOS should match a ctrl binding");
        assert!(ctrl.matches_exact(cmd_mods), "exact match also normalises command");
    }

    #[test]
    fn label_formats_correctly() {
        assert_eq!(ModifierCombo::CTRL.label(), "Ctrl/Cmd");
        assert_eq!(ModifierCombo::NONE.label(), "None");
        assert_eq!(ModifierCombo::CTRL_ALT.label(), "Ctrl/Cmd+Alt");
        assert_eq!(ModifierCombo { shift: true, ctrl: true, alt: false }.label(), "Shift+Ctrl/Cmd");
    }

    #[test]
    fn manipulator_free_thread_local_roundtrip() {
        set_manipulator_free(true);
        assert!(manipulator_free());
        set_manipulator_free(false);
        assert!(!manipulator_free());
    }
}
