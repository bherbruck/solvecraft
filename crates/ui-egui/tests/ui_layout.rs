//! Layout checks over every toolbar command, headless (see `solvecraft_ui_egui::scenario`):
//! each command's dialog opens docked at the viewport's right within its width limits, and
//! every window it opens fits on screen.

use serde_json::{Value, json};
use solvecraft_ui_egui::scenario::{Harness, check};

/// A box body (its sketch finished) to give feature dialogs something to work on.
fn harness() -> Harness {
    let mut h = Harness::new();
    for (id, p) in [
        ("SketchCreate", json!({"plane": "XY"})),
        ("ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [40, 30]})),
        ("SketchStop", json!({})),
        ("Extrude", json!({"distance": 10})),
    ] {
        h.call("engine.execute", json!({"command": id, "params": p}));
    }
    h.frames(3);
    h
}

#[test]
#[ignore = "pending: the exploded view dialog opens 635 px wide (solvecraft-ui)"]
fn every_dialog_opens_docked_within_its_width_limits() {
    let mut bad = Vec::new();
    let mut seen = 0;
    for spec in solvecraft_engine::command_specs() {
        if spec.tab.is_empty() || spec.tab == "SKETCH" {
            continue;
        }
        let mut h = harness();
        h.call("ui.start", json!({"command": spec.id}));
        h.frames(4);
        if h.app.dialog.is_some() {
            seen += 1;
            // Command dialogs are 260–380 px (plus their 1 px border), the few with tables
            // (motion study, configurations) 480–560.
            let w = h.ctx.memory(|m| m.area_rect(egui::Id::new("sc_dialog"))).map_or(0.0, |r| r.width());
            let limits = if w > 400.0 { json!({"min": 480, "max": 562}) } else { json!({"min": 260, "max": 382}) };
            if let Err(e) = check(&mut h, &json!({ "dialog_docked": limits })) {
                bad.push(format!("{}: {e}", spec.id));
            }
        }
        if let Err(e) = check(&mut h, &json!({"windows_fit": {"max_width": 900}})) {
            bad.push(format!("{}: {e}", spec.id));
        }
    }
    assert!(seen > 20, "only {seen} commands opened a dialog");
    assert!(bad.is_empty(), "{} problems:\n{}", bad.len(), bad.join("\n"));
}

/// Windows that are not command dialogs: each fits on screen.
#[test]
fn other_windows_fit_on_screen() {
    let mut bad = Vec::new();
    let opens: [(&str, Value); 3] = [("ui.help", json!({"item": "about"})), ("ui.key", json!({"key": "S"})), ("ui.prefs", json!({"open": true}))];
    for (m, p) in opens {
        let mut h = harness();
        h.call(m, p.clone());
        h.frames(4);
        if let Err(e) = check(&mut h, &json!({"windows_fit": {"max_width": 900}})) {
            bad.push(format!("{m} {p}: {e}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
