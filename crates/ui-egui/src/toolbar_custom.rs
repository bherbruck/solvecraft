//! Toolbar customisation: which commands each panel shows as buttons, per workspace and tab.
//! A command's menu (right-click on a toolbar button, a panel drop-down row or an S box row)
//! pins it to the toolbar, removes it, or pins it to the S box's favourites; buttons are dragged
//! to reorder them within and between panels; Reset Toolbar restores the tab. Kept with the
//! preferences and exported or imported with the keyboard shortcuts.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solvecraft_engine::{CommandSpec, command_specs};

use crate::SolveApp;
use crate::context_menu::Item;

/// The only workspace so far.
pub const WORKSPACE: &str = "DESIGN";

#[derive(Default, Clone, Debug, PartialEq)]
pub struct ToolbarCustom {
    /// `"<workspace>/<tab>/<panel>"` → the panel's buttons, in order (only panels the user
    /// changed).
    pub panels: BTreeMap<String, Vec<String>>,
}

impl ToolbarCustom {
    pub fn prefs(&self) -> Value {
        json!(self.panels)
    }
    pub fn load(&mut self, v: &Value) {
        self.panels = v
            .as_object()
            .map(|o| {
                o.iter()
                    .take(500)
                    .filter_map(|(k, v)| {
                        let ids: Vec<String> = v.as_array()?.iter().take(100).filter_map(|x| x.as_str().map(str::to_string)).collect();
                        Some((k.clone(), ids))
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
}

fn key(tab: &str, panel: &str) -> String {
    format!("{WORKSPACE}/{tab}/{panel}")
}

/// The buttons a panel shows: the user's list, or the first `promoted` commands of the panel.
pub fn buttons(app: &SolveApp, tab: &str, panel: &str, cmds: &[&'static CommandSpec], promoted: usize) -> Vec<&'static CommandSpec> {
    match app.toolbar_custom.panels.get(&key(tab, panel)) {
        Some(ids) => ids.iter().filter_map(|id| solvecraft_engine::find_command(id)).collect(),
        None => cmds.iter().take(promoted).copied().collect(),
    }
}

/// The panels of a tab with their commands and default button counts (what the toolbar draws).
pub fn panel_lists(tab: &str) -> Vec<(&'static str, Vec<&'static CommandSpec>, usize)> {
    let specs = command_specs();
    crate::toolbar::panels(tab)
        .iter()
        .map(|panel| {
            let (cmds, promote) = match crate::workspace::layout(tab, panel, &specs) {
                Some(l) => l,
                None => (specs.iter().filter(|c| c.tab == tab && c.panel == *panel).copied().collect(), crate::toolbar::promoted(tab, panel)),
            };
            let cmds = crate::sketch_tools::also_in(tab, panel, cmds, &specs);
            (*panel, cmds, promote)
        })
        .collect()
}

/// The buttons of a panel as ids, made the user's own list (so it can change).
fn own(app: &mut SolveApp, tab: &str, panel: &str) -> Vec<String> {
    let k = key(tab, panel);
    if let Some(v) = app.toolbar_custom.panels.get(&k) {
        return v.clone();
    }
    let lists = panel_lists(tab);
    let ids: Vec<String> =
        lists.iter().find(|(p, ..)| *p == panel).map(|(_, cmds, n)| cmds.iter().take(*n).map(|c| c.id.to_string()).collect()).unwrap_or_default();
    ids
}

/// Where a command is a button on a tab now: its panel.
pub fn pinned_on(app: &SolveApp, tab: &str, id: &str) -> Option<&'static str> {
    panel_lists(tab).into_iter().find(|(panel, cmds, n)| buttons(app, tab, panel, cmds, *n).iter().any(|c| c.id == id)).map(|(p, ..)| p)
}

/// The panel a command belongs to on a tab (its drop-down), or the tab's first panel.
fn home_panel(tab: &str, id: &str) -> Option<&'static str> {
    let lists = panel_lists(tab);
    lists.iter().find(|(_, cmds, _)| cmds.iter().any(|c| c.id == id)).or(lists.first()).map(|(p, ..)| *p)
}

/// Pin a command to the toolbar of a tab (as a button of its panel, at the end).
pub fn pin(app: &mut SolveApp, tab: &str, id: &str) -> Result<(), String> {
    if solvecraft_engine::find_command(id).is_none() {
        return Err(format!("no command `{id}`"));
    }
    if pinned_on(app, tab, id).is_some() {
        return Ok(());
    }
    let panel = home_panel(tab, id).ok_or_else(|| format!("the {tab} tab has no panels"))?;
    let mut ids = own(app, tab, panel);
    ids.push(id.to_string());
    app.toolbar_custom.panels.insert(key(tab, panel), ids);
    Ok(())
}

/// Remove a command's button from a tab (it stays in its panel's drop-down).
pub fn remove(app: &mut SolveApp, tab: &str, id: &str) {
    let Some(panel) = pinned_on(app, tab, id) else { return };
    let mut ids = own(app, tab, panel);
    ids.retain(|x| x != id);
    app.toolbar_custom.panels.insert(key(tab, panel), ids);
}

/// Move a button before another one (`before`: None puts it at the end of `panel`).
pub fn reorder(app: &mut SolveApp, tab: &str, id: &str, panel: &str, before: Option<&str>) {
    if let Some(from) = pinned_on(app, tab, id) {
        let mut ids = own(app, tab, from);
        ids.retain(|x| x != id);
        app.toolbar_custom.panels.insert(key(tab, from), ids);
    }
    let mut ids = own(app, tab, panel);
    ids.retain(|x| x != id);
    let at = before.and_then(|b| ids.iter().position(|x| x == b)).unwrap_or(ids.len());
    ids.insert(at, id.to_string());
    app.toolbar_custom.panels.insert(key(tab, panel), ids);
}

/// Reset Toolbar: a tab's panels back to their defaults.
pub fn reset(app: &mut SolveApp, tab: &str) {
    let prefix = format!("{WORKSPACE}/{tab}/");
    app.toolbar_custom.panels.retain(|k, _| !k.starts_with(&prefix));
}

/// The menu of a command (toolbar button, drop-down row, S box row).
pub fn command_items(app: &SolveApp, id: &str) -> Vec<Item> {
    let tab = app.ui.tab.clone();
    let on = pinned_on(app, &tab, id).is_some();
    let fav = app.sbox.pinned.iter().any(|p| p == id);
    let mut v = Vec::new();
    v.push(if on {
        Item::action("ui.toolbarRemove", "Remove from Toolbar", "").with(json!({ "command": id }))
    } else {
        Item::action("ui.toolbarPin", "Pin to Toolbar", "").with(json!({ "command": id }))
    });
    v.push(Item::action("ui.shortcutPin", if fav { "Unpin from Shortcuts" } else { "Pin to Shortcuts" }, "").with(json!({ "command": id })));
    v.push(Item::sep());
    v.push(Item::action("ui.toolbarReset", "Reset Toolbar", "").with(json!({ "tab": tab })));
    v
}

/// Run a toolbar item from a menu.
pub fn run(app: &mut SolveApp, action: &str, p: &Value) {
    let tab = p.get("tab").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| app.ui.tab.clone());
    let id = p.get("command").and_then(Value::as_str).unwrap_or("");
    match action {
        "ui.toolbarPin" => {
            if let Err(e) = pin(app, &tab, id) {
                app.set_status(e, true);
            }
        }
        "ui.toolbarRemove" => remove(app, &tab, id),
        "ui.toolbarReset" => reset(app, &tab),
        "ui.shortcutPin" => app.sbox.toggle_pin(id),
        _ => {}
    }
}

/// Shortcut keys, S box favourites and the toolbar as one file (Keyboard Shortcuts › Export).
pub fn export(app: &SolveApp) -> Value {
    json!({ "format": "solvecraft-shortcuts/1", "shortcuts": app.keymap.prefs(), "favourites": app.sbox.pinned, "toolbar": app.toolbar_custom.prefs() })
}

pub fn import(app: &mut SolveApp, v: &Value) -> Result<(), String> {
    if v.get("format").and_then(Value::as_str) != Some("solvecraft-shortcuts/1") {
        return Err("not a SolveCraft shortcuts file".into());
    }
    if let Some(s) = v.get("shortcuts") {
        app.keymap.load(s);
    }
    if let Some(f) = v.get("favourites").and_then(Value::as_array) {
        app.sbox.pinned = f.iter().take(64).filter_map(|x| x.as_str().map(str::to_string)).collect();
    }
    if let Some(t) = v.get("toolbar") {
        app.toolbar_custom.load(t);
    }
    Ok(())
}
