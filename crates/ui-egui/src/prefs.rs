//! Preferences, one dialog in the spirit of Fusion's, opened from the gear at the top right of the
//! window, File › Preferences or Ctrl+, (Cmd+, on macOS): General (theme, UI scale, units of new
//! designs, autosave, display precision, the agent cursor), Navigation (mouse presets, zoom
//! direction, zoom to the cursor, orbit centre, Look At animation), Keyboard Shortcuts (search,
//! rebind, conflicts; `keymap`), Design (default modelling orientation, Y up or Z up), Graphics
//! (visual style, grid, shadow, perspective, anti-aliasing), Sketch and Selection. Changes apply
//! at once; Cancel puts back what was there when the dialog opened, OK keeps them, Apply keeps
//! them and stays open, and Restore Defaults resets the page shown. Everything is kept between
//! runs, except anti-aliasing, which needs a restart.

use egui::{Modifiers, Pos2, RichText};
use serde::{Deserialize, Serialize};
use serde_json::json;
use solvecraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::theme::Tokens;
use crate::viewport::NavMode;

pub const UNITS: [&str; 5] = ["mm", "cm", "m", "in", "ft"];
/// Mouse mappings: (id, label).
pub const NAV_PRESETS: [(&str, &str); 5] =
    [("fusion", "Fusion"), ("solidworks", "SolidWorks"), ("inventor", "Inventor"), ("alias", "Alias"), ("tinkercad", "Tinkercad")];
/// The dialog's pages, in order.
pub const PAGES: [&str; 7] = ["General", "Navigation", "Keyboard Shortcuts", "Design", "Graphics", "Sketch", "Selection"];
pub const PAGE_SHORTCUTS: usize = 2;
pub const PAGE_DESIGN: usize = 3;
pub const THEMES: [(&str, &str); 3] = [("dark", "Dark"), ("light", "Light"), ("system", "System")];
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
    /// `dark`, `light` or `system` (follow the operating system).
    pub theme: String,
    /// Interface scale (1 = 100 %).
    pub ui_scale: f32,
    /// The wheel zooms toward the point under the cursor (else toward the view's centre).
    pub zoom_to_cursor: bool,
    /// View changes (view cube, Look At, Home) animate; `look_at_speed` scales how fast.
    pub look_at_anim: bool,
    pub look_at_speed: f32,
    /// Default modelling orientation: `z` (Z up) or `y` (Y up) (#3).
    pub up_axis: String,
    /// After a STEP or IGES file opens, offer to stand a Y-up part up (`stand_up`).
    pub offer_stand_up: bool,
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
            theme: "dark".into(),
            ui_scale: 1.0,
            zoom_to_cursor: true,
            look_at_anim: true,
            look_at_speed: 1.0,
            up_axis: "z".into(),
            offer_stand_up: true,
        }
    }
}

/// The preferences window.
#[derive(Default)]
pub struct PrefsWindow {
    pub open: bool,
    /// The page shown (index into [`PAGES`]).
    pub section: usize,
    /// Everything as it was when the dialog opened (or at the last Apply), for Cancel.
    snapshot: Option<Snapshot>,
}

/// Open the dialog at a page.
pub fn open_at(app: &mut SolveApp, page: usize) {
    app.prefs_window.open = true;
    app.prefs_window.section = page.min(PAGES.len() - 1);
}

/// Every setting the dialog changes, for Cancel and Restore Defaults.
#[derive(Clone)]
struct Snapshot {
    prefs: Prefs,
    dark: bool,
    grid: bool,
    shadow: bool,
    perspective: bool,
    style: u8,
    pick_bodies: bool,
    agent: crate::agent_cursor::Settings,
    autosave: f64,
    auto_project: bool,
    keys: std::collections::BTreeMap<String, String>,
}

impl Snapshot {
    fn take(app: &SolveApp) -> Snapshot {
        Snapshot {
            prefs: app.preferences.clone(),
            dark: app.ui.dark,
            grid: app.ui.show_grid,
            shadow: app.ui.ground_shadow,
            perspective: app.ui.perspective,
            style: app.ui.visual_style,
            pick_bodies: app.ui.pick_bodies,
            agent: app.ui.agent_cursor,
            autosave: app.autosave_minutes,
            auto_project: app.session.auto_project,
            keys: app.keymap.custom.clone(),
        }
    }
    fn restore(self, app: &mut SolveApp) {
        let msaa = app.preferences.msaa;
        app.preferences = self.prefs;
        app.ui.dark = self.dark;
        app.ui.show_grid = self.grid;
        app.ui.ground_shadow = self.shadow;
        app.ui.perspective = self.perspective;
        app.ui.visual_style = self.style;
        app.ui.pick_bodies = self.pick_bodies;
        app.ui.agent_cursor = self.agent;
        app.autosave_minutes = self.autosave;
        set_autosave(app);
        app.session.auto_project = self.auto_project;
        app.keymap.custom = self.keys;
        if app.preferences.msaa != msaa {
            save_graphics(app);
        }
    }
}

/// Restore Defaults for one page.
fn defaults(app: &mut SolveApp, page: usize) {
    let d = Prefs::default();
    let p = &mut app.preferences;
    match page {
        0 => {
            p.theme = d.theme;
            p.ui_scale = d.ui_scale;
            p.default_units = d.default_units;
            p.length_decimals = d.length_decimals;
            p.angle_decimals = d.angle_decimals;
            app.ui.dark = true;
            app.ui.agent_cursor = Default::default();
            app.autosave_minutes = 5.0;
            set_autosave(app);
        }
        1 => {
            p.nav = d.nav;
            p.zoom_reverse = d.zoom_reverse;
            p.zoom_to_cursor = d.zoom_to_cursor;
            p.orbit_cursor = d.orbit_cursor;
            p.look_at_anim = d.look_at_anim;
            p.look_at_speed = d.look_at_speed;
        }
        PAGE_SHORTCUTS => crate::keymap::reset(app, None),
        PAGE_DESIGN => {
            p.up_axis = d.up_axis;
            p.offer_stand_up = d.offer_stand_up;
        }
        4 => {
            let msaa = p.msaa != d.msaa;
            p.msaa = d.msaa;
            app.ui.visual_style = 0;
            app.ui.show_grid = true;
            app.ui.ground_shadow = true;
            app.ui.perspective = false;
            if msaa {
                save_graphics(app);
            }
        }
        5 => app.session.auto_project = true,
        _ => app.ui.pick_bodies = false,
    }
}

/// Settings that follow the preferences every frame: the theme when it follows the system, and
/// the interface scale.
pub fn apply(app: &mut SolveApp, ctx: &egui::Context) {
    if app.preferences.theme == "system"
        && let Some(t) = ctx.system_theme()
    {
        app.ui.dark = t == egui::Theme::Dark;
    }
    let z = app.preferences.ui_scale.clamp(0.5, 2.5);
    if z.is_finite() && (ctx.zoom_factor() - z).abs() > 1e-3 {
        ctx.set_zoom_factor(z);
    }
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
        // Tinkercad: Shift+right pans; everywhere else right drag orbits.
        return Some(if app.preferences.nav == "tinkercad" && mods.shift { NavMode::Pan } else { NavMode::Orbit });
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
        // Alias: middle pans, Ctrl+middle zooms (right drag orbits).
        "alias" => Some(if ctrl { NavMode::Zoom } else { NavMode::Pan }),
        // Tinkercad: middle pans.
        "tinkercad" => Some(NavMode::Pan),
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
    // File › Keyboard Shortcuts and Help › Keyboard Shortcuts open this dialog at that page.
    if app.keymap.open {
        app.keymap.open = false;
        open_at(app, PAGE_SHORTCUTS);
    }
    if !app.prefs_window.open {
        app.prefs_window.snapshot = None;
        return;
    }
    if app.prefs_window.snapshot.is_none() {
        app.prefs_window.snapshot = Some(Snapshot::take(app));
    }
    let t = Tokens::get();
    let mut open = true;
    let mut button: Option<&str> = None;
    let before = app.preferences.clone();
    crate::frame::window(ctx, "Preferences", crate::frame::Width::Wide)
        .id(egui::Id::new("sc_prefs"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            // A fixed page area: the vertical separator fills the height it is given, so an
            // auto-sized one would grow every frame.
            ui.allocate_ui_with_layout(egui::vec2(700.0, 400.0), egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.set_min_size(egui::vec2(700.0, 400.0));
                ui.vertical(|ui| {
                    ui.set_width(140.0);
                    for (i, s) in PAGES.iter().enumerate() {
                        if ui.selectable_label(app.prefs_window.section == i, *s).clicked() {
                            app.prefs_window.section = i;
                            app.keymap.read_only = false;
                        }
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    // A fixed width, so the window keeps its size from page to page.
                    ui.set_width(540.0);
                    ui.label(RichText::new(PAGES.get(app.prefs_window.section).copied().unwrap_or_default()).strong().size(14.0));
                    ui.add_space(6.0);
                    page(app, ui, &t);
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Restore Defaults").on_hover_text("Reset the settings on this page").clicked() {
                    button = Some("defaults");
                }
                crate::frame::right_aligned(ui, 700.0, 170.0, |ui| {
                    if ui.button("OK").clicked() {
                        button = Some("ok");
                    }
                    if ui.button("Cancel").clicked() {
                        button = Some("cancel");
                    }
                    if ui.button("Apply").clicked() {
                        button = Some("apply");
                    }
                });
            });
        });
    if app.preferences.msaa != before.msaa {
        save_graphics(app);
    }
    if app.preferences.up_axis != before.up_axis {
        up_axis_changed(app);
    }
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !crate::keymap::capturing(app);
    match button {
        Some("defaults") => defaults(app, app.prefs_window.section),
        Some("apply") => app.prefs_window.snapshot = Some(Snapshot::take(app)),
        Some("ok") => close(app, false),
        Some("cancel") => close(app, true),
        _ if !open || esc => close(app, !open || esc),
        _ => {}
    }
}

/// Close the dialog; `revert`: put back what was there when it opened (Cancel, Esc, ×).
fn close(app: &mut SolveApp, revert: bool) {
    if let Some(s) = app.prefs_window.snapshot.take()
        && revert
    {
        let up = app.preferences.up_axis.clone();
        s.restore(app);
        if app.preferences.up_axis != up {
            up_axis_changed(app);
        }
    }
    app.prefs_window.open = false;
    app.keymap.read_only = false;
    crate::keymap::stop_capture(app);
}

/// The default orientation changed: the view turns to match.
fn up_axis_changed(app: &mut SolveApp) {
    app.animate_view("home");
}

/// One page of the dialog.
fn page(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    let note = |ui: &mut egui::Ui, s: &str| {
        ui.label("");
        ui.label(RichText::new(s).size(11.0).color(t.text_dim));
        ui.end_row();
    };
    if app.prefs_window.section == PAGE_SHORTCUTS {
        crate::keymap::page(app, ui);
        return;
    }
    egui::Grid::new(("sc_prefs_grid", app.prefs_window.section)).num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| match app.prefs_window.section {
        0 => {
            ui.label("Theme");
            ui.horizontal(|ui| {
                for (id, label) in THEMES {
                    if ui.radio(app.preferences.theme == id, label).clicked() {
                        app.preferences.theme = id.to_string();
                        if id != "system" {
                            app.ui.dark = id == "dark";
                        }
                    }
                }
            });
            ui.end_row();
            ui.label("UI scale");
            ui.add(egui::Slider::new(&mut app.preferences.ui_scale, 0.75..=2.0).step_by(0.05).custom_formatter(|v, _| format!("{:.0} %", v * 100.0)));
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
            note(ui, "0 turns it off");
            ui.label("Length decimals");
            ui.add(egui::DragValue::new(&mut app.preferences.length_decimals).range(0..=6));
            ui.end_row();
            ui.label("Angle decimals");
            ui.add(egui::DragValue::new(&mut app.preferences.angle_decimals).range(0..=6));
            ui.end_row();
            ui.label("Agent cursor");
            let s = &mut app.ui.agent_cursor;
            ui.checkbox(&mut s.show, "Show the pointer an AI agent moves (#35)");
            ui.end_row();
            ui.label("Agent cursor speed");
            ui.add_enabled_ui(s.show, |ui| {
                crate::agent_cursor::speed_slider(ui, &mut s.speed);
            });
            ui.end_row();
            ui.label("");
            ui.add_enabled(s.show, egui::Checkbox::new(&mut s.follow_camera, "Follow the camera (Look At sketches)"));
            ui.end_row();
        }
        1 => {
            ui.label("Pan, zoom, orbit");
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
            note(ui, &format!("{}; the wheel zooms", nav_help(&app.preferences.nav)));
            ui.label("Reverse zoom direction");
            ui.checkbox(&mut app.preferences.zoom_reverse, "");
            ui.end_row();
            ui.label("Zoom to the cursor");
            ui.checkbox(&mut app.preferences.zoom_to_cursor, "")
                .on_hover_text("The wheel zooms toward the point under the cursor (off: toward the view's centre)");
            ui.end_row();
            ui.label("Orbit around");
            ui.horizontal(|ui| {
                ui.radio_value(&mut app.preferences.orbit_cursor, false, "View centre");
                ui.radio_value(&mut app.preferences.orbit_cursor, true, "Point under the cursor");
            });
            ui.end_row();
            ui.label("Animate view changes");
            ui.checkbox(&mut app.preferences.look_at_anim, "View cube, Look At, Home and Fit");
            ui.end_row();
            ui.label("Animation speed");
            ui.add_enabled(
                app.preferences.look_at_anim,
                egui::Slider::new(&mut app.preferences.look_at_speed, 0.5..=3.0).step_by(0.25).custom_formatter(|v, _| format!("{v:.2}×")),
            );
            ui.end_row();
        }
        PAGE_DESIGN => {
            ui.label("Default modelling orientation");
            ui.horizontal(|ui| {
                ui.radio_value(&mut app.preferences.up_axis, "z".to_string(), "Z up");
                ui.radio_value(&mut app.preferences.up_axis, "y".to_string(), "Y up");
            });
            ui.end_row();
            note(ui, "Which axis points up in the views (view cube, Home, Look At). Designs are unchanged.");
            ui.label("Offer to stand up Y-up files");
            ui.checkbox(&mut app.preferences.offer_stand_up, "")
                .on_hover_text("After a STEP or IGES file opens with Z up, offer a quarter turn about X");
            ui.end_row();
        }
        4 => {
            ui.label("Visual style");
            let styles = ["Shaded with edges", "Shaded", "Wireframe", "Shaded with hidden edges"];
            egui::ComboBox::from_id_salt("pref_style").selected_text(styles[usize::from(app.ui.visual_style.min(3))]).show_ui(ui, |ui| {
                for (i, s) in styles.iter().enumerate() {
                    ui.selectable_value(&mut app.ui.visual_style, i as u8, *s);
                }
            });
            ui.end_row();
            ui.label("Grid");
            ui.checkbox(&mut app.ui.show_grid, "");
            ui.end_row();
            ui.label("Ground shadow");
            ui.checkbox(&mut app.ui.ground_shadow, "");
            ui.end_row();
            ui.label("Perspective view");
            ui.checkbox(&mut app.ui.perspective, "");
            ui.end_row();
            ui.label("Anti-aliasing");
            egui::ComboBox::from_id_salt("pref_msaa").selected_text(format!("{}×", app.preferences.msaa)).show_ui(ui, |ui| {
                for m in MSAA {
                    ui.selectable_value(&mut app.preferences.msaa, m, format!("{m}×"));
                }
            });
            ui.end_row();
            note(ui, "Anti-aliasing applies the next time SolveCraft starts");
            ui.label("Renderer");
            ui.label(match (&app.viewport.gpu, &app.help.gpu) {
                (Some(_), Some(g)) => format!("GPU: {g}"),
                (Some(_), None) => "GPU".to_string(),
                _ => "CPU (software)".to_string(),
            });
            ui.end_row();
        }
        5 => {
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
}

/// What the mouse does under a preset.
pub fn nav_help(preset: &str) -> &'static str {
    match preset {
        "solidworks" => "Middle drag orbits, Ctrl+middle pans, Shift+middle zooms, right drag orbits",
        "inventor" => "Middle drag pans, Shift+middle orbits, Ctrl+middle zooms, right drag orbits",
        "alias" => "Alt+right or right drag orbits, middle drag pans, Ctrl+middle zooms",
        "tinkercad" => "Right drag orbits, middle or Shift+right drag pans",
        _ => "Middle drag pans, Shift+middle orbits, right drag orbits",
    }
}

/// The control channel's `ui.prefs`: `open` (a page name) opens the dialog there, `button`
/// presses OK, Cancel, Apply or Restore Defaults, `stand_up: true` answers the stand-up offer;
/// any other keys set preferences. Returns them all, the open page and the offer.
pub fn control(app: &mut SolveApp, p: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut rest = p.as_object().cloned().unwrap_or_default();
    if let Some(page) = rest.remove("open") {
        let name = page.as_str().unwrap_or("General");
        let i = PAGES.iter().position(|x| x.eq_ignore_ascii_case(name)).ok_or_else(|| format!("no page `{name}` ({})", PAGES.join(", ")))?;
        open_at(app, i);
        if app.prefs_window.snapshot.is_none() {
            app.prefs_window.snapshot = Some(Snapshot::take(app));
        }
    }
    if rest.remove("stand_up").and_then(|v| v.as_bool()) == Some(true) {
        crate::stand_up::stand_up(app)?;
    }
    let button = rest.remove("button");
    let mut out = set(app, &serde_json::Value::Object(rest))?;
    match button.as_ref().and_then(|b| b.as_str()) {
        Some("ok") => close(app, false),
        Some("cancel") => close(app, true),
        Some("apply") => app.prefs_window.snapshot = Some(Snapshot::take(app)),
        Some("defaults") => defaults(app, app.prefs_window.section),
        Some(b) => return Err(format!("unknown button `{b}` (ok, cancel, apply, defaults)")),
        None => {}
    }
    if button.is_some() {
        out = serde_json::to_value(&app.preferences).map_err(|e| e.to_string())?;
    }
    if let Some(o) = out.as_object_mut() {
        o.insert("dialog".into(), json!(app.prefs_window.open.then(|| PAGES.get(app.prefs_window.section).copied().unwrap_or_default())));
        o.insert("stand_up_offer".into(), json!(app.stand_up));
        o.insert("dark".into(), json!(app.ui.dark));
    }
    Ok(out)
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
        return Err("`nav` must be fusion, solidworks, inventor, alias or tinkercad and `msaa` 1, 2, 4 or 8".into());
    }
    if !THEMES.iter().any(|(id, _)| *id == new.theme) || !["z", "y"].contains(&new.up_axis.as_str()) {
        return Err("`theme` must be dark, light or system and `up_axis` z or y".into());
    }
    if !(0.5..=2.5).contains(&new.ui_scale) || !(0.25..=4.0).contains(&new.look_at_speed) {
        return Err("`ui_scale` must be 0.5…2.5 and `look_at_speed` 0.25…4".into());
    }
    if new.theme != app.preferences.theme && new.theme != "system" {
        app.ui.dark = new.theme == "dark";
    }
    let up_changed = new.up_axis != app.preferences.up_axis;
    let msaa_changed = new.msaa != app.preferences.msaa;
    app.preferences = new;
    if msaa_changed {
        save_graphics(app);
    }
    if up_changed {
        up_axis_changed(app);
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

    /// The dialog: changes apply at once, Cancel puts back what was there when it opened, Apply
    /// keeps them, Restore Defaults resets one page, and the shortcuts menus open its page.
    #[test]
    fn preferences_dialog_cancel_apply_and_defaults() {
        let mut app = SolveApp::new(solvecraft_engine::Session::default(), Default::default());
        control(&mut app, &json!({"open": "general"})).unwrap();
        assert!(app.prefs_window.open && app.prefs_window.section == 0);
        control(&mut app, &json!({"theme": "light", "ui_scale": 1.25, "zoom_to_cursor": false})).unwrap();
        assert!(!app.ui.dark, "light applies at once");
        app.keymap.custom.insert("solid.extrude".into(), "Shift+E".into());
        let v = control(&mut app, &json!({"button": "cancel"})).unwrap();
        assert_eq!(v["dialog"], serde_json::Value::Null);
        assert!(app.ui.dark && app.preferences.theme == "dark" && app.preferences.ui_scale == 1.0 && app.preferences.zoom_to_cursor);
        assert!(app.keymap.custom.is_empty(), "a rebinding made while the dialog was open is undone too");

        control(&mut app, &json!({"open": "Navigation", "nav": "alias", "look_at_anim": false})).unwrap();
        control(&mut app, &json!({"button": "apply"})).unwrap();
        control(&mut app, &json!({"nav": "tinkercad"})).unwrap();
        control(&mut app, &json!({"button": "cancel"})).unwrap();
        assert_eq!(app.preferences.nav, "alias", "Cancel goes back to the last Apply");
        let to = {
            let mut c = app.cam;
            c.set_view(solvecraft_engine::render::StandardView::Top);
            c
        };
        app.animate_to(to);
        assert!(app.cam_anim.is_none() && app.cam == to, "no animation: the view jumps");

        control(&mut app, &json!({"open": "Navigation"})).unwrap();
        control(&mut app, &json!({"button": "defaults"})).unwrap();
        assert_eq!(app.preferences.nav, "fusion");
        assert!(app.preferences.look_at_anim);
        control(&mut app, &json!({"button": "ok"})).unwrap();
        assert!(!app.prefs_window.open);

        // File and Help › Keyboard Shortcuts open the same dialog at its page.
        app.keymap.open = true;
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| show(&mut app, ui.ctx()));
        assert!(app.prefs_window.open && app.prefs_window.section == PAGE_SHORTCUTS && !app.keymap.open);
        assert!(control(&mut app, &json!({"open": "Plugins"})).is_err());
        assert!(set(&mut app, &json!({"up_axis": "x"})).is_err());
        assert!(set(&mut app, &json!({"theme": "sepia"})).is_err());
    }

    /// A STEP file opened Z-up is offered to be stood up: a quarter turn about X, undoable.
    #[test]
    fn a_y_up_step_file_can_be_stood_up() {
        let dir = std::env::temp_dir().join(format!("solvecraft-standup-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("tall_in_y.step").to_string_lossy().to_string();
        let mut src = SolveApp::new(solvecraft_engine::Session::default(), Default::default());
        src.run("solid.box", json!({"length": 10, "width": 40, "height": 5})).unwrap();
        src.run("file.export", json!({"path": path})).unwrap();

        let mut app = SolveApp::new(solvecraft_engine::Session::default(), Default::default());
        app.open_path(&path);
        let offered = app.stand_up.clone().unwrap_or_default();
        assert_eq!(offered.len(), 1, "the file's body is offered");
        let size = |app: &SolveApp| solvecraft_engine::view::bounds(&app.session);
        assert!((size(&app).max.y - size(&app).min.y - 40.0).abs() < 1e-6);
        control(&mut app, &json!({"stand_up": true})).unwrap();
        let b = size(&app);
        assert!((b.max.z - b.min.z - 40.0).abs() < 1e-6, "Y became Z: {b:?}");
        assert!(app.stand_up.is_none());
        app.run("edit.undo", json!({})).unwrap();
        assert!((size(&app).max.y - size(&app).min.y - 40.0).abs() < 1e-6, "one undo step");

        // Not offered when turned off, or with Y up as the default orientation.
        let mut app = SolveApp::new(solvecraft_engine::Session::default(), Default::default());
        app.preferences.offer_stand_up = false;
        app.open_path(&path);
        assert!(app.stand_up.is_none());
        let mut app = SolveApp::new(solvecraft_engine::Session::default(), Default::default());
        app.preferences.up_axis = "y".into();
        app.open_path(&path);
        assert!(app.stand_up.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn orbiting_around_the_cursor_keeps_that_point_still() {
        let mut app = SolveApp::new(solvecraft_engine::Session::default(), Default::default());
        app.run("solid.box", json!({"length": 40, "width": 40, "height": 40})).unwrap();
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
