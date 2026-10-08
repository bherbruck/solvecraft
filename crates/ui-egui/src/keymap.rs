//! Keyboard shortcuts: every command's default key comes from its `CommandSpec`; the user can
//! rebind or clear any of them (Preferences › Keyboard Shortcuts). A new key that another command
//! already uses is reported and only taken over on request; keys the app itself handles (Esc, S,
//! V, Delete, Ctrl+Z…) can't be bound. Changes are kept between runs.

use std::collections::BTreeMap;

use egui::RichText;
use serde_json::{Value, json};
use solvecraft_engine::command_specs;

use crate::SolveApp;
use crate::theme::Tokens;

/// Keys the application handles before any command shortcut.
pub const RESERVED: &[&str] =
    &["Escape", "S", "V", "F2", "F6", "Delete", "Backspace", "Ctrl+Z", "Ctrl+Y", "Ctrl+S", "Ctrl+N", "Ctrl+O", "Ctrl+Comma"];

#[derive(Default)]
pub struct Keymap {
    /// Command id → key (`""`: no key). Only the changed ones.
    pub custom: BTreeMap<String, String>,
    /// Asks for the Keyboard Shortcuts page of Preferences (File and Help menus).
    pub open: bool,
    /// Opened from Help: a list to read, not to edit.
    pub read_only: bool,
    filter: String,
    /// The command waiting for a key press.
    capturing: Option<String>,
    /// A key that another command uses: (command, key, the other commands).
    conflict: Option<(String, String, Vec<String>)>,
    message: Option<String>,
}

impl Keymap {
    pub fn prefs(&self) -> Value {
        json!(self.custom)
    }
    pub fn load(&mut self, v: &Value) {
        self.custom =
            v.as_object().map(|o| o.iter().take(2000).filter_map(|(k, v)| Some((current(k), v.as_str()?.to_string()))).collect()).unwrap_or_default();
    }
}

/// Saved before the command ids were renamed: the current id.
fn current(id: &str) -> String {
    solvecraft_engine::legacy_ids::current_id(id).to_string()
}

/// A key press as a shortcut name: `E`, `Shift+E`, `Ctrl+Alt+K`, `F5`, `Delete`.
pub fn key_name(key: egui::Key, mods: egui::Modifiers) -> String {
    let mut s = String::new();
    if mods.command || mods.ctrl {
        s.push_str("Ctrl+");
    }
    if mods.alt {
        s.push_str("Alt+");
    }
    if mods.shift {
        s.push_str("Shift+");
    }
    s.push_str(key.name());
    s
}

/// The key a command runs from now.
pub fn effective(app: &SolveApp, id: &str) -> Option<String> {
    match app.keymap.custom.get(id) {
        Some(k) if k.is_empty() => None,
        Some(k) => Some(k.clone()),
        None => solvecraft_engine::find_command(id).and_then(|c| c.shortcut).map(str::to_string),
    }
}

/// Commands bound to `key` now.
pub fn commands_for(app: &SolveApp, key: &str) -> Vec<&'static str> {
    command_specs().into_iter().filter(|c| effective(app, c.id).as_deref() == Some(key)).map(|c| c.id).collect()
}

/// The enabled command a key press runs, if any.
pub fn command_for(app: &SolveApp, key: &str) -> Option<&'static str> {
    commands_for(app, key).into_iter().find(|id| solvecraft_engine::find_command(id).is_some_and(|c| c.info(&app.session).enabled))
}

/// Bind `key` to a command (`""` clears it). Refused for reserved keys, and when other commands
/// use the key unless `replace` (they lose it).
pub fn bind(app: &mut SolveApp, id: &str, key: &str, replace: bool) -> Result<(), String> {
    let spec = solvecraft_engine::find_command(id).ok_or_else(|| format!("no command `{id}`"))?;
    if RESERVED.contains(&key) {
        return Err(format!("{key} is used by SolveCraft itself"));
    }
    if !key.is_empty() {
        let others: Vec<&'static str> = commands_for(app, key).into_iter().filter(|o| *o != id).collect();
        if !others.is_empty() && !replace {
            let names: Vec<&str> = others.iter().filter_map(|o| solvecraft_engine::find_command(o).map(|c| c.label)).collect();
            return Err(format!("{key} is already used by {}", names.join(", ")));
        }
        for o in others {
            app.keymap.custom.insert(o.to_string(), String::new());
        }
    }
    if spec.shortcut.unwrap_or("") == key {
        app.keymap.custom.remove(id);
    } else {
        app.keymap.custom.insert(id.to_string(), key.to_string());
    }
    Ok(())
}

pub fn reset(app: &mut SolveApp, id: Option<&str>) {
    match id {
        Some(id) => drop(app.keymap.custom.remove(id)),
        None => app.keymap.custom.clear(),
    }
}

/// The Keyboard Shortcuts page of Preferences: search, rebind (click, then press the key), clear,
/// reset; a key another command uses is reported and taken over only on request.
pub fn page(app: &mut SolveApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    // A key press while waiting for one binds it (Esc cancels).
    if let Some(id) = app.keymap.capturing.clone() {
        let press = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((k, m)) = press {
            app.keymap.capturing = None;
            if k != egui::Key::Escape {
                let name = key_name(k, m);
                match bind(app, &id, &name, false) {
                    Ok(()) => app.keymap.message = None,
                    Err(e) if e.contains("already used") => {
                        app.keymap.conflict = Some((id.clone(), name.clone(), commands_for(app, &name).iter().map(|s| s.to_string()).collect()));
                        app.keymap.message = Some(e);
                    }
                    Err(e) => app.keymap.message = Some(e),
                }
            }
        }
    }
    let t = Tokens::get();
    let mut action: Option<(String, &str)> = None;
    {
        {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut app.keymap.filter).hint_text("Filter commands or keys").desired_width(300.0));
                if !app.keymap.read_only && ui.button("Reset All").clicked() {
                    action = Some((String::new(), "reset_all"));
                }
                if ui.button("Export…").on_hover_text("Keys, S box favourites and the toolbar, as a file").clicked() {
                    action = Some((String::new(), "export"));
                }
                if !app.keymap.read_only && ui.button("Import…").clicked() {
                    action = Some((String::new(), "import"));
                }
            });
            if let Some(m) = &app.keymap.message {
                ui.label(RichText::new(m).color(t.warning));
            }
            if let Some((id, key, _)) = app.keymap.conflict.clone() {
                ui.horizontal(|ui| {
                    if ui.button(format!("Use {key} here anyway")).clicked() {
                        action = Some((format!("{id}\n{key}"), "replace"));
                    }
                    if ui.button("Cancel").clicked() {
                        action = Some((String::new(), "cancel"));
                    }
                });
            }
            ui.separator();
            let q = app.keymap.filter.to_lowercase();
            egui::ScrollArea::both().max_height(330.0).auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new("sc_keymap_grid").num_columns(4).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
                    for c in command_specs() {
                        let key = effective(app, c.id).unwrap_or_default();
                        if !q.is_empty() && ![c.label, c.id, key.as_str()].iter().any(|s| s.to_lowercase().contains(&q)) {
                            continue;
                        }
                        ui.label(c.label);
                        ui.label(RichText::new(c.id).size(10.5).color(t.text_dim));
                        let capturing = app.keymap.capturing.as_deref() == Some(c.id);
                        let changed = app.keymap.custom.contains_key(c.id);
                        let text = if capturing {
                            "press a key…".to_string()
                        } else if key.is_empty() {
                            "—".to_string()
                        } else {
                            key.clone()
                        };
                        if app.keymap.read_only {
                            ui.label(RichText::new(text).strong());
                            ui.end_row();
                            continue;
                        }
                        let b = egui::Button::new(RichText::new(text).color(if changed { t.accent } else { t.text })).min_size(egui::vec2(80.0, 0.0));
                        if ui.add(b).on_hover_text("Click, then press the new key").clicked() {
                            action = Some((c.id.to_string(), "capture"));
                        }
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!key.is_empty(), egui::Button::new("Clear").small()).clicked() {
                                action = Some((c.id.to_string(), "clear"));
                            }
                            if ui.add_enabled(changed, egui::Button::new("Reset").small()).clicked() {
                                action = Some((c.id.to_string(), "reset"));
                            }
                        });
                        ui.end_row();
                    }
                });
            });
        }
    }
    match action {
        Some((id, "capture")) => {
            app.keymap.capturing = Some(id);
            app.keymap.conflict = None;
            app.keymap.message = None;
        }
        Some((id, "clear")) => drop(bind(app, &id, "", false)),
        Some((id, "reset")) => reset(app, Some(&id)),
        Some((_, "reset_all")) => reset(app, None),
        Some((_, "export")) => {
            let text = crate::toolbar_custom::export(app).to_string();
            if let Some(path) = app.services.pick_save.as_ref().and_then(|f| f("SolveCraft shortcuts.json", &["json"])) {
                app.keymap.message = std::fs::write(&path, text).err().map(|e| format!("export: {e}"));
            }
        }
        Some((_, "import")) => {
            if let Some(path) = app.services.pick_open.as_ref().and_then(|f| f()) {
                let r = std::fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
                    .and_then(|v| crate::toolbar_custom::import(app, &v));
                app.keymap.message = r.err().map(|e| format!("import: {e}"));
            }
        }
        Some((pair, "replace")) => {
            if let Some((id, key)) = pair.split_once('\n') {
                let _ = bind(app, id, key, true);
            }
            app.keymap.conflict = None;
            app.keymap.message = None;
        }
        Some((_, "cancel")) => {
            app.keymap.conflict = None;
            app.keymap.message = None;
        }
        _ => {}
    }
}

/// Stop waiting for a key (the dialog closed).
pub fn stop_capture(app: &mut SolveApp) {
    app.keymap.capturing = None;
    app.keymap.conflict = None;
    app.keymap.message = None;
}

/// Is the page waiting for a key (so the key must not also run a command)?
pub fn capturing(app: &SolveApp) -> bool {
    app.prefs_window.open && app.prefs_window.section == crate::prefs::PAGE_SHORTCUTS && app.keymap.capturing.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> SolveApp {
        SolveApp::new(solvecraft_engine::Session::default(), Default::default())
    }

    #[test]
    fn rebinding_detects_conflicts_and_resets() {
        let mut a = app();
        assert_eq!(effective(&a, "solid.extrude").as_deref(), Some("E"));
        // Shift+E is free.
        bind(&mut a, "solid.extrude", "Shift+E", false).unwrap();
        assert_eq!(commands_for(&a, "Shift+E"), vec!["solid.extrude"]);
        assert!(commands_for(&a, "E").is_empty());
        // M belongs to Move/Copy: refused, then taken over on request.
        let e = bind(&mut a, "solid.revolve", "M", false).unwrap_err();
        assert!(e.contains("already used by Move/Copy"), "{e}");
        bind(&mut a, "solid.revolve", "M", true).unwrap();
        assert_eq!(commands_for(&a, "M"), vec!["solid.revolve"]);
        assert_eq!(effective(&a, "solid.move"), None);
        assert!(bind(&mut a, "solid.revolve", "Ctrl+Z", false).is_err(), "reserved");
        // Kept between runs.
        let mut b = app();
        b.keymap.load(&a.keymap.prefs());
        assert_eq!(effective(&b, "solid.revolve").as_deref(), Some("M"));
        reset(&mut b, None);
        assert_eq!(effective(&b, "solid.move").as_deref(), Some("M"));
        assert_eq!(key_name(egui::Key::K, egui::Modifiers { ctrl: true, shift: true, ..Default::default() }), "Ctrl+Shift+K");
    }
}
