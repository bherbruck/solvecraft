//! The command registry. Ids follow Fusion's command ids where Fusion has the command; the
//! toolbar placement (`tab`, `panel`) follows Fusion's Design workspace so the registry doubles
//! as the parity metric (`cargo xtask parity`).

mod browser;
mod clipboard;
mod component;
mod edit;
mod face;
mod features;
mod features_more;
pub use features::auto_operation;
mod file;
mod inspect;
pub(crate) mod joints;
mod measure_sel;
mod section;
pub(crate) use measure_sel::measure_items;
mod parameters;
mod sheet;
mod sketch;
mod sketch_constraints;
mod sketch_create;
mod sketch_freeform;
mod sketch_import;
mod sketch_modify;
mod sketch_project;

use serde::Serialize;
use serde_json::Value;

use crate::{Result, Session};

pub type Run = fn(&mut Session, &Value) -> Result<Value>;
pub type Enabled = fn(&Session) -> std::result::Result<(), String>;

pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Toolbar tab (`SOLID`, `SKETCH`…); empty = not on the toolbar.
    pub tab: &'static str,
    /// Toolbar panel (`CREATE`, `MODIFY`…).
    pub panel: &'static str,
    /// Icon name for the UI (drawn in code).
    pub icon: &'static str,
    pub shortcut: Option<&'static str>,
    /// One-line parameter documentation.
    pub params: &'static str,
    pub enabled: Enabled,
    pub run: Run,
    pub undoable: bool,
}

impl CommandSpec {
    pub const fn new(id: &'static str, label: &'static str, run: Run) -> Self {
        CommandSpec { id, label, tab: "", panel: "", icon: "", shortcut: None, params: "", enabled: always, run, undoable: true }
    }
    pub const fn at(mut self, tab: &'static str, panel: &'static str) -> Self {
        self.tab = tab;
        self.panel = panel;
        self
    }
    pub const fn icon(mut self, i: &'static str) -> Self {
        self.icon = i;
        self
    }
    pub const fn key(mut self, k: &'static str) -> Self {
        self.shortcut = Some(k);
        self
    }
    pub const fn params(mut self, p: &'static str) -> Self {
        self.params = p;
        self
    }
    pub const fn enabled(mut self, e: Enabled) -> Self {
        self.enabled = e;
        self
    }
    pub const fn noundo(mut self) -> Self {
        self.undoable = false;
        self
    }
    pub fn info(&self, s: &Session) -> CommandInfo {
        let e = (self.enabled)(s);
        CommandInfo {
            id: self.id,
            label: self.label,
            tab: self.tab,
            panel: self.panel,
            icon: self.icon,
            shortcut: self.shortcut,
            params: self.params,
            enabled: e.is_ok(),
            disabled_reason: e.err(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CommandInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub tab: &'static str,
    pub panel: &'static str,
    pub icon: &'static str,
    pub shortcut: Option<&'static str>,
    pub params: &'static str,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
}

pub fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}

pub fn in_sketch(s: &Session) -> std::result::Result<(), String> {
    if s.active_sketch.is_some() { Ok(()) } else { Err("no sketch is being edited (Create Sketch or Edit Sketch first)".into()) }
}

/// Every command, in toolbar order.
pub fn command_specs() -> Vec<&'static CommandSpec> {
    let mut v: Vec<&'static CommandSpec> = Vec::new();
    v.extend(sketch::COMMANDS.iter());
    v.extend(sketch_create::COMMANDS.iter());
    v.extend(sketch_constraints::COMMANDS.iter());
    v.extend(sketch_import::COMMANDS.iter());
    v.extend(sketch_modify::COMMANDS.iter());
    v.extend(sketch_project::COMMANDS.iter());
    v.extend(features::COMMANDS.iter());
    v.extend(features_more::COMMANDS.iter());
    v.extend(sheet::COMMANDS.iter());
    v.extend(component::COMMANDS.iter());
    v.extend(joints::COMMANDS.iter());
    v.extend(browser::COMMANDS.iter());
    v.extend(edit::COMMANDS.iter());
    v.extend(clipboard::COMMANDS.iter());
    v.extend(parameters::COMMANDS.iter());
    v.extend(file::COMMANDS.iter());
    v.extend(inspect::COMMANDS.iter());
    v.extend(section::COMMANDS.iter());
    v
}

pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    let all = command_specs();
    all.iter().find(|c| c.id == id).or_else(|| all.iter().find(|c| c.id.eq_ignore_ascii_case(id))).copied()
}
