//! Preferences (File › Preferences), in the spirit of Fusion's: General (theme, units of new
//! designs, autosave interval), Navigation (mouse mapping preset, zoom direction, orbit centre), Display
//! (grid, perspective, visual style with or without edges, display precision, anti-aliasing), Sketch and Selection. Everything
//! applies at once and is kept between runs, except anti-aliasing, which needs a restart.

use egui::{Modifiers, Pos2, RichText};
use serde::{Deserialize, Serialize};
use serde_json::json;
use solvecraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::theme::Tokens;
use crate::viewport::NavMode;

pub const UNITS: [&str; 5] = ["mm", "cm", "m", "in", "ft"];
/// Mouse mappings: (id, label).
pub const NAV_PRESETS: [(&str, &str); 3] = [("fusion", "Fusion"), ("solidworks", "SolidWorks"), ("inventor", "Inventor")];
pub const MSAA: [u32; 4] = [1, 2, 4, 8];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Units of new designs.
    pub default_units: String,
    /// Mouse mapping: `fusion`, `solidworks` or `inventor`.
    pub nav: String,
    /// The wheel zooms out when turned forward.
    pub zoom_reverse: bool,
    /// Orbit around the point under the cursor (else around the view's centre).
    pub orbit_cursor: bool,
    /// Anti-aliasing samples (applies on restart).
    pub msaa: u32,
    /// Decimals shown for lengths and angles.
    pub length_decimals: u8,
    pub angle_decimals: u8,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            default_units: "mm".into(),
            nav: "fusion".into(),
            zoom_reverse: false,
            orbit_cursor: false,
            msaa: 4,
            length_decimals: 2,
            angle_decimals: 1,
        }
    }
}

/// The preferences window.
#[derive(Default)]
pub struct PrefsWindow {
    pub open: bool,
    section: usize,
}

/// A new, empty design takes the preferred units (not an undo step, not a change to save).
pub fn apply_new_design(app: &mut SolveApp) {
    if app.session.doc.features.is_empty() && app.session.doc.units != app.preferences.default_units {
        let saved = !app.session.is_dirty();
        if app.session.execute("document.units", &json!({ "units": app.preferences.default_units })).is_ok() {
            app.session.undo.pop();
            if saved {
                app.session.mark_saved();
            }
        }
    }
}

/// The autosave interval follows `app.autosave_minutes` (0: off).
pub fn set_autosave(app: &mut SolveApp) {
    let m = app.autosave_minutes;
    if let Some(a) = app.autosave.as_mut() {
        a.interval = if m > 0.0 { std::time::Duration::from_secs_f64(m * 60.0) } else { std::time::Duration::from_secs(u64::MAX / 4) };
    }
}

/// Which navigation a mouse drag does under the preset.
pub fn nav_mode(app: &SolveApp, middle: bool, secondary: bool, mods: Modifiers) -> Option<NavMode> {
    if secondary && !middle {
        return Some(NavMode::Orbit);
    }
    if !middle {
        return None;
    }
    let ctrl = mods.ctrl || mods.command;
    match app.preferences.nav.as_str() {
        // Middle orbits, Ctrl+middle pans, Shift+middle zooms.
        "solidworks" => Some(if ctrl {
            NavMode::Pan
        } else if mods.shift {
            NavMode::Zoom
        } else {
            NavMode::Orbit
        }),
        // Middle pans, Shift+middle orbits, Ctrl+middle zooms.
        "inventor" => Some(if mods.shift {
            NavMode::Orbit
        } else if ctrl {
            NavMode::Zoom
        } else {
            NavMode::Pan
        }),
        // Fusion: middle pans, Shift+middle orbits.
        _ => Some(if mods.shift { NavMode::Orbit } else { NavMode::Pan }),
    }
}

/// +1, or −1 when the wheel zooms the other way.
pub fn zoom_sign(app: &SolveApp) -> f64 {
    if app.preferences.zoom_reverse { -1.0 } else { 1.0 }
}

/// Orbit by pixel deltas: around the view's centre, or, with "orbit around the cursor", around
/// the model point that was under the cursor when the drag began (kept still on screen).
pub fn orbit(app: &mut SolveApp, rect: egui::Rect, dx: f64, dy: f64, press: Option<Pos2>) {
    if !app.preferences.orbit_cursor {
        app.cam.orbit(dx, dy);
        return;
    }
    let pivot = match app.viewport.orbit_pivot {
        Some(p) => p,
        None => {
            let proj = crate::viewport::projection(app, rect);
            let at = press.unwrap_or(rect.center());
            let p = pivot_under(app, &proj, at);
            app.viewport.orbit_pivot = Some(p);
            p
        }
    };
    let before = crate::viewport::projection(app, rect).to_screen(pivot);
    app.cam.orbit(dx, dy);
    let after = crate::viewport::projection(app, rect).to_screen(pivot);
    if let (Some(a), Some(b)) = (before, after) {
        app.cam.pan(f64::from(a.x - b.x), f64::from(a.y - b.y), f64::from(rect.height().max(1.0)));
    }
}

/// The model point under a screen position, or the point at the view's depth.
fn pivot_under(app: &SolveApp, proj: &crate::viewport::Proj, at: Pos2) -> Vec3 {
    use crate::viewport::Hit;
    for h in crate::viewport::pick(app, proj, at) {
        match h {
            Hit::Face { point, .. } | Hit::Vertex { point, .. } => return point,
            Hit::Edge { mid, .. } => return mid,
            _ => {}
        }
    }
    let (o, d) = proj.ray(at);
    let n = app.cam.back();
    let den = d.dot(n);
    if den.abs() > 1e-9 { o + d * ((app.cam.target - o).dot(n) / den) } else { app.cam.target }
}

/// Units per millimetre and the unit's name.
pub fn unit_scale(units: &str) -> (f64, &'static str) {
    match units {
        "cm" => (0.1, "cm"),
        "m" => (0.001, "m"),
        "in" => (1.0 / 25.4, "in"),
        "ft" => (1.0 / 304.8, "ft"),
        _ => (1.0, "mm"),
    }
}

/// A length in millimetres in `units`, with `decimals` (for display).
pub fn format_length(mm: f64, units: &str, decimals: u8) -> String {
    let (k, u) = unit_scale(units);
    format!("{:.*} {u}", usize::from(decimals.min(8)), mm * k)
}

/// A length, area (`power` 2) or volume (3) given in millimetres, shown in the design's units
/// with the length precision.
pub fn show_mm(app: &SolveApp, v: f64, power: i32) -> String {
    let (k, u) = unit_scale(&app.session.doc.units);
    let sup = match power {
        2 => "²",
        3 => "³",
        _ => "",
    };
    format!("{:.*} {u}{sup}", usize::from(app.preferences.length_decimals.min(8)), v * k.powi(power))
}

/// Numbers in millimetres, in the design's units without the unit name.
pub fn show_mm_bare(app: &SolveApp, v: f64) -> String {
    let (k, _) = unit_scale(&app.session.doc.units);
    format!("{:.*}", usize::from(app.preferences.length_decimals.min(8)), v * k)
}

/// Write the anti-aliasing setting where the host reads it at start (the window's samples can't
/// change while it is open).
fn save_graphics(app: &SolveApp) {
    if let Some(path) = &app.services.graphics_path {
        let _ = std::fs::write(path, json!({ "msaa": app.preferences.msaa }).to_string());
    }
}

pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.prefs_window.open {
        return;
    }
    let t = Tokens::get();
    let mut open = true;
    let sections = ["General", "Navigation", "Display", "Sketch", "Selection"];
    let before = app.preferences.clone();
    crate::frame::window(ctx, "Preferences", crate::frame::Width::Wide)
        .id(egui::Id::new("sc_prefs"))
        .open(&mut open)
        .default_size([560.0, 360.0])
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(120.0);
                    for (i, s) in sections.iter().enumerate() {
                        if ui.selectable_label(app.prefs_window.section == i, *s).clicked() {
                            app.prefs_window.section = i;
                        }
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    egui::Grid::new("sc_prefs_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| match app.prefs_window.section {
                        0 => {
                            ui.label("Theme");
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut app.ui.dark, true, "Dark");
                                ui.radio_value(&mut app.ui.dark, false, "Light");
                            });
                            ui.end_row();
                            ui.label("Units of new designs");
                            egui::ComboBox::from_id_salt("pref_units").selected_text(app.preferences.default_units.clone()).show_ui(ui, |ui| {
                                for u in UNITS {
                                    ui.selectable_value(&mut app.preferences.default_units, u.to_string(), u);
                                }
                            });
                            ui.end_row();
                            ui.label("Autosave every");
                            if ui.add(egui::DragValue::new(&mut app.autosave_minutes).range(0.0..=120.0).speed(0.5).suffix(" min")).changed() {
                                set_autosave(app);
                            }
                            ui.end_row();
                            ui.label("");
                            ui.label(RichText::new("0 turns it off").size(11.0).color(t.text_dim));
                            ui.end_row();
                        }
                        1 => {
                            ui.label("Mouse");
                            egui::ComboBox::from_id_salt("pref_nav")
                                .selected_text(NAV_PRESETS.iter().find(|(id, _)| *id == app.preferences.nav).map_or("Fusion", |x| x.1))
                                .show_ui(ui, |ui| {
                                    for (id, label) in NAV_PRESETS {
                                        if ui.selectable_value(&mut app.preferences.nav, id.to_string(), label).clicked() {
                                            app.preferences.zoom_reverse = id == "solidworks";
                                        }
                                    }
                                });
                            ui.end_row();
                            ui.label("");
                            let how = match app.preferences.nav.as_str() {
                                "solidworks" => "Middle drag orbits, Ctrl+middle pans, Shift+middle zooms",
                                "inventor" => "Middle drag pans, Shift+middle orbits, Ctrl+middle zooms",
                                _ => "Middle drag pans, Shift+middle orbits",
                            };
                            ui.label(RichText::new(format!("{how}; right drag orbits; the wheel zooms at the cursor")).size(11.0).color(t.text_dim));
                            ui.end_row();
                            ui.label("Reverse zoom direction");
                            ui.checkbox(&mut app.preferences.zoom_reverse, "");
                            ui.end_row();
                            ui.label("Orbit around");
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut app.preferences.orbit_cursor, false, "View centre");
                                ui.radio_value(&mut app.preferences.orbit_cursor, true, "Point under the cursor");
                            });
                            ui.end_row();
                        }
                        2 => {
                            ui.label("Grid");
                            ui.checkbox(&mut app.ui.show_grid, "");
                            ui.end_row();
                            ui.label("Perspective view");
                            ui.checkbox(&mut app.ui.perspective, "");
                            ui.end_row();
                            ui.label("Visual style");
                            let styles = ["Shaded with edges", "Shaded", "Wireframe", "Shaded with hidden edges"];
                            egui::ComboBox::from_id_salt("pref_style").selected_text(styles[usize::from(app.ui.visual_style.min(3))]).show_ui(
                                ui,
                                |ui| {
                                    for (i, s) in styles.iter().enumerate() {
                                        ui.selectable_value(&mut app.ui.visual_style, i as u8, *s);
                                    }
                                },
                            );
                            ui.end_row();
                            ui.label("Length decimals");
                            ui.add(egui::DragValue::new(&mut app.preferences.length_decimals).range(0..=6));
                            ui.end_row();
                            ui.label("Angle decimals");
                            ui.add(egui::DragValue::new(&mut app.preferences.angle_decimals).range(0..=6));
                            ui.end_row();
                            ui.label("Anti-aliasing");
                            egui::ComboBox::from_id_salt("pref_msaa").selected_text(format!("{}×", app.preferences.msaa)).show_ui(ui, |ui| {
                                for m in MSAA {
                                    ui.selectable_value(&mut app.preferences.msaa, m, format!("{m}×"));
                                }
                            });
                            ui.end_row();
                            ui.label("");
                            ui.label(RichText::new("Anti-aliasing applies the next time SolveCraft starts").size(11.0).color(t.text_dim));
                            ui.end_row();
                        }
                        3 => {
                            ui.label("Project face edges into new sketches");
                            let mut auto = app.session.auto_project;
                            if ui.checkbox(&mut auto, "").changed() {
                                let _ = app.run("sketch.auto_project", json!({ "value": auto }));
                            }
                            ui.end_row();
                        }
                        _ => {
                            ui.label("Click selects whole bodies");
                            ui.checkbox(&mut app.ui.pick_bodies, "");
                            ui.end_row();
                        }
                    });
                });
            });
        });
    if app.preferences.msaa != before.msaa {
        save_graphics(app);
    }
    if !open {
        app.prefs_window.open = false;
    }
}

/// Set preferences from JSON (control channel); returns them all.
pub fn set(app: &mut SolveApp, p: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut cur = serde_json::to_value(&app.preferences).map_err(|e| e.to_string())?;
    if let (Some(o), Some(src)) = (cur.as_object_mut(), p.as_object()) {
        for (k, v) in src {
            if !o.contains_key(k) {
                return Err(format!("unknown preference `{k}`"));
            }
            o.insert(k.clone(), v.clone());
        }
    }
    let new: Prefs = serde_json::from_value(cur).map_err(|e| e.to_string())?;
    if !UNITS.contains(&new.default_units.as_str()) {
        return Err(format!("units must be one of {}", UNITS.join(", ")));
    }
    if !NAV_PRESETS.iter().any(|(id, _)| *id == new.nav) || !MSAA.contains(&new.msaa) {
        return Err("`nav` must be fusion, solidworks or inventor and `msaa` 1, 2, 4 or 8".into());
    }
    let msaa_changed = new.msaa != app.preferences.msaa;
    app.preferences = new;
    if msaa_changed {
        save_graphics(app);
    }
    serde_json::to_value(&app.preferences).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_map_the_mouse_and_prefs_round_trip() {
        let mut app = SolveApp::new(solvecraft_engine::Session::default(), Default::default());
        let shift = Modifiers { shift: true, ..Default::default() };
        let ctrl = Modifiers { ctrl: true, ..Default::default() };
        let none = Modifiers::default();
        assert!(matches!(nav_mode(&app, true, false, none), Some(NavMode::Pan)));
        assert!(matches!(nav_mode(&app, true, false, shift), Some(NavMode::Orbit)));
        set(&mut app, &json!({"nav": "solidworks"})).unwrap();
        assert!(matches!(nav_mode(&app, true, false, none), Some(NavMode::Orbit)));
        assert!(matches!(nav_mode(&app, true, false, ctrl), Some(NavMode::Pan)));
        set(&mut app, &json!({"nav": "inventor", "zoom_reverse": true})).unwrap();
        assert!(matches!(nav_mode(&app, true, false, ctrl), Some(NavMode::Zoom)));
        assert_eq!(zoom_sign(&app), -1.0);
        assert!(matches!(nav_mode(&app, false, true, none), Some(NavMode::Orbit)), "right drag orbits everywhere");
        assert!(set(&mut app, &json!({"nav": "blender"})).is_err());
        assert!(set(&mut app, &json!({"default_units": "furlong"})).is_err());
        assert!(set(&mut app, &json!({"nope": 1})).is_err());
        // Kept between runs.
        let mut other = SolveApp::new(solvecraft_engine::Session::default(), Default::default());
        other.load_prefs(&app.prefs());
        assert_eq!(other.preferences, app.preferences);
        assert_eq!(format_length(25.4, "in", 3), "1.000 in");
    }

    #[test]
    fn orbiting_around_the_cursor_keeps_that_point_still() {
        let mut app = SolveApp::new(solvecraft_engine::Session::default(), Default::default());
        app.run("PrimitiveBox", json!({"length": 40, "width": 40, "height": 40})).unwrap();
        app.preferences.orbit_cursor = true;
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        app.fit_view();
        let at = egui::pos2(500.0, 250.0);
        let pivot = pivot_under(&app, &crate::viewport::projection(&app, rect), at);
        orbit(&mut app, rect, 40.0, 25.0, Some(at));
        let s = crate::viewport::projection(&app, rect).to_screen(pivot).unwrap();
        assert!(s.distance(at) < 2.0, "{s:?}");
    }
}
