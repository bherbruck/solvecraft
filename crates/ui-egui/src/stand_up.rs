//! Standing up a Y-up file (#3). Many CAD programs save parts with Y up, so a STEP or IGES file
//! from them opens lying on its back in a Z-up design. After such a file opens, SolveCraft
//! offers to stand it up: a quarter turn about X (Y → Z) of the bodies it brought, as one
//! Move feature that can be undone or edited. Preferences › Design › "Offer to stand up Y-up
//! files" turns the offer off; with Y up as the default orientation it isn't needed.

use serde_json::json;

use crate::SolveApp;

/// The bodies a just-opened file brought (`before`: body names before it opened, none for a
/// new design): offer to stand them up.
pub fn offer(app: &mut SolveApp, path: &str, before: &[String]) {
    let io = &solvecraft_engine::io::is_step_path;
    if !(io(path) || solvecraft_engine::io::is_iges_path(path)) || !app.preferences.offer_stand_up || app.preferences.up_axis != "z" {
        return;
    }
    let bodies: Vec<String> = app.session.world_state().bodies.iter().map(|b| b.name.clone()).filter(|n| !before.contains(n)).collect();
    if !bodies.is_empty() {
        app.stand_up = Some(bodies);
    }
}

/// Turn the offered bodies a quarter turn about X (Y up becomes Z up).
pub fn stand_up(app: &mut SolveApp) -> Result<(), String> {
    let Some(bodies) = app.stand_up.take() else { return Err("no file is waiting to be stood up".into()) };
    app.run("solid.move", json!({ "bodies": bodies, "axis": [1, 0, 0], "angle": "90 deg" })).map_err(|e| e.to_string())?;
    app.animate_view("home");
    Ok(())
}

/// The offer, under the toolbar.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if app.stand_up.is_none() {
        return;
    }
    let mut act: Option<&str> = None;
    crate::frame::window(ctx, "Stand this part up?", crate::frame::Width::Normal)
        .id(egui::Id::new("sc_stand_up"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 140.0))
        .show(ctx, |ui| {
            ui.label("Files from many CAD programs have Y up, so they open lying on their back here (Z up).");
            ui.label("Stand it up: a quarter turn about X, as one Move feature you can undo.");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Stand it up").clicked() {
                    act = Some("stand");
                }
                if ui.button("Leave it").clicked() {
                    act = Some("leave");
                }
                if ui.button("Don't ask again").on_hover_text("Preferences › Design turns the offer back on").clicked() {
                    act = Some("never");
                }
            });
        });
    match act {
        Some("stand") => {
            if let Err(e) = stand_up(app) {
                app.set_status(e, true);
            }
        }
        Some("leave") => app.stand_up = None,
        Some("never") => {
            app.stand_up = None;
            app.preferences.offer_stand_up = false;
        }
        _ => {}
    }
}
