//! The browser (left): document settings, origin, bodies and sketches.

use egui::{Color32, RichText, Sense, vec2};
use serde_json::json;
use solvecraft_engine::Sel;
use solvecraft_engine::doc::FeatureKind;

use crate::theme::Tokens;
use crate::{SolveApp, icons};

fn row(ui: &mut egui::Ui, icon: &str, label: &str, color: Option<Color32>, selected: bool) -> egui::Response {
    let t = Tokens::get();
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click());
    if selected {
        ui.painter().rect_filled(r, 2.0, t.accent_soft);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 2.0, t.hover);
    }
    let ir = egui::Rect::from_min_size(r.min + vec2(2.0, 2.0), vec2(16.0, 16.0));
    icons::paint(ui.painter(), ir, icon, t.icon, t.icon_fill, t.accent);
    ui.painter().text(r.min + vec2(22.0, 10.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(12.5), color.unwrap_or(t.text));
    resp
}

fn eye(ui: &mut egui::Ui, visible: bool) -> bool {
    let t = Tokens::get();
    let (r, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
    icons::paint(ui.painter(), r, "eye", if visible { t.icon } else { t.border }, t.icon_fill, t.accent);
    resp.on_hover_text(if visible { "Hide" } else { "Show" }).clicked()
}

/// A browser click on a plane or axis: it goes to the open dialog's input if that takes it,
/// otherwise it becomes the selection.
fn pick_from_browser(app: &mut SolveApp, x: Sel) {
    let hit = match &x {
        Sel::Plane { name } => crate::viewport::Hit::Plane { name: name.clone(), point: solvecraft_engine::geom::Vec3::ZERO },
        Sel::Axis { name } => crate::viewport::Hit::Axis { name: name.clone() },
        _ => return,
    };
    if let Some(mut d) = app.dialog.take() {
        if let Some(sel) = d.candidate(&app.session, &hit) {
            d.pick(&app.session, sel);
        }
        app.dialog = Some(d);
        return;
    }
    let _ = app.run("select.set", json!({ "items": [x] }));
}

pub fn browser(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    egui::Panel::left("sc_browser")
        .exact_size(230.0)
        .resizable(false)
        .frame(egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin::symmetric(8, 6)))
        .show(ui, |ui| {
            ui.label(RichText::new("BROWSER").size(11.0).strong().color(t.text_dim));
            ui.add_space(4.0);
            let st = app.session.model.state();
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    icons::paint(ui.painter(), r, "box", t.icon, t.icon_fill, t.accent);
                    ui.label(RichText::new(&app.session.doc.name).strong());
                });
                ui.indent("doc", |ui| {
                    egui::CollapsingHeader::new("Document Settings").default_open(false).show(ui, |ui| {
                        row(ui, "settings", &format!("Units: {}", app.session.doc.units), None, false);
                        let n = app.session.doc.params.len();
                        if row(ui, "params", &format!("Parameters ({n})"), None, false).clicked() {
                            app.start("ChangeParameterCommand");
                        }
                    });
                    ui.horizontal(|ui| {
                        if eye(ui, app.ui.show_origin) {
                            app.ui.show_origin = !app.ui.show_origin;
                        }
                        egui::CollapsingHeader::new("Origin").default_open(false).show(ui, |ui| {
                            let items = [("O", "point", "Origin point"), ("X", "axis", "X axis"), ("Y", "axis", "Y axis"), ("Z", "axis", "Z axis")]
                                .into_iter()
                                .chain([("XY", "plane", "XY"), ("XZ", "plane", "XZ"), ("YZ", "plane", "YZ")]);
                            for (key, icon, label) in items {
                                let visible = !app.ui.hidden_origin.iter().any(|h| h == key);
                                let sel = match icon {
                                    "plane" => Some(Sel::Plane { name: key.into() }),
                                    "axis" => Some(Sel::Axis { name: key.into() }),
                                    _ => None,
                                };
                                let selected = sel.as_ref().is_some_and(|x| app.session.selection.contains(x));
                                ui.horizontal(|ui| {
                                    if eye(ui, visible) {
                                        if visible {
                                            app.ui.hidden_origin.push(key.into());
                                        } else {
                                            app.ui.hidden_origin.retain(|h| h != key);
                                        }
                                    }
                                    if row(ui, icon, label, None, selected).clicked()
                                        && let Some(x) = &sel
                                    {
                                        pick_from_browser(app, x.clone());
                                    }
                                });
                            }
                        });
                    });
                    egui::CollapsingHeader::new(format!("Bodies ({})", st.bodies.len())).default_open(true).show(ui, |ui| {
                        for b in &st.bodies {
                            let visible = !app.ui.hidden_bodies.contains(&b.name);
                            let selected = app.session.selection.iter().any(|x| matches!(x, Sel::Body { name } if *name == b.name));
                            ui.horizontal(|ui| {
                                if eye(ui, visible) {
                                    if visible {
                                        app.ui.hidden_bodies.push(b.name.clone());
                                    } else {
                                        app.ui.hidden_bodies.retain(|n| n != &b.name);
                                    }
                                }
                                if row(ui, "body", &b.name, None, selected).clicked() {
                                    let _ = app.run("select.set", json!({"items": [{"type": "body", "name": b.name}]}));
                                }
                            });
                        }
                    });
                    let sketches: Vec<(u64, String)> = app
                        .session
                        .doc
                        .features
                        .iter()
                        .filter(|f| matches!(f.kind, FeatureKind::Sketch { .. }))
                        .map(|f| (f.id, f.name.clone()))
                        .collect();
                    egui::CollapsingHeader::new(format!("Sketches ({})", sketches.len())).default_open(true).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if eye(ui, app.ui.show_sketches) {
                                app.ui.show_sketches = !app.ui.show_sketches;
                            }
                            ui.label(RichText::new("show unused sketches").size(11.0).color(t.text_dim));
                        });
                        for (id, name) in sketches {
                            let active = app.session.active_sketch == Some(id);
                            let r = if active {
                                row(ui, "sketch", &format!("{name}  (editing)"), Some(t.sketch_accent), true)
                            } else {
                                row(ui, "sketch", &name, None, false)
                            };
                            if r.double_clicked() {
                                app.edit_sketch(id);
                            }
                            r.on_hover_text("Double-click to edit");
                        }
                    });
                });
            });
        });
}
