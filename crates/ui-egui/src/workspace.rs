//! Toolbar layout of the tabs that share commands with others (Fusion's layout): the SOLID
//! tab's ASSEMBLE panel, SHEET METAL and PLASTIC. A command is registered on one tab and panel;
//! these panels list the commands they show in order (the first few get big buttons), so Extrude
//! or Joint appear on several tabs. Commands registered on such a panel but not listed follow at
//! the end.

use solvecraft_engine::CommandSpec;
use solvecraft_engine::doc::FeatureKind;

const ASSEMBLE: &[&str] = &[
    "FusionCreateNewComponentCommand",
    "JointAssembleCmdNew",
    "JointAsBuiltCmd",
    "JointOrigin",
    "RigidGroupCmd",
    "FusionMoveJointsCommand",
    "FusionMotionRelationshipCommand",
    "FusionMotionStudyCommand",
    "explode.create",
    "EnableContactSetsCmd",
    "EnableAllContactCmd",
    "DisableAllContactCmd",
    "ContactSetCmd",
    "InterferenceCheckCommand",
];

const SHEET_CREATE: &[&str] = &[
    "SketchCreate",
    "FusionSheetMetalFlangeCommand",
    "FusionSheetMetalFlatPatternCmd",
    "FusionSheetMetalHemFlangeCommand",
    "ConvertToSheetMetalCmd",
    "FusionCreateNewComponentCommand",
    "Extrude",
    "FusionHoleCommand",
    "FusionThreadCommand",
    "PatternRectangular",
    "PatternCircular",
    "PatternOnPath",
    "MirrorCommand",
    "JointOrigin",
];

const SHEET_MODIFY: &[&str] = &[
    "FusionSheetMetalRulesCommand",
    "FusionSheetmetalUnfoldCommand",
    "sheet.refold",
    "FusionFilletEdgesCommand",
    "FusionChamferCommand",
    "FusionMoveCommand",
    "AlignCmd",
    "FusionDeleteCommand",
    "SoftDeleteCommand",
    "PhysicalMaterialCommand",
    "ChangeParameterCommand",
];

const PLASTIC_SETUP: &[&str] = &["FusionManagePlasticRuleCommand", "FusionAssignPlasticRuleCommand"];

const PLASTIC_CREATE: &[&str] = &[
    "SketchCreate",
    "FusionBossCommand",
    "FusionSnapFitCommand",
    "FusionLipCommand",
    "FusionRestCommand",
    "FusionCreateNewComponentCommand",
    "Extrude",
    "Revolve",
    "Sweep",
    "SolidLoft",
    "FusionRibCommand",
    "FusionWebCommand",
    "EmbossCmd",
    "FusionHoleCommand",
    "FusionThreadCommand",
    "PrimitiveBox",
    "PrimitiveCylinder",
    "PrimitiveSphere",
    "PrimitiveTorus",
    "PrimitiveCoil",
    "PrimitivePipe",
    "PatternRectangular",
    "PatternCircular",
    "PatternOnPath",
    "MirrorCommand",
    "JointOrigin",
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
            assert_eq!(a.get(1).copied(), Some("JointAssembleCmdNew"), "{tab}: {a:?}");
            assert!(a.contains(&"FusionMoveJointsCommand") && a.contains(&"FusionCreateNewComponentCommand"));
        }
        let c = ids("SHEET METAL", "CREATE");
        assert_eq!(c.get(1).copied(), Some("FusionSheetMetalFlangeCommand"));
        assert!(c.contains(&"Extrude") && c.contains(&"FusionSheetMetalFlatPatternCmd"));
        assert!(ids("SHEET METAL", "MODIFY").contains(&"FusionSheetmetalUnfoldCommand"));
        assert!(ids("PLASTIC", "SETUP").contains(&"FusionManagePlasticRuleCommand"));
        let p = ids("PLASTIC", "CREATE");
        assert!(p.contains(&"FusionBossCommand") && p.contains(&"FusionRibCommand") && p.contains(&"FusionLipCommand"));
        assert!(ids("PLASTIC", "MODIFY").contains(&"FusionShellBodyCommand"));
        assert!(panels("PLASTIC").is_some_and(|p| p.first() == Some(&"SETUP")));
    }
}
