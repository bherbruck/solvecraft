//! The one place windows are made. Every command dialog, panel and pop-up window comes from
//! here, so they all shrink to their content within the same width limits (a test checks that no
//! other file creates an `egui::Window`, and that every dialog stays within the limits).
//!
//! - [`docked`]: command dialogs and command panels, flush to the right edge of the viewport.
//! - [`window`]: floating windows (Preferences, Parameters, prompts…), centred by default.
//! - [`right_aligned`]: right-align a row's last items without widening the window (a
//!   right-to-left layout would stretch it to its maximum width).

use egui::{Align2, Pos2, Ui, WidgetText, vec2};

/// Width limits of a docked command dialog.
pub const DIALOG_MIN_W: f32 = 260.0;
pub const DIALOG_MAX_W: f32 = 380.0;
/// Docked dialogs holding a table (motion study, configurations, exploded views).
pub const DIALOG_WIDE_MAX_W: f32 = 560.0;
/// Floating windows: prompts and panels, and those holding a table (Parameters, Keyboard
/// Shortcuts).
pub const WINDOW_MAX_W: f32 = 460.0;
pub const WINDOW_WIDE_MAX_W: f32 = 760.0;

/// How wide a window may get.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    Normal,
    Wide,
}

/// A command dialog docked at `anchor` (its top right corner), shrink-wrapped to its content.
pub fn docked<'a>(title: impl Into<WidgetText>, id: egui::Id, anchor: Pos2, max_height: f32, width: Width) -> egui::Window<'a> {
    let max = match width {
        Width::Normal => DIALOG_MAX_W,
        Width::Wide => DIALOG_WIDE_MAX_W,
    };
    egui::Window::new(title)
        .id(id)
        .title_bar(false)
        .pivot(Align2::RIGHT_TOP)
        .fixed_pos(anchor)
        .constrain(false)
        .auto_sized()
        .collapsible(false)
        .max_width(max)
        .max_height(max_height.max(120.0))
}

/// A floating window, centred on the screen unless the caller places it, shrink-wrapped to its
/// content up to its width limit.
pub fn window<'a>(ctx: &egui::Context, title: impl Into<WidgetText>, width: Width) -> egui::Window<'a> {
    let max = match width {
        Width::Normal => WINDOW_MAX_W,
        Width::Wide => WINDOW_WIDE_MAX_W,
    };
    let c = ctx.content_rect().center();
    egui::Window::new(title).collapsible(false).pivot(Align2::CENTER_CENTER).default_pos(c - vec2(0.0, 60.0)).max_width(max)
}

/// The width a window's content had last frame (at least `min`), for laying out rows that end at
/// the right edge.
pub fn content_width(ui: &Ui, key: egui::Id, min: f32) -> f32 {
    ui.ctx().data(|d| d.get_temp::<f32>(key)).unwrap_or(min).max(min)
}

/// Remember the content width for next frame (call at the end of the window's content).
pub fn remember_width(ui: &Ui, key: egui::Id) {
    let w = ui.min_rect().width();
    ui.ctx().data_mut(|d| d.insert_temp(key, w));
}

/// In a horizontal row: push what `add` draws (`width` wide) to the right edge of a content
/// `total` wide, without making the row any wider than that.
pub fn right_aligned<R>(ui: &mut Ui, total: f32, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let used = ui.min_rect().width();
    let gap = (total - used - width - ui.spacing().item_spacing.x).max(0.0);
    ui.add_space(gap);
    add(ui)
}

#[cfg(test)]
mod tests {
    /// Windows are made in this file only, so none can skip the width limits.
    #[test]
    fn windows_come_from_the_frame_helper() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut bad = Vec::new();
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "rs") && p.file_name().is_some_and(|n| n != "frame.rs") {
                let text = std::fs::read_to_string(&p).unwrap();
                if text.contains("Window::new(") {
                    bad.push(p.display().to_string());
                }
            }
        }
        assert!(bad.is_empty(), "use crate::frame::{{docked, window}} instead of egui::Window::new in {bad:?}");
    }
}
