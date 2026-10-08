//! Autosave ticks and the "Recover unsaved design?" window shown at launch.

use egui::RichText;
use serde_json::json;
use solvecraft_engine::recovery::{self, Autosaver, Entry};

use crate::SolveApp;

/// A command that ran at least this long counts as a big operation: autosave right after it.
pub const BIG_OPERATION_MS: f64 = 1500.0;

/// Autosave (when due, or after a big operation).
pub fn tick(app: &mut SolveApp) {
    let big = std::mem::take(&mut app.big_operation);
    let Some(a) = app.autosave.as_mut() else { return };
    if let Err(e) = a.tick(&app.session, big) {
        let msg = format!("autosave failed: {e}");
        if app.status.as_ref().is_none_or(|(s, _, _)| *s != msg) {
            app.set_status(msg, true);
        }
    }
}

/// Start autosaving into `dir` and queue the designs that crashed apps left there.
pub fn start(app: &mut SolveApp, dir: &std::path::Path, interval: std::time::Duration) {
    match Autosaver::new(dir, interval) {
        Ok(a) => {
            app.recoverable = recovery::orphans(dir);
            app.autosave = Some(a);
        }
        Err(e) => app.set_status(format!("autosave is off: {e}"), true),
    }
}

/// The app is closing: keep unsaved work for the next launch, drop everything else.
pub fn close(app: &mut SolveApp) {
    if let Some(a) = app.autosave.take() {
        a.close(&app.session);
    }
}

fn ago(secs: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(secs);
    let d = now.saturating_sub(secs);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", d / 60),
        3600..=86_399 => format!("{} h ago", d / 3600),
        _ => format!("{} days ago", d / 86_400),
    }
}

pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if app.recoverable.is_empty() {
        return;
    }
    let dir = app.autosave.as_ref().map(|a| a.dir.clone()).or_else(recovery::default_dir);
    let mut act: Option<(Entry, bool)> = None;
    let mut later = false;
    crate::frame::window(ctx, RichText::new("Recover unsaved design?").strong().size(13.0), crate::frame::Width::Normal)
        .collapsible(false)
        .resizable(false)
        .min_width(420.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label("SolveCraft closed before these designs were saved. Their last autosave can be opened.");
            ui.add_space(6.0);
            for e in app.recoverable.iter().take(20) {
                ui.label(RichText::new(&e.name).strong());
                let file = e.path.clone().unwrap_or_else(|| "never saved".into());
                ui.label(RichText::new(format!("{file} · {} features · autosaved {}", e.features, ago(e.saved_at))).small().weak());
                ui.horizontal(|ui| {
                    if ui.button("Recover").clicked() {
                        act = Some((e.clone(), true));
                    }
                    if ui.button("Discard").clicked() {
                        act = Some((e.clone(), false));
                    }
                });
                ui.separator();
            }
            ui.horizontal(|ui| {
                if ui.button("Later").on_hover_text("Keep them; ask again next launch").clicked() {
                    later = true;
                }
            });
        });
    let dir_s = dir.map(|d| d.to_string_lossy().to_string());
    if let Some((e, recover)) = act {
        app.recoverable.retain(|x| x.id != e.id);
        if recover {
            if app.session.is_dirty() {
                app.set_status("save or discard the open design's changes before recovering another", true);
                app.recoverable.insert(0, e);
                return;
            }
            if app.run("doc.recover", json!({"id": e.id, "dir": dir_s})).is_ok() {
                // Ours now: autosave it under this app's entry before the old entry goes.
                let kept = app.autosave.as_mut().map(|a| a.save(&app.session));
                if let Some(Ok(_)) = kept {
                    let _ = app.run("doc.recovery_discard", json!({"id": e.id, "dir": dir_s}));
                }
                app.set_status(format!("recovered {}: save it to keep it", e.name), false);
            }
        } else {
            let _ = app.run("doc.recovery_discard", json!({"id": e.id, "dir": dir_s}));
        }
    }
    if later {
        app.recoverable.clear();
    }
}
