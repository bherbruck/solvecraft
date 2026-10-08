//! Application bar, the workspace toolbar (tabs and panels) and keyboard shortcuts.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;
use solvecraft_engine::command_specs;

use crate::theme::Tokens;
use crate::{SolveApp, icons};

pub const TABS: &[&str] = &["SOLID", "SURFACE", "MESH", "SHEET METAL", "PLASTIC", "UTILITIES"];

/// Panels shown on each tab (in order). Commands come from the registry by (tab, panel).
fn panels(tab: &str) -> &'static [&'static str] {
    if let Some(p) = crate::workspace::panels(tab) {
        return p;
    }
    match tab {
        "SOLID" => &["CREATE", "MODIFY", "ASSEMBLE", "CONSTRUCT", "INSPECT", "INSERT", "SELECT"],
        "SKETCH" => &["CREATE", "MODIFY", "CONSTRAINTS", "INSPECT", "INSERT", "SELECT", "FINISH SKETCH"],
        "SURFACE" => &["CREATE", "MODIFY", "ASSEMBLE", "CONSTRUCT", "INSPECT", "INSERT", "SELECT"],
        "MESH" => &["CREATE", "PREPARE", "MODIFY", "ASSEMBLE", "CONSTRUCT", "INSPECT", "INSERT", "SELECT"],
        "SHEET METAL" => &["CREATE", "MODIFY", "ASSEMBLE", "CONSTRUCT", "INSPECT", "INSERT", "SELECT"],
        "PLASTIC" => &["CREATE", "MODIFY", "ASSEMBLE", "CONSTRUCT", "INSPECT", "INSERT", "SELECT"],
        _ => &["MAKE", "ADD-INS", "UTILITY", "INSPECT", "SELECT"],
    }
}

/// How many commands of a panel get a big button.
fn promoted(tab: &str, panel: &str) -> usize {
    match (tab, panel) {
        ("SOLID", "CREATE") => 4,
        ("SOLID", "MODIFY") => 4,
        ("SKETCH", "CREATE") => 6,
        ("SKETCH", "CONSTRAINTS") => 6,
        (_, "FINISH SKETCH") => 1,
        _ => 2,
    }
}

pub fn app_bar(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    egui::Panel::top("sc_appbar").exact_size(34.0).frame(egui::Frame::NONE.fill(t.app_bar)).show(ui, |ui| {
        let r = ui.max_rect();
        // The bar is the window's title bar: its empty space moves the window.
        if app.custom_titlebar || app.integrated_titlebar {
            crate::titlebar::drag_area(ui, r);
        }
        let left = r.left() + if app.integrated_titlebar { crate::titlebar::TRAFFIC_LIGHTS } else { 0.0 };
        // Brand mark: a small solid block drawn in code.
        let logo = Rect::from_center_size(pos2(left + 20.0, r.center().y), vec2(20.0, 20.0));
        icons::paint(ui.painter(), logo, "box", Color32::from_rgb(220, 226, 236), Color32::from_rgb(90, 160, 240), Color32::WHITE);
        ui.painter().text(pos2(left + 36.0, r.center().y), Align2::LEFT_CENTER, "SolveCraft", FontId::proportional(14.0), t.app_bar_text);
        let mut x = left + 128.0;
        let mut click = |ui: &mut egui::Ui, icon: &str, tip: &str| -> bool {
            let br = Rect::from_center_size(pos2(x, r.center().y), vec2(26.0, 26.0));
            x += 30.0;
            let resp = ui.interact(br, ui.id().with(("ab", icon)), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(br, 4.0, Color32::from_white_alpha(30));
            }
            icons::paint(
                ui.painter(),
                br.shrink(5.0),
                icon,
                Color32::from_rgb(225, 230, 238),
                Color32::from_rgb(120, 140, 170),
                Color32::from_rgb(120, 190, 255),
            );
            resp.on_hover_text(tip).clicked()
        };
        let file = click(ui, "folder", "File");
        if click(ui, "save", "Save (Ctrl+S)") {
            save(app);
        }
        if click(ui, "undo", "Undo (Ctrl+Z)") {
            let _ = app.run("UndoCommand", json!({}));
        }
        if click(ui, "redo", "Redo (Ctrl+Y)") {
            let _ = app.run("RedoCommand", json!({}));
        }
        if click(ui, "home", "Start page") {
            let rev = app.session.revision;
            app.home.show(rev);
        }
        let theme_tip = if app.ui.dark { "Light theme" } else { "Dark theme" };
        if click(ui, if app.ui.dark { "sun" } else { "moon" }, theme_tip) {
            app.ui.dark = !app.ui.dark;
        }
        if click(ui, "help", "Help") {
            app.help.menu = !app.help.menu;
            let at = ui.ctx().input(|i| i.pointer.latest_pos()).map_or(300.0, |p| p.x);
            ui.ctx().data_mut(|d| {
                d.insert_temp(egui::Id::new("sc_help_opened"), true);
                d.insert_temp(egui::Id::new("sc_help_anchor"), at);
            });
        }
        if file {
            app.ui.palette_open = false;
            let ctx = ui.ctx().clone();
            let open = ctx.data(|d| d.get_temp::<bool>(egui::Id::new("sc_file_menu")).unwrap_or(false));
            let pass = ctx.cumulative_pass_nr();
            // The folder button toggles the menu. Remember the pass it opened on so the same
            // click isn't also treated as a click outside the menu (which would close it at once).
            ctx.data_mut(|d| {
                d.insert_temp(egui::Id::new("sc_file_menu"), !open);
                d.insert_temp(egui::Id::new("sc_file_menu_opened"), pass);
            });
        }
        // Document tabs.
        let captions = if app.custom_titlebar { crate::titlebar::WIDTH } else { 0.0 };
        crate::documents::tabs(app, ui, r, x + 16.0, r.right() - captions - 60.0);
        ui.painter().text(
            pos2(r.right() - 12.0 - captions, r.center().y),
            Align2::RIGHT_CENTER,
            format!("v{}", env!("CARGO_PKG_VERSION")),
            FontId::proportional(11.0),
            Color32::from_rgb(160, 168, 182),
        );
        if app.custom_titlebar {
            crate::titlebar::caption_buttons(app, ui, r);
        }
    });
    file_menu(app, ui.ctx());
    let help_x = ui.ctx().data(|d| d.get_temp::<f32>(egui::Id::new("sc_help_anchor")).unwrap_or(300.0));
    crate::help::show(app, &ui.ctx().clone(), pos2(help_x - 14.0, 34.0));
}

fn file_menu(app: &mut SolveApp, ctx: &egui::Context) {
    let id = egui::Id::new("sc_file_menu");
    if !ctx.data(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
        return;
    }
    let mut close = false;
    let resp = egui::Area::new(egui::Id::new("sc_file_area"))
        .fixed_pos(pos2(100.0 + if app.integrated_titlebar { crate::titlebar::TRAFFIC_LIGHTS } else { 0.0 }, 34.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(190.0);
                let item = |ui: &mut egui::Ui, label: &str, key: &str| {
                    ui.add(egui::Button::new(label).shortcut_text(key).frame(false).min_size(vec2(180.0, 22.0))).clicked()
                };
                if item(ui, "New Design", "Ctrl+N") {
                    crate::documents::new_design(app);
                    close = true;
                }
                if item(ui, "Open…", "Ctrl+O") {
                    if let Some(p) = app.services.pick_open.as_ref().and_then(|f| f()) {
                        app.open_path(&p);
                    }
                    close = true;
                }
                if item(ui, "Insert STEP…", "") {
                    app.start("FusionImportCommandFromToolbar");
                    close = true;
                }
                if item(ui, "Insert Mesh…", "") {
                    app.start("ParaMeshInsertAlignCommand");
                    close = true;
                }
                if item(ui, "Save", "Ctrl+S") {
                    save(app);
                    close = true;
                }
                if item(ui, "Save As…", "") {
                    save_as(app);
                    close = true;
                }
                ui.separator();
                if item(ui, "Export STL…", "") {
                    export(app, "stl");
                    close = true;
                }
                if item(ui, "Export STEP…", "") {
                    export(app, "step");
                    close = true;
                }
                if item(ui, "Export 3MF…", "") {
                    export(app, "3mf");
                    close = true;
                }
                if item(ui, "Export OBJ…", "") {
                    export(app, "obj");
                    close = true;
                }
                ui.separator();
                if item(ui, "Preferences…", "") {
                    app.prefs_window.open = true;
                    close = true;
                }
                if item(ui, "Keyboard Shortcuts…", "") {
                    app.keymap.open = true;
                    app.keymap.read_only = false;
                    close = true;
                }
                if item(ui, "Quit", "") {
                    app.quit_requested = true;
                    close = true;
                }
            });
        });
    let just_opened = ctx.data(|d| d.get_temp::<u64>(egui::Id::new("sc_file_menu_opened"))) == Some(ctx.cumulative_pass_nr());
    if close || (!just_opened && resp.response.clicked_elsewhere()) {
        ctx.data_mut(|d| d.insert_temp(id, false));
    }
}

pub fn save(app: &mut SolveApp) {
    if crate::browser::needs_capture(app, crate::browser::SAVE) {
        app.tree.capture_prompt = Some(crate::browser::SAVE.into());
        return;
    }
    if app.session.path.is_some() {
        let _ = app.run("SaveDocumentCommand", json!({}));
    } else {
        save_as(app);
    }
}

fn save_as(app: &mut SolveApp) {
    let name = format!("{}.{}", app.session.doc.name, solvecraft_engine::io::DESIGN_EXT);
    if let Some(p) = app.services.pick_save.as_ref().and_then(|f| f(&name, &[solvecraft_engine::io::DESIGN_EXT])) {
        let _ = app.run("SaveDocumentAsCommand", json!({"path": p}));
    }
}

fn export(app: &mut SolveApp, ext: &str) {
    let name = format!("{}.{ext}", app.session.doc.name);
    if let Some(p) = app.services.pick_save.as_ref().and_then(|f| f(&name, &[ext]))
        && app.run("ExportCommand", json!({"path": p})).is_ok()
    {
        app.set_status(format!("Exported {p}"), false);
    }
}

pub fn toolbar(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let sketching = app.session.active_sketch.is_some();
    egui::Panel::top("sc_toolbar").exact_size(92.0).frame(egui::Frame::NONE.fill(t.toolbar)).show(ui, |ui| {
        let r = ui.max_rect();
        let painter = ui.painter().clone();
        // Workspace selector and tabs.
        let tabs_y = r.top() + 2.0;
        let ws = Rect::from_min_size(pos2(r.left() + 8.0, tabs_y + 26.0), vec2(96.0, 56.0));
        painter.rect(ws, 4.0, t.field, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        painter.text(pos2(ws.center().x - 5.0, ws.top() + 14.0), Align2::CENTER_CENTER, "DESIGN", FontId::proportional(12.0), t.text);
        caret(&painter, pos2(ws.center().x + 26.0, ws.top() + 14.0), t.text);
        painter.text(pos2(ws.center().x, ws.top() + 34.0), Align2::CENTER_CENTER, "workspace", FontId::proportional(10.0), t.text_dim);
        let mut x = r.left() + 112.0;
        let mut tabs: Vec<&str> = Vec::new();
        if sketching {
            tabs.push("SKETCH");
        }
        tabs.extend(TABS.iter());
        for tab in tabs {
            let w = 12.0 + tab.len() as f32 * 7.4;
            let tr = Rect::from_min_size(pos2(x, tabs_y), vec2(w, 22.0));
            let active = app.ui.tab == tab;
            let resp = ui.interact(tr, ui.id().with(("tab", tab)), Sense::click());
            if active {
                painter.rect_filled(tr, egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, t.tab_active);
                let c = if tab == "SKETCH" { t.sketch_accent } else { t.accent };
                painter.line_segment([pos2(tr.left() + 4.0, tr.bottom() - 1.0), pos2(tr.right() - 4.0, tr.bottom() - 1.0)], Stroke::new(2.0, c));
            } else if resp.hovered() {
                painter.rect_filled(tr, 4.0, t.hover);
            }
            let col = if tab == "SKETCH" {
                t.sketch_accent
            } else if active {
                t.text
            } else {
                t.text_dim
            };
            painter.text(tr.center(), Align2::CENTER_CENTER, tab, FontId::proportional(12.0), col);
            if resp.clicked() {
                app.ui.tab = tab.to_string();
            }
            x += w + 4.0;
        }
        // Panels.
        let tab = app.ui.tab.clone();
        let specs = command_specs();
        let mut px = r.left() + 112.0;
        let top = r.top() + 26.0;
        for panel in panels(&tab) {
            let (cmds, promote) = match crate::workspace::layout(&tab, panel, &specs) {
                Some(l) => l,
                None => (specs.iter().filter(|c| c.tab == tab && c.panel == *panel).copied().collect(), promoted(&tab, panel)),
            };
            let cmds = crate::sketch_tools::also_in(&tab, panel, cmds, &specs);
            let n = promote.min(cmds.len()).max(if cmds.is_empty() { 1 } else { 0 });
            let width = (n as f32 * 40.0).max(64.0) + 8.0;
            let enabled_panel = !cmds.is_empty() || *panel == "SELECT";
            for (i, c) in cmds.iter().take(n).enumerate() {
                let br = Rect::from_min_size(pos2(px + 4.0 + i as f32 * 40.0, top + 2.0), vec2(38.0, 38.0));
                let info = c.info(&app.session);
                let resp = ui.interact(br, ui.id().with(("cmd", c.id)), Sense::click());
                if resp.hovered() && info.enabled {
                    painter.rect_filled(br, 4.0, t.hover);
                }
                let (ink, fill, acc) = if info.enabled {
                    (t.icon, t.icon_fill, if tab == "SKETCH" { t.sketch_accent } else { t.accent })
                } else {
                    (t.border, t.panel_header, t.border)
                };
                let icon = if c.id == "SketchStop" { "finish" } else { c.icon };
                icons::paint(&painter, br.shrink(6.0), icon, ink, fill, acc);
                let tip = match &info.disabled_reason {
                    Some(why) => format!("{}\n{why}", c.label),
                    None => format!("{}{}", c.label, c.shortcut.map(|k| format!("  ({k})")).unwrap_or_default()),
                };
                if resp.on_hover_text(tip).clicked() && info.enabled {
                    let id = c.id;
                    app.start(id);
                }
            }
            if cmds.is_empty() {
                let br = Rect::from_min_size(pos2(px + 4.0, top + 2.0), vec2(width - 8.0, 38.0));
                painter.text(br.center(), Align2::CENTER_CENTER, "—", FontId::proportional(14.0), t.border);
            }
            // Panel label with drop-down.
            let lr = Rect::from_min_size(pos2(px, top + 42.0), vec2(width, 18.0));
            let label = panel.to_string();
            let lresp = ui.interact(lr, ui.id().with(("panel", panel)), Sense::click());
            if lresp.hovered() && enabled_panel {
                painter.rect_filled(lr, 3.0, t.hover);
            }
            let lc = if *panel == "FINISH SKETCH" {
                t.sketch_accent
            } else if enabled_panel {
                t.text
            } else {
                t.text_dim
            };
            let g = painter.layout_no_wrap(label, FontId::proportional(10.5), lc);
            let gx = lr.center().x - (g.size().x + 9.0) / 2.0;
            let gy = lr.center().y - g.size().y / 2.0;
            let gw = g.size().x;
            painter.galley(pos2(gx, gy), g, lc);
            caret(&painter, pos2(gx + gw + 6.0, lr.center().y), lc);
            let popup_id = ui.id().with(("panel_menu", panel));
            if lresp.clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("sc_panel_open"), popup_id));
            }
            let open = ui.ctx().data(|d| d.get_temp::<egui::Id>(egui::Id::new("sc_panel_open"))) == Some(popup_id);
            if open {
                let area = egui::Area::new(popup_id).fixed_pos(lr.left_bottom()).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(200.0);
                        if *panel == "SELECT" {
                            selection_filter(app, ui);
                        } else if cmds.is_empty() {
                            ui.label(egui::RichText::new("Not available yet").color(t.text_dim));
                        }
                        for c in &cmds {
                            let info = c.info(&app.session);
                            ui.horizontal(|ui| {
                                let (ir, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                                icons::paint(ui.painter(), ir, c.icon, t.icon, t.icon_fill, t.accent);
                                let b = ui.add_enabled(info.enabled, egui::Button::new(c.label).frame(false).shortcut_text(c.shortcut.unwrap_or("")));
                                if b.clicked() {
                                    let id = c.id;
                                    app.start(id);
                                    ui.ctx().data_mut(|d| d.remove::<egui::Id>(egui::Id::new("sc_panel_open")));
                                }
                            });
                        }
                    });
                });
                if area.response.clicked_elsewhere() && !lresp.clicked() {
                    ui.ctx().data_mut(|d| d.remove::<egui::Id>(egui::Id::new("sc_panel_open")));
                }
            }
            px += width;
            painter.line_segment([pos2(px, top + 4.0), pos2(px, top + 58.0)], Stroke::new(1.0, t.border));
            px += 2.0;
        }
        painter.line_segment([pos2(r.left(), r.bottom() - 0.5), pos2(r.right(), r.bottom() - 0.5)], Stroke::new(1.0, t.border));
    });
}

/// The SELECT panel: what a click in the view picks.
fn selection_filter(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui.label(egui::RichText::new("Selection filter").color(t.text_dim).size(11.0));
    ui.checkbox(&mut app.ui.pick_bodies, "Select bodies (a face picks its body)");
    for (k, label) in [
        ("faces", "Faces"),
        ("edges", "Edges"),
        ("vertices", "Vertices"),
        ("sketch", "Sketch geometry and profiles"),
        ("sketch_curves", "Sketch curves"),
        ("sketch_points", "Sketch points"),
        ("dimensions", "Sketch dimensions"),
        ("constraints", "Sketch constraints"),
    ] {
        let mut on = !app.ui.pick_off.iter().any(|x| x == k);
        if ui.checkbox(&mut on, label).changed() {
            app.ui.pick_off.retain(|x| x != k);
            if !on {
                app.ui.pick_off.push(k.to_string());
            }
        }
    }
}

/// A small drop-down caret.
fn caret(p: &egui::Painter, c: egui::Pos2, col: Color32) {
    p.add(egui::Shape::convex_polygon(vec![pos2(c.x - 3.5, c.y - 2.0), pos2(c.x + 3.5, c.y - 2.0), pos2(c.x, c.y + 2.5)], col, Stroke::NONE));
}

/// Global keyboard shortcuts (when no text field has focus).
pub fn shortcuts(app: &mut SolveApp, ctx: &egui::Context) {
    // The Keyboard Shortcuts window is waiting for a key: it is not a shortcut.
    if crate::keymap::capturing(app) {
        return;
    }
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let (keys, mods) = ctx.input(|i| {
        let keys: Vec<egui::Key> =
            i.events.iter().filter_map(|e| if let egui::Event::Key { key, pressed: true, .. } = e { Some(*key) } else { None }).collect();
        (keys, i.modifiers)
    });
    for k in keys {
        use egui::Key;
        if mods.command {
            match k {
                Key::Z => drop(app.run("UndoCommand", json!({}))),
                Key::Y => drop(app.run("RedoCommand", json!({}))),
                Key::S => save(app),
                Key::N => crate::documents::new_design(app),
                Key::O => {
                    if let Some(p) = app.services.pick_open.as_ref().and_then(|f| f()) {
                        app.open_path(&p);
                    }
                }
                _ => {
                    if let Some(id) = crate::keymap::command_for(app, &crate::keymap::key_name(k, mods)) {
                        app.start(id);
                    }
                }
            }
            continue;
        }
        match k {
            Key::Escape => {
                app.esc_handled = true;
                if app.tool.is_some() {
                    crate::tools::finish(app);
                } else if app.dialog.is_some() {
                    crate::dialogs::cancel(app);
                } else if !app.session.selection.is_empty() || app.session.active_sketch.is_none() {
                    let _ = app.run("select.clear", json!({}));
                } else {
                    // Nothing left to cancel: Esc finishes the sketch.
                    app.finish_sketch();
                }
            }
            Key::S if !mods.shift && !mods.alt => {
                let at = ctx.input(|i| i.pointer.latest_pos());
                crate::shortcut_box::open(app, at);
            }
            Key::V => crate::context_menu::toggle_visibility(app),
            Key::F2 => {
                let at = ctx.input(|i| i.pointer.latest_pos()).unwrap_or(ctx.content_rect().center());
                crate::context_menu::rename_selection(app, at);
            }
            Key::F6 => app.animate_view("fit"),
            Key::Delete | Key::Backspace => crate::viewport::delete_selection(app),
            _ => {
                if let Some(id) = crate::keymap::command_for(app, &crate::keymap::key_name(k, mods)) {
                    app.start(id);
                }
            }
        }
    }
}
