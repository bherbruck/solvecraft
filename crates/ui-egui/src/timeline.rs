//! The timeline (bottom): features in order, the rollback marker, errors and warnings.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;
use solvecraft_engine::doc::FeatureKind;

use crate::theme::Tokens;
use crate::{SolveApp, icons};

fn icon_of(k: &FeatureKind) -> &'static str {
    match k {
        FeatureKind::Sketch { .. } => "sketch",
        FeatureKind::Extrude { .. } => "extrude",
        FeatureKind::Revolve { .. } => "revolve",
        FeatureKind::Fillet { .. } => "fillet",
        FeatureKind::Chamfer { .. } => "chamfer",
        FeatureKind::Box { .. } => "box",
        FeatureKind::Cylinder { .. } => "cylinder",
        FeatureKind::Sphere { .. } => "sphere",
        FeatureKind::Torus { .. } => "torus",
        FeatureKind::Combine { .. } => "combine",
        FeatureKind::Pattern { pattern: solvecraft_engine::doc::PatternKind::Rectangular { .. }, .. } => "pattern_rect",
        FeatureKind::Pattern { .. } => "pattern_circ",
        FeatureKind::Mirror { .. } => "mirror",
        FeatureKind::ConstructionPlane { .. } => "plane",
        FeatureKind::Hole { .. } => "hole",
        FeatureKind::Loft { .. } => "loft",
        FeatureKind::Sweep { .. } => "sweep",
        FeatureKind::Shell { .. } => "shell",
        FeatureKind::Draft { .. } => "draft",
        FeatureKind::Split { .. } => "split",
        FeatureKind::Move { .. } => "move",
    }
}

pub fn timeline(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    egui::Panel::bottom("sc_timeline").exact_size(40.0).frame(egui::Frame::NONE.fill(t.timeline)).show(ui, |ui| {
        let r = ui.max_rect();
        let p = ui.painter().clone();
        p.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, t.border));
        let n = app.session.doc.features.len();
        let marker = app.session.doc.marker.unwrap_or(n);
        // Playback controls.
        let mut x = r.left() + 10.0;
        let controls = [("⏮", Some(0usize)), ("◀", Some(marker.saturating_sub(1))), ("▶", Some((marker + 1).min(n))), ("⏭", None)];
        for (label, target) in controls {
            let br = Rect::from_center_size(pos2(x + 11.0, r.center().y), vec2(22.0, 22.0));
            let resp = ui.interact(br, ui.id().with(("tl", label)), Sense::click());
            if resp.hovered() {
                p.rect_filled(br, 3.0, t.hover);
            }
            p.text(br.center(), Align2::CENTER_CENTER, label, FontId::proportional(13.0), t.icon);
            if resp.clicked() {
                let pos = match target {
                    Some(k) if k < n => json!({"position": k}),
                    _ => json!({}),
                };
                let _ = app.run("timeline.rollback", pos);
            }
            x += 24.0;
        }
        x += 14.0;
        let feats: Vec<_> =
            app.session.doc.features.iter().map(|f| (f.id, f.name.clone(), icon_of(&f.kind), f.suppressed, f.kind.type_name())).collect();
        let mut edit: Option<u64> = None;
        for (i, (id, name, icon, suppressed, ty)) in feats.iter().enumerate() {
            if i == marker {
                p.rect_filled(Rect::from_min_size(pos2(x, r.top() + 5.0), vec2(4.0, r.height() - 10.0)), 1.0, Color32::from_rgb(70, 76, 88));
                x += 8.0;
            }
            let br = Rect::from_min_size(pos2(x, r.top() + 6.0), vec2(28.0, 28.0));
            let resp = ui.interact(br, ui.id().with(("feat", *id)), Sense::click());
            let res = app.session.model.result(*id);
            let err = res.and_then(|r| r.error.clone());
            let warn = res.and_then(|r| r.warning.clone());
            let rolled = i >= marker;
            let sel = app.session.selection.iter().any(|s| matches!(s, solvecraft_engine::Sel::Feature { id: f } if f == id));
            let bg = if err.is_some() {
                Color32::from_rgb(250, 220, 220)
            } else if sel {
                t.accent_soft
            } else if resp.hovered() {
                t.hover
            } else {
                Color32::WHITE
            };
            p.rect(br, 3.0, bg, Stroke::new(1.0, if err.is_some() { t.error } else { t.border }), egui::StrokeKind::Inside);
            let ink = if rolled || *suppressed { t.border } else { t.icon };
            icons::paint(&p, br.shrink(4.0), icon, ink, if rolled { t.panel_header } else { t.icon_fill }, if rolled { t.border } else { t.accent });
            if warn.is_some() && err.is_none() {
                icons::paint(&p, Rect::from_min_size(br.right_top() - vec2(10.0, 0.0), vec2(10.0, 10.0)), "warning", t.icon, t.warning, t.warning);
            }
            let mut tip = format!("{name}  ({ty})");
            if let Some(e) = &err {
                tip += &format!("\nError: {e}");
            }
            if let Some(w) = &warn {
                tip += &format!("\nWarning: {w}");
            }
            if let Some(r) = res {
                tip += &format!("\n{:.1} ms", r.ms);
            }
            let resp = resp.on_hover_text(tip);
            if resp.clicked() {
                let _ = app.run("select.set", json!({"items": [{"type": "feature", "id": id}]}));
            }
            if resp.double_clicked() {
                edit = Some(*id);
            }
            resp.context_menu(|ui| {
                if ui.button(if *suppressed { "Unsuppress" } else { "Suppress" }).clicked() {
                    let _ = app.run("timeline.suppress", json!({"feature": id}));
                    ui.close();
                }
                if ui.button("Roll back to here").clicked() {
                    let _ = app.run("timeline.rollback", json!({"position": i}));
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    let _ = app.run("FusionDeleteCommand", json!({"features": [id.to_string()]}));
                    ui.close();
                }
            });
            x += 32.0;
        }
        if marker >= feats.len() {
            p.rect_filled(Rect::from_min_size(pos2(x, r.top() + 5.0), vec2(4.0, r.height() - 10.0)), 1.0, Color32::from_rgb(70, 76, 88));
        }
        if let Some(id) = edit {
            if matches!(app.session.doc.feature(id).map(|f| &f.kind), Some(FeatureKind::Sketch { .. })) {
                let _ = app.run("SketchActivate", json!({"sketch": id}));
            } else {
                app.palette.text = format!("timeline.edit {{\"feature\": {id}, \"set\": {{}}}}");
                app.ui.palette_open = true;
            }
        }
    });
}
