//! Toolbar layout of the tabs that share commands with others (Fusion's layout): the SOLID
//! tab's ASSEMBLE panel, SHEET METAL and PLASTIC. A command is registered on one tab and panel;
//! these panels list the commands they show in order (the first few get big buttons), so Extrude
//! or Joint appear on several tabs. Commands registered on such a panel but not listed follow at
//! the end.

use solvecraft_engine::CommandSpec;
use solvecraft_engine::doc::FeatureKind;

const ASSEMBLE: &[&str] = &[
    "component.create",
    "joint.create",
    "joint.as_built",
    "joint.origin",
    "joint.rigid_group",
    "joint.drive",
    "joint.motion_link",
    "motion.study",
    "explode.create",
    "contact.enable_sets",
    "contact.enable_all",
    "contact.disable_all",
    "contact.create",
    "inspect.interference",
];

const SHEET_CREATE: &[&str] = &[
    "sketch.create",
    "sheet.flange",
    "sheet.flat_pattern",
    "sheet.hem",
    "sheet.fold",
    "sheet.convert",
    "component.create",
    "solid.extrude",
    "solid.hole",
    "solid.thread",
    "solid.pattern.rectangular",
    "solid.pattern.circular",
    "solid.pattern.path",
    "solid.mirror",
    "joint.origin",
];

const SHEET_MODIFY: &[&str] = &[
    "sheet.manage_rules",
    "sheet.unfold",
    "sheet.refold",
    "solid.fillet",
    "solid.chamfer",
    "solid.move",
    "solid.align",
    "timeline.delete",
    "solid.remove",
    "material.assign",
    "parameters.change",
];

const PLASTIC_SETUP: &[&str] = &["plastic.manage_rules", "plastic.assign_rule"];

const PLASTIC_CREATE: &[&str] = &[
    "sketch.create",
    "plastic.boss",
    "plastic.snap_fit",
    "plastic.lip",
    "plastic.rest",
    "component.create",
    "solid.extrude",
    "solid.revolve",
    "solid.sweep",
    "solid.loft",
    "solid.rib",
    "solid.web",
    "solid.emboss",
    "solid.hole",
    "solid.thread",
    "solid.box",
    "solid.cylinder",
    "solid.sphere",
    "solid.torus",
    "solid.coil",
    "solid.pipe",
    "solid.pattern.rectangular",
    "solid.pattern.circular",
    "solid.pattern.path",
    "solid.mirror",
    "joint.origin",
];

/// Panels of a tab, when this module lays the tab out.
pub fn panels(tab: &str) -> Option<&'static [&'static str]> {
    match tab {
        "SOLID" => Some(&["CREATE", "MODIFY", "ASSEMBLE", "CONFIGURE", "CONSTRUCT", "INSPECT", "INSERT", "SELECT"]),
        "PLASTIC" => Some(&["SETUP", "CREATE", "MODIFY", "ASSEMBLE", "CONSTRUCT", "INSPECT", "INSERT", "SELECT"]),
        _ => None,
    }
}

/// The commands a panel shows, in order, and how many get a big button; `None` leaves the panel
/// to the registry.
pub fn layout<'a>(tab: &str, panel: &str, specs: &[&'a CommandSpec]) -> Option<(Vec<&'a CommandSpec>, usize)> {
    let (ids, promoted, own_tab): (&[&str], usize, &str) = match (tab, panel) {
        ("SOLID" | "SHEET METAL" | "PLASTIC", "ASSEMBLE") => (ASSEMBLE, 3, tab),
        ("SHEET METAL", "CREATE") => (SHEET_CREATE, 3, tab),
        ("SHEET METAL", "MODIFY") => (SHEET_MODIFY, 2, tab),
        ("PLASTIC", "SETUP") => (PLASTIC_SETUP, 2, tab),
        ("PLASTIC", "CREATE") => (PLASTIC_CREATE, 5, tab),
        // The solid tab's panels.
        ("PLASTIC", "MODIFY") => (&[], 4, "SOLID"),
        ("SHEET METAL" | "PLASTIC", "CONSTRUCT" | "INSPECT" | "INSERT") => (&[], 2, "SOLID"),
        _ => return None,
    };
    let mut out: Vec<&CommandSpec> = ids.iter().filter_map(|id| specs.iter().find(|c| c.id == *id).copied()).collect();
    for c in specs.iter().filter(|c| (c.tab == tab || c.tab == own_tab) && c.panel == panel) {
        if !out.iter().any(|o| o.id == c.id) {
            out.push(*c);
        }
    }
    Some((out, promoted))
}

/// Timeline and browser icon of the sheet metal and plastic features (`None`: not one of them).
pub fn feature_icon(k: &FeatureKind) -> Option<&'static str> {
    Some(match k {
        FeatureKind::SheetBase { .. } | FeatureKind::SheetContour { .. } | FeatureKind::SheetFlange { .. } => "flange",
        FeatureKind::SheetHem { .. } => "hem",
        FeatureKind::SheetFold { .. } => "fold",
        FeatureKind::SheetUnfold { refold: false, .. } => "unfold",
        FeatureKind::SheetUnfold { .. } => "refold",
        FeatureKind::SheetConvert { .. } => "convert_sheet",
        FeatureKind::Boss { .. } => "boss",
        FeatureKind::Lip { .. } => "lip",
        FeatureKind::SnapFit { .. } => "snap",
        FeatureKind::Rest { .. } => "rest",
        FeatureKind::Rib { .. } => "rib",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(tab: &str, panel: &str) -> Vec<&'static str> {
        let specs = solvecraft_engine::command_specs();
        layout(tab, panel, &specs).map(|(v, _)| v.iter().map(|c| c.id).collect()).unwrap_or_default()
    }

    /// Fusion's layout: joints on every tab's ASSEMBLE panel, sheet metal and plastic tabs
    /// reach the shared solid commands, the plastic rules sit on SETUP.
    #[test]
    fn tabs_share_commands_in_fusion_order() {
        for tab in ["SOLID", "SHEET METAL", "PLASTIC"] {
            let a = ids(tab, "ASSEMBLE");
            assert_eq!(a.get(1).copied(), Some("joint.create"), "{tab}: {a:?}");
            assert!(a.contains(&"joint.drive") && a.contains(&"component.create"));
        }
        let c = ids("SHEET METAL", "CREATE");
        assert_eq!(c.get(1).copied(), Some("sheet.flange"));
        assert!(c.contains(&"solid.extrude") && c.contains(&"sheet.flat_pattern"));
        assert!(ids("SHEET METAL", "MODIFY").contains(&"sheet.unfold"));
        assert!(ids("PLASTIC", "SETUP").contains(&"plastic.manage_rules"));
        let p = ids("PLASTIC", "CREATE");
        assert!(p.contains(&"plastic.boss") && p.contains(&"solid.rib") && p.contains(&"plastic.lip"));
        assert!(ids("PLASTIC", "MODIFY").contains(&"solid.shell"));
        assert!(panels("PLASTIC").is_some_and(|p| p.first() == Some(&"SETUP")));
    }
}
