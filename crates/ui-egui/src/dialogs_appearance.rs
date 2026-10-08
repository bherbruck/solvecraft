//! The Appearance dialog: library swatches (named looks), the looks in this design, a colour
//! picker with opacity, and the objects to apply to (bodies, or faces). A swatch dragged onto a
//! body or face in the viewport, or onto a body's row in the Browser, applies at once; Apply
//! (and OK) gives the picked objects the current look.

use egui::{Color32, RichText, Sense, Stroke, vec2};
use serde_json::{Value, json};
use solvecraft_engine::Sel;

use crate::SolveApp;
use crate::dialogs::{Kind, combo, row_label};
use crate::selection::{BODIES, FACES, SelInput};
use crate::theme::Tokens;
use crate::viewport::Hit;

/// A named look: colour and opacity.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub name: String,
    pub category: String,
    pub color: [u8; 3],
    pub opacity: f32,
}

/// Built-in looks, used until the engine offers a library (`appearance.library`).
fn builtin() -> Vec<Look> {
    let l = |cat: &str, name: &str, c: [u8; 3], o: f32| Look { name: name.into(), category: cat.into(), color: c, opacity: o };
    vec![
        l("Metal", "Aluminum - Satin", [196, 199, 204], 1.0),
        l("Metal", "Steel - Brushed", [160, 164, 170], 1.0),
        l("Metal", "Stainless Steel", [182, 186, 190], 1.0),
        l("Metal", "Brass", [196, 160, 80], 1.0),
        l("Metal", "Copper", [184, 115, 72], 1.0),
        l("Metal", "Chrome", [222, 226, 232], 1.0),
        l("Metal", "Anodized Black", [42, 44, 48], 1.0),
        l("Plastic", "ABS - White", [236, 236, 232], 1.0),
        l("Plastic", "ABS - Black", [34, 34, 36], 1.0),
        l("Plastic", "ABS - Grey", [128, 130, 134], 1.0),
        l("Plastic", "Polycarbonate - Clear", [210, 226, 240], 0.35),
        l("Plastic", "Nylon - Natural", [228, 222, 204], 1.0),
        l("Plastic", "Rubber - Black", [26, 26, 26], 1.0),
        l("Paint", "Paint - Red", [196, 40, 36], 1.0),
        l("Paint", "Paint - Orange", [232, 120, 30], 1.0),
        l("Paint", "Paint - Yellow", [236, 196, 40], 1.0),
        l("Paint", "Paint - Green", [52, 140, 72], 1.0),
        l("Paint", "Paint - Blue", [36, 92, 180], 1.0),
        l("Paint", "Paint - Purple", [112, 64, 156], 1.0),
        l("Wood", "Oak", [190, 150, 100], 1.0),
        l("Wood", "Walnut", [110, 76, 50], 1.0),
        l("Glass", "Glass - Clear", [220, 236, 244], 0.25),
        l("Glass", "Glass - Smoked", [90, 96, 104], 0.5),
    ]
}

/// The library: the engine's when it has one, else the built-in looks.
pub fn library(app: &mut SolveApp) -> Vec<Look> {
    let from_engine = app.session.execute("appearance.library", &json!({})).ok().and_then(|v| {
        let list = v.get("appearances").or(v.get("library")).unwrap_or(&v).as_array()?.clone();
        let looks: Vec<Look> = list
            .iter()
            .filter_map(|x| {
                Some(Look {
                    name: x.get("name")?.as_str()?.to_string(),
                    category: x.get("category").and_then(Value::as_str).unwrap_or("Library").to_string(),
                    color: parse_color(x.get("color")?)?,
                    opacity: x.get("opacity").and_then(Value::as_f64).unwrap_or(1.0) as f32,
                })
            })
            .collect();
        (!looks.is_empty()).then_some(looks)
    });
    from_engine.unwrap_or_else(builtin)
}

/// "#rrggbb" or [r, g, b].
pub fn parse_color(v: &Value) -> Option<[u8; 3]> {
    if let Some(s) = v.as_str() {
        let h = s.trim().trim_start_matches('#');
        if h.len() != 6 {
            return None;
        }
        let byte = |i: usize| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok());
        return Some([byte(0)?, byte(2)?, byte(4)?]);
    }
    let a = v.as_array()?;
    let c = |i: usize| a.get(i).and_then(Value::as_u64).filter(|x| *x <= 255).map(|x| x as u8);
    Some([c(0)?, c(1)?, c(2)?])
}

pub fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

#[derive(Clone, Debug)]
pub struct Ap {
    /// Apply to faces instead of bodies.
    pub faces: bool,
    pub color: [u8; 3],
    pub opacity: f32,
    /// The look picked last (its name), if the colour came from one.
    pub look: Option<String>,
    /// Library category shown (0: all).
    pub category: usize,
    /// What the last drop or Apply did.
    pub note: Option<String>,
}

/// The dialog for the Appearance command.
pub fn start(_app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "AppearanceCommand").then(|| {
        (
            Kind::Appearance(Ap { faces: false, color: [196, 199, 204], opacity: 1.0, look: None, category: 0, note: None }),
            vec![SelInput::new("Objects", BODIES, true)],
        )
    })
}

/// The command giving `targets` (bodies or faces) a look.
fn apply_params(targets: &[Sel], color: [u8; 3], opacity: f32, look: Option<&str>) -> Option<Value> {
    let bodies: Vec<String> = targets.iter().filter_map(|x| if let Sel::Body { name } = x { Some(name.clone()) } else { None }).collect();
    let faces: Vec<Value> = targets
        .iter()
        .filter_map(|x| if let Sel::Face { body, point, .. } = x { Some(json!({"body": body, "point": [point.x, point.y, point.z]})) } else { None })
        .collect();
    if bodies.is_empty() && faces.is_empty() {
        return None;
    }
    let mut p = json!({"color": hex(color)});
    if !bodies.is_empty() {
        p["bodies"] = json!(bodies);
    }
    if !faces.is_empty() {
        p["faces"] = json!(faces);
        // An engine without face looks: their bodies.
        if bodies.is_empty() && solvecraft_engine::find_command("appearance.list").is_none() {
            let mut owners: Vec<String> =
                targets.iter().filter_map(|x| if let Sel::Face { body, .. } = x { Some(body.clone()) } else { None }).collect();
            owners.dedup();
            p["bodies"] = json!(owners);
        }
    }
    if opacity < 0.999 {
        p["opacity"] = json!((opacity * 1000.0).round() / 1000.0);
    }
    if let Some(n) = look {
        p["appearance"] = json!(n);
    }
    Some(p)
}

/// The commands OK runs: the current look on the picked objects (nothing when none are picked).
pub fn commands(k: &Ap, inputs: &[SelInput]) -> Vec<(String, Value)> {
    let items = inputs.first().map(|i| i.items.as_slice()).unwrap_or(&[]);
    apply_params(items, k.color, k.opacity, k.look.as_deref()).map(|p| vec![("AppearanceCommand".to_string(), p)]).unwrap_or_default()
}

/// What a swatch dropped now would land on: the face or body under the cursor in the viewport,
/// or the bodies of the Browser row under it.
fn drop_target(app: &SolveApp, faces: bool) -> Vec<Sel> {
    match &app.viewport.hover {
        Some(Hit::Face { body, index, point }) if faces => vec![Sel::Face { body: body.clone(), index: *index, point: *point }],
        Some(Hit::Face { body, .. } | Hit::Edge { body, .. } | Hit::Vertex { body, .. }) => vec![Sel::Body { name: body.clone() }],
        _ => app.viewport.hover_bodies.iter().map(|n| Sel::Body { name: n.clone() }).collect(),
    }
}

/// The looks in this design (colour and how many items wear it): the engine's list when it has
/// one, else the body colours.
fn in_design(app: &mut SolveApp) -> Vec<([u8; 3], usize)> {
    if let Ok(v) = app.session.execute("appearance.list", &json!({}))
        && let Some(list) = v.get("in_design").and_then(Value::as_array)
    {
        let count = |x: &Value, k: &str| x.get(k).and_then(Value::as_array).map_or(0, Vec::len);
        return list
            .iter()
            .filter_map(|x| Some((parse_color(x.get("color")?)?, count(x, "bodies") + count(x, "faces") + count(x, "components"))))
            .collect();
    }
    let mut out: Vec<([u8; 3], usize)> = Vec::new();
    for c in app.session.doc.appearances.bodies.values().map(|l| l.color) {
        match out.iter_mut().find(|(x, _)| *x == c) {
            Some(e) => e.1 += 1,
            None => out.push((c, 1)),
        }
    }
    out
}

fn c32(c: [u8; 3], opacity: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (opacity.clamp(0.0, 1.0) * 255.0) as u8)
}

/// A swatch: a rounded square of the look; returns its response (click, drag).
fn swatch(ui: &mut egui::Ui, id: egui::Id, color: [u8; 3], opacity: f32, picked: bool, tip: &str, payload: Look) -> egui::Response {
    let t = Tokens::get();
    let r = ui
        .dnd_drag_source(id, payload, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
            // A checker behind see-through looks.
            if opacity < 0.999 {
                let h = rect.width() / 2.0;
                for (dx, dy) in [(0.0, 0.0), (h, h)] {
                    ui.painter().rect_filled(egui::Rect::from_min_size(rect.min + vec2(dx, dy), vec2(h, h)), 0.0, Color32::from_gray(200));
                }
            }
            ui.painter().rect(
                rect,
                5.0,
                c32(color, opacity),
                Stroke::new(if picked { 2.0 } else { 1.0 }, if picked { t.accent } else { t.border }),
                egui::StrokeKind::Inside,
            );
        })
        .response;
    let r = ui.interact(r.rect, id.with("click"), Sense::click()).on_hover_text(tip);
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    r
}

/// Swatches six to a row.
fn swatch_rows<T>(ui: &mut egui::Ui, items: &[T], mut each: impl FnMut(&mut egui::Ui, &T)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for row in items.chunks(6) {
            ui.horizontal(|ui| {
                for x in row {
                    each(ui, x);
                }
            });
        }
    });
}

/// The dialog's rows (after the Objects input); applies drops and Apply at once.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, k: &mut Ap, inputs: &mut [SelInput]) -> bool {
    let t = Tokens::get();
    // Apply To: bodies or faces (the input follows).
    row_label(ui, "Apply To");
    let mut mode = usize::from(k.faces);
    combo(ui, "ap_mode", &["Bodies/Components", "Faces"], &mut mode);
    ui.end_row();
    if (mode == 1) != k.faces {
        k.faces = mode == 1;
        if let Some(i) = inputs.first_mut() {
            *i = SelInput::new("Objects", if k.faces { FACES } else { BODIES }, true);
        }
    }
    // The current look: colour and opacity.
    row_label(ui, "Colour");
    ui.horizontal(|ui| {
        let mut rgb = k.color;
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            k.color = rgb;
            k.look = None;
        }
        ui.label(RichText::new(k.look.clone().unwrap_or_else(|| hex(k.color))).color(t.text_dim));
    });
    ui.end_row();
    row_label(ui, "Opacity");
    ui.spacing_mut().slider_width = 110.0;
    ui.add(egui::Slider::new(&mut k.opacity, 0.05..=1.0).fixed_decimals(2));
    ui.end_row();
    row_label(ui, "");
    let picked: Vec<Sel> = inputs.first().map(|i| i.items.clone()).unwrap_or_default();
    if ui.add_enabled(!picked.is_empty(), egui::Button::new("Apply to selection")).clicked()
        && let Some(p) = apply_params(&picked, k.color, k.opacity, k.look.as_deref())
    {
        k.note = Some(match app.run("AppearanceCommand", p) {
            Ok(_) => format!("applied to {} item(s)", picked.len()),
            Err(e) => e,
        });
    }
    ui.end_row();
    // In this design.
    let used = in_design(app);
    if !used.is_empty() {
        row_label(ui, "In This Design");
        let items: Vec<(usize, &([u8; 3], usize))> = used.iter().enumerate().collect();
        swatch_rows(ui, &items, |ui, (i, (c, n))| {
            let look = Look { name: hex(*c), category: "In This Design".into(), color: *c, opacity: 1.0 };
            if swatch(ui, egui::Id::new(("ap_used", *i)), *c, 1.0, k.color == *c, &format!("{} · {n} item(s)", hex(*c)), look).clicked() {
                k.color = *c;
                k.look = None;
            }
        });
        ui.end_row();
    }
    // The library, by category.
    let lib = library(app);
    let mut cats: Vec<String> = vec!["All".into()];
    for l in &lib {
        if !cats.contains(&l.category) {
            cats.push(l.category.clone());
        }
    }
    let refs: Vec<&str> = cats.iter().map(String::as_str).collect();
    row_label(ui, "Library");
    combo(ui, "ap_cat", &refs, &mut k.category);
    ui.end_row();
    row_label(ui, "");
    let want = cats.get(k.category).cloned().unwrap_or_default();
    let shown: Vec<(usize, &Look)> = lib.iter().enumerate().filter(|(_, l)| k.category == 0 || l.category == want).collect();
    swatch_rows(ui, &shown, |ui, (i, l)| {
        let picked_look = k.look.as_deref() == Some(l.name.as_str());
        if swatch(ui, egui::Id::new(("ap_lib", *i)), l.color, l.opacity, picked_look, &format!("{} (drag onto a body or face)", l.name), (*l).clone())
            .clicked()
        {
            k.color = l.color;
            k.opacity = l.opacity;
            k.look = Some(l.name.clone());
        }
    });
    ui.end_row();
    // A swatch dropped on the model or the Browser.
    let released = ui.input(|i| i.pointer.any_released());
    if released && let Some(look) = egui::DragAndDrop::take_payload::<Look>(ui.ctx()) {
        let targets = drop_target(app, k.faces);
        k.color = look.color;
        k.opacity = look.opacity;
        k.look = (look.category != "In This Design").then(|| look.name.clone());
        if let Some(p) = apply_params(&targets, look.color, look.opacity, k.look.as_deref()) {
            k.note = Some(match app.run("AppearanceCommand", p) {
                Ok(_) => format!("{} on {}", look.name, target_names(&targets)),
                Err(e) => e,
            });
        }
    }
    if let Some(n) = &k.note {
        row_label(ui, "");
        ui.label(RichText::new(n.as_str()).color(t.text_dim));
        ui.end_row();
    }
    false
}

fn target_names(targets: &[Sel]) -> String {
    targets
        .iter()
        .map(|x| match x {
            Sel::Body { name } => name.clone(),
            Sel::Face { body, .. } => format!("a face of {body}"),
            _ => String::new(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use solvecraft_engine::Session;
    use solvecraft_engine::geom::Vec3;

    use super::*;
    use crate::Services;
    use crate::dialogs::apply_commands;

    fn app() -> SolveApp {
        let mut s = Session::default();
        s.execute("PrimitiveBox", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        s.execute("PrimitiveBox", &json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0]})).unwrap();
        SolveApp::new(s, Services::default())
    }

    #[test]
    fn colours_parse_both_ways() {
        assert_eq!(parse_color(&json!("#0a1B2c")), Some([10, 27, 44]));
        assert_eq!(parse_color(&json!([1, 2, 3])), Some([1, 2, 3]));
        assert_eq!(parse_color(&json!([1, 2, 300])), None);
        assert_eq!(parse_color(&json!("#12345")), None);
        assert_eq!(hex([10, 27, 44]), "#0a1b2c");
    }

    /// Pre-selected bodies are the objects; OK gives them the picked look.
    #[test]
    fn ok_applies_the_look_to_the_objects() {
        let mut a = app();
        a.session.selection = vec![Sel::Body { name: "Body1".into() }, Sel::Body { name: "Body2".into() }];
        a.start("AppearanceCommand");
        let mut d = a.dialog.clone().unwrap();
        assert_eq!(d.inputs[0].items.len(), 2);
        if let Kind::Appearance(k) = &mut d.kind {
            k.color = [196, 40, 36];
            k.look = Some("Paint - Red".into());
        }
        let c = apply_commands(&a, &d).unwrap();
        assert_eq!(c[0].0, "AppearanceCommand");
        assert_eq!(c[0].1["color"], "#c42824");
        assert_eq!(c[0].1["bodies"], json!(["Body1", "Body2"]));
        for (id, p) in c {
            a.run(&id, p).unwrap();
        }
        assert_eq!(a.session.doc.appearances.bodies.get("Body2").map(|l| l.color), Some([196, 40, 36]));
        assert_eq!(in_design(&mut a), vec![([196, 40, 36], 2)]);
    }

    /// A swatch dropped on a face in face mode targets the face; otherwise its body.
    #[test]
    fn drops_land_on_what_is_under_the_cursor() {
        let mut a = app();
        let p = Vec3::new(5.0, 5.0, 10.0);
        a.viewport.hover = Some(Hit::Face { body: "Body1".into(), index: 0, point: p });
        assert_eq!(drop_target(&a, false), vec![Sel::Body { name: "Body1".into() }]);
        assert_eq!(drop_target(&a, true), vec![Sel::Face { body: "Body1".into(), index: 0, point: p }]);
        a.viewport.hover = None;
        a.viewport.hover_bodies = vec!["Body2".into()];
        assert_eq!(drop_target(&a, false), vec![Sel::Body { name: "Body2".into() }]);
        let face = apply_params(&[Sel::Face { body: "Body1".into(), index: 0, point: p }], [1, 2, 3], 0.5, None).unwrap();
        assert_eq!(face["faces"][0]["body"], "Body1");
        assert_eq!(face["opacity"], 0.5);
        assert!(!library(&mut a).is_empty());
    }
}
