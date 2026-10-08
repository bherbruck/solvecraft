//! The browser (left): the design as a tree of components, each with its bodies, sketches and
//! construction planes, plus document settings and the origin.
//!
//! Rows are drawn in code (icons from [`icons`]) and act through commands. A click selects
//! (Ctrl adds, Shift extends over the visible rows), hovering a body row lights it up in the
//! viewport, a double-click on a sketch edits it, and a right-click opens the item's menu
//! ([`crate::context_menu`]).

use std::collections::BTreeSet;

use egui::{Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solvecraft_engine::Sel;
use solvecraft_engine::doc::FeatureKind;

use crate::context_menu::{Item, Target};
use crate::theme::Tokens;
use crate::{SolveApp, icons};

/// Browser state kept between frames.
#[derive(Default)]
pub struct TreeState {
    /// Folders closed by the user (keys like `c0/bodies`).
    pub collapsed: BTreeSet<String>,
    /// Folders opened by the user (folders that start closed).
    pub expanded: BTreeSet<String>,
    /// Scroll to this body's row and open its folders (Find in Browser).
    pub reveal: Option<String>,
    /// The Move/Copy panel of an occurrence.
    pub occurrence_move: Option<OccMove>,
    /// The Redefine Sketch Plane panel (sketch id).
    pub redefine: Option<u64>,
    /// Components picked in the tree (they have no viewport selection).
    pub picked_components: Vec<u64>,
    /// The last row clicked (for Shift ranges).
    anchor: Option<String>,
    /// Row keys in drawing order this frame (for Shift ranges).
    order: Vec<String>,
}

/// Moving an occurrence: the offsets typed so far (pending until Capture Position).
#[derive(Clone, Debug)]
pub struct OccMove {
    pub occurrence: u64,
    pub translate: [f64; 3],
    pub angle_deg: f64,
}

impl OccMove {
    pub fn new(occurrence: u64) -> Self {
        OccMove { occurrence, translate: [0.0; 3], angle_deg: 0.0 }
    }
}

// ---------------------------------------------------------------------------------------------
// Queries used by the menus
// ---------------------------------------------------------------------------------------------

/// Is a sketch drawn in the viewport?
pub fn sketch_visible(app: &SolveApp, id: u64) -> bool {
    if app.ui.hidden_sketches.contains(&id) {
        return false;
    }
    app.ui.shown_sketches.contains(&id) || (app.ui.show_sketches && !solvecraft_engine::view::sketch_consumed(&app.session, id))
}

pub fn set_sketch_visible(app: &mut SolveApp, id: u64, on: bool) {
    app.ui.hidden_sketches.retain(|x| *x != id);
    app.ui.shown_sketches.retain(|x| *x != id);
    if on {
        app.ui.shown_sketches.push(id);
    } else {
        app.ui.hidden_sketches.push(id);
    }
}

pub fn redefine_sketch(app: &mut SolveApp, id: u64) {
    app.tree.redefine = Some(id);
}

pub fn group_bodies(_app: &SolveApp, _id: u64) -> Vec<String> {
    Vec::new()
}

pub fn group_items(_app: &SolveApp, _id: u64) -> Vec<Item> {
    Vec::new()
}

pub fn group_name(_app: &SolveApp, _id: u64) -> Option<String> {
    None
}

pub fn group_action(_app: &mut SolveApp, _action: &str, _p: &Value) {}

// ---------------------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------------------

const ROW_H: f32 = 22.0;
const INDENT: f32 = 14.0;

/// One row's look.
#[derive(Default)]
struct Row<'a> {
    depth: usize,
    /// Some(open) for rows that fold.
    fold: Option<bool>,
    /// Some(visible) for rows with an eye.
    eye: Option<bool>,
    icon: &'a str,
    label: &'a str,
    selected: bool,
    dim: bool,
    color: Option<Color32>,
    /// Component rows: is it the active one?
    radio: Option<bool>,
    /// Trailing badges: "lock", "pin".
    badges: &'a [&'a str],
}

/// What happened on a row.
#[derive(Default)]
struct RowResp {
    rect: Option<Rect>,
    clicked: bool,
    double: bool,
    secondary: Option<Pos2>,
    fold: bool,
    eye: bool,
    radio: bool,
    hovered: bool,
}

fn draw_row(ui: &mut egui::Ui, id: egui::Id, row: &Row) -> RowResp {
    let t = Tokens::get();
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let mut out = RowResp { rect: Some(r), hovered: resp.hovered(), ..Default::default() };
    if row.selected {
        ui.painter().rect_filled(r, 3.0, t.accent_soft);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 3.0, t.hover);
    }
    let mut x = r.left() + 2.0 + row.depth as f32 * INDENT;
    let cy = r.center().y;
    let ink = if row.dim { t.text_dim } else { t.icon };
    // Fold arrow.
    if let Some(open) = row.fold {
        let ar = Rect::from_center_size(pos2(x + 6.0, cy), vec2(12.0, 12.0));
        let pts = if open {
            vec![pos2(ar.left() + 2.0, ar.top() + 4.0), pos2(ar.right() - 2.0, ar.top() + 4.0), pos2(ar.center().x, ar.bottom() - 3.0)]
        } else {
            vec![pos2(ar.left() + 4.0, ar.top() + 2.0), pos2(ar.right() - 3.0, ar.center().y), pos2(ar.left() + 4.0, ar.bottom() - 2.0)]
        };
        ui.painter().add(egui::Shape::convex_polygon(pts, t.text_dim, Stroke::NONE));
        out.fold = ui.interact(ar.expand(3.0), id.with("fold"), Sense::click()).clicked();
    }
    x += 14.0;
    // Eye.
    if let Some(vis) = row.eye {
        let er = Rect::from_center_size(pos2(x + 8.0, cy), vec2(15.0, 15.0));
        icons::paint(ui.painter(), er, "eye", if vis { ink } else { t.border }, t.icon_fill, t.accent);
        if !vis {
            ui.painter().line_segment([er.left_bottom() + vec2(2.0, -2.0), er.right_top() + vec2(-2.0, 2.0)], Stroke::new(1.4, t.text_dim));
        }
        let e = ui.interact(er.expand(2.0), id.with("eye"), Sense::click());
        out.eye = e.clicked();
        e.on_hover_text(if vis { "Hide" } else { "Show" });
    }
    x += 18.0;
    // Activation radio (components).
    if let Some(active) = row.radio {
        let rc = pos2(x + 6.0, cy);
        ui.painter().circle_stroke(rc, 5.0, Stroke::new(1.2, if active { t.accent } else { t.text_dim }));
        if active {
            ui.painter().circle_filled(rc, 2.8, t.accent);
        }
        let rr = ui.interact(Rect::from_center_size(rc, vec2(14.0, 14.0)), id.with("radio"), Sense::click());
        out.radio = rr.clicked();
        rr.on_hover_text(if active { "Active component" } else { "Activate" });
        x += 15.0;
    }
    if !row.icon.is_empty() {
        let ir = Rect::from_min_size(pos2(x, cy - 8.0), vec2(16.0, 16.0));
        icons::paint(ui.painter(), ir, row.icon, ink, if row.dim { t.panel_header } else { t.icon_fill }, t.accent);
        x += 21.0;
    }
    let col = row.color.unwrap_or(if row.dim { t.text_dim } else { t.text });
    let galley = ui.painter().layout_no_wrap(row.label.to_string(), FontId::proportional(12.5), col);
    let lw = galley.size().x;
    ui.painter().galley(pos2(x, cy - galley.size().y / 2.0), galley, col);
    let mut bx = (x + lw + 6.0).min(r.right() - 16.0);
    for b in row.badges {
        let br = Rect::from_center_size(pos2(bx + 6.0, cy), vec2(12.0, 12.0));
        badge(ui.painter(), br, b, t.text_dim);
        bx += 15.0;
    }
    out.clicked = resp.clicked();
    out.double = resp.double_clicked();
    if resp.secondary_clicked() {
        out.secondary = resp.interact_pointer_pos().or(Some(r.left_bottom()));
    }
    out
}

/// Small glyphs after a row's label: a padlock, a pin (grounded).
fn badge(p: &egui::Painter, r: Rect, name: &str, c: Color32) {
    let s = Stroke::new(1.3, c);
    match name {
        "lock" => {
            let body = Rect::from_min_max(pos2(r.left() + 1.5, r.center().y - 0.5), pos2(r.right() - 1.5, r.bottom() - 0.5));
            p.rect_filled(body, 1.5, c);
            let top = pos2(r.center().x, r.top() + 4.0);
            p.add(egui::Shape::line(
                (0..=8)
                    .map(|k| {
                        let a = std::f32::consts::PI * (k as f32 / 8.0);
                        pos2(top.x - 3.2 * a.cos(), top.y - 3.2 * a.sin() + 1.0)
                    })
                    .chain([pos2(top.x + 3.2, body.top())])
                    .collect(),
                s,
            ));
            p.line_segment([pos2(top.x - 3.2, top.y + 1.0), pos2(top.x - 3.2, body.top())], s);
        }
        "pin" => {
            let head = pos2(r.center().x + 2.0, r.top() + 4.0);
            p.circle_filled(head, 3.2, c);
            p.line_segment([head, pos2(r.left() + 1.5, r.bottom() - 1.0)], Stroke::new(1.6, c));
        }
        _ => {}
    }
}

/// A Ctrl/Shift-aware click on a selectable row.
fn click_select(app: &mut SolveApp, ui: &egui::Ui, key: String, sel: Option<Sel>) {
    let mods = ui.input(|i| i.modifiers);
    if mods.shift
        && let Some(a) = app.tree.anchor.clone()
    {
        let order = app.tree.order.clone();
        if let (Some(i), Some(j)) = (order.iter().position(|k| *k == a), order.iter().position(|k| *k == key)) {
            let (lo, hi) = (i.min(j), i.max(j));
            let items: Vec<Sel> = order[lo..=hi].iter().filter_map(|k| sel_of_key(k)).collect();
            let _ = app.run("select.set", json!({ "items": items, "add": mods.command || mods.ctrl }));
            return;
        }
    }
    app.tree.anchor = Some(key);
    let Some(sel) = sel else { return };
    if mods.command || mods.ctrl {
        if app.session.selection.contains(&sel) {
            let rest: Vec<Sel> = app.session.selection.iter().filter(|x| **x != sel).cloned().collect();
            let _ = app.run("select.set", json!({ "items": rest }));
        } else {
            let _ = app.run("select.set", json!({ "items": [sel], "add": true }));
        }
    } else {
        app.tree.picked_components.clear();
        let _ = app.run("select.set", json!({ "items": [sel] }));
    }
}

fn sel_of_key(k: &str) -> Option<Sel> {
    if let Some(b) = k.strip_prefix("b:") {
        return Some(Sel::Body { name: b.to_string() });
    }
    if let Some(s) = k.strip_prefix("s:") {
        return s.parse().ok().map(|id| Sel::Feature { id });
    }
    None
}

fn is_open(app: &SolveApp, key: &str, default: bool) -> bool {
    if default { !app.tree.collapsed.contains(key) } else { app.tree.expanded.contains(key) }
}

fn toggle(app: &mut SolveApp, key: &str, default: bool) {
    if default {
        if !app.tree.collapsed.remove(key) {
            app.tree.collapsed.insert(key.to_string());
        }
    } else if !app.tree.expanded.remove(key) {
        app.tree.expanded.insert(key.to_string());
    }
}

/// Everything a frame's rows want done, applied after drawing.
#[derive(Default)]
struct Actions {
    menu: Option<(Pos2, Target)>,
    hover_bodies: Vec<String>,
}

pub fn browser(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    app.tree.order.clear();
    let mut acts = Actions::default();
    egui::Panel::left("sc_browser")
        .exact_size(250.0)
        .resizable(false)
        .frame(egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin::symmetric(6, 6)))
        .show(ui, |ui| {
            ui.label(RichText::new("BROWSER").size(11.0).strong().color(t.text_dim));
            ui.add_space(4.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                component_rows(app, ui, 0, 0, &mut acts);
            });
        });
    app.viewport.hover_bodies = acts.hover_bodies;
    if let Some((at, target)) = acts.menu {
        crate::context_menu::open_for(app, at, target);
    }
    occurrence_panel(app, ui.ctx());
    redefine_panel(app, ui.ctx());
}

fn component_name(app: &SolveApp, id: u64) -> String {
    if id == 0 {
        return app.session.doc.name.clone();
    }
    let doc = &app.session.doc;
    let name = doc.components.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_default();
    let n = doc.occurrences.iter().filter(|o| o.component == id).count();
    if n > 1 { format!("{name} ({n} instances)") } else { format!("{name}:1") }
}

fn component_rows(app: &mut SolveApp, ui: &mut egui::Ui, id: u64, depth: usize, acts: &mut Actions) {
    let t = Tokens::get();
    let key = format!("c{id}");
    let open = is_open(app, &key, true);
    let active = app.session.active_component == id;
    let bodies = crate::context_menu::component_bodies(app, id);
    let visible = bodies.is_empty() || bodies.iter().any(|b| !app.ui.hidden_bodies.contains(b));
    let grounded = app.session.doc.occurrence_of(id).is_some_and(|o| o.grounded);
    let faded = app.session.active_component != 0 && !app.session.doc.component_within(id, app.session.active_component) && !active;
    let name = component_name(app, id);
    let badges: &[&str] = if grounded { &["pin"] } else { &[] };
    let row = Row {
        depth,
        fold: Some(open),
        eye: (id != 0).then_some(visible),
        icon: "component",
        label: &name,
        selected: app.tree.picked_components.contains(&id),
        dim: faded,
        radio: Some(active),
        badges,
        ..Default::default()
    };
    let r = draw_row(ui, ui.id().with(("comp", id)), &row);
    if r.fold {
        toggle(app, &key, true);
    }
    if r.radio {
        let _ = app.run("component.activate", json!({ "component": id }));
    }
    if r.eye {
        if visible {
            app.ui.hidden_bodies.extend(bodies.iter().filter(|b| !app.ui.hidden_bodies.contains(b)).cloned().collect::<Vec<_>>());
        } else {
            app.ui.hidden_bodies.retain(|b| !bodies.contains(b));
        }
    }
    if r.clicked {
        let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
        if !ctrl {
            app.tree.picked_components.clear();
            let _ = app.run("select.clear", json!({}));
        }
        if app.tree.picked_components.contains(&id) {
            app.tree.picked_components.retain(|c| *c != id);
        } else {
            app.tree.picked_components.push(id);
        }
    }
    if r.double {
        let _ = app.run("component.activate", json!({ "component": id }));
    }
    if r.hovered {
        acts.hover_bodies = bodies.clone();
    }
    if let Some(p) = r.secondary {
        acts.menu = Some((p, Target::Component { id }));
    }
    if !open {
        return;
    }
    let d = depth + 1;
    if id == 0 {
        doc_settings(app, ui, d);
        origin_rows(app, ui, d);
    }
    let _ = t;
    body_rows(app, ui, id, d, acts);
    sketch_rows(app, ui, id, d, acts);
    plane_rows(app, ui, id, d);
    let children: Vec<u64> = app.session.doc.components.iter().filter(|c| c.parent == id).map(|c| c.id).collect();
    for c in children {
        component_rows(app, ui, c, d, acts);
    }
}

fn doc_settings(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize) {
    let key = "docsettings";
    let open = is_open(app, key, false);
    let r = draw_row(ui, ui.id().with(key), &Row { depth, fold: Some(open), icon: "settings", label: "Document Settings", ..Default::default() });
    if r.fold || r.clicked {
        toggle(app, key, false);
    }
    if !open {
        return;
    }
    let units = format!("Units: {}", app.session.doc.units);
    draw_row(ui, ui.id().with("units"), &Row { depth: depth + 1, icon: "settings", label: &units, ..Default::default() });
    let params = format!("Parameters ({})", app.session.doc.params.len());
    if draw_row(ui, ui.id().with("params"), &Row { depth: depth + 1, icon: "params", label: &params, ..Default::default() }).clicked {
        app.start("ChangeParameterCommand");
    }
}

fn origin_rows(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize) {
    let key = "origin";
    let open = is_open(app, key, false);
    let r = draw_row(
        ui,
        ui.id().with(key),
        &Row { depth, fold: Some(open), eye: Some(app.ui.show_origin), icon: "origin", label: "Origin", ..Default::default() },
    );
    if r.fold || r.clicked {
        toggle(app, key, false);
    }
    if r.eye {
        app.ui.show_origin = !app.ui.show_origin;
    }
    if !open {
        return;
    }
    let items = [
        ("O", "point", "O"),
        ("X", "axis", "X"),
        ("Y", "axis", "Y"),
        ("Z", "axis", "Z"),
        ("XY", "plane", "XY"),
        ("XZ", "plane", "XZ"),
        ("YZ", "plane", "YZ"),
    ];
    for (k, icon, label) in items {
        let visible = !app.ui.hidden_origin.iter().any(|h| h == k);
        let sel = match icon {
            "plane" => Some(Sel::Plane { name: k.into() }),
            "axis" => Some(Sel::Axis { name: k.into() }),
            _ => None,
        };
        let selected = sel.as_ref().is_some_and(|x| app.session.selection.contains(x));
        let r = draw_row(
            ui,
            ui.id().with(("o", k)),
            &Row { depth: depth + 1, eye: Some(visible), icon, label, selected, dim: !app.ui.show_origin, ..Default::default() },
        );
        if r.eye {
            if visible {
                app.ui.hidden_origin.push(k.into());
            } else {
                app.ui.hidden_origin.retain(|h| h != k);
            }
        }
        if r.clicked
            && let Some(x) = sel
        {
            pick_from_browser(app, x);
        }
    }
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

/// A folder row (Bodies, Sketches, Construction) with an eye for everything in it; true when
/// it is open.
fn folder(app: &mut SolveApp, ui: &mut egui::Ui, key: &str, label: &str, depth: usize, visible: Option<bool>) -> (bool, RowResp) {
    let open = is_open(app, key, true);
    let r = draw_row(ui, ui.id().with(key), &Row { depth, fold: Some(open), eye: visible, icon: "folder", label, ..Default::default() });
    if r.fold || r.clicked {
        toggle(app, key, true);
    }
    (open, r)
}

fn body_rows(app: &mut SolveApp, ui: &mut egui::Ui, comp: u64, depth: usize, acts: &mut Actions) {
    let st = app.session.model.state();
    let doc = app.session.doc.clone();
    let bodies: Vec<String> = st.bodies.iter().filter(|b| doc.body_component(&b.name, b.feature) == comp).map(|b| b.name.clone()).collect();
    if bodies.is_empty() {
        return;
    }
    let key = format!("c{comp}/bodies");
    let reveal = app.tree.reveal.as_ref().is_some_and(|b| bodies.contains(b));
    if reveal {
        app.tree.collapsed.remove(&key);
        app.tree.collapsed.remove(&format!("c{comp}"));
    }
    let any_visible = bodies.iter().any(|b| !app.ui.hidden_bodies.contains(b));
    let (open, r) = folder(app, ui, &key, "Bodies", depth, Some(any_visible));
    if r.eye {
        if any_visible {
            app.ui.hidden_bodies.extend(bodies.iter().filter(|b| !app.ui.hidden_bodies.contains(b)).cloned().collect::<Vec<_>>());
        } else {
            app.ui.hidden_bodies.retain(|b| !bodies.contains(b));
        }
    }
    if r.hovered {
        acts.hover_bodies = bodies.clone();
    }
    if !open {
        return;
    }
    for b in &bodies {
        let visible = !app.ui.hidden_bodies.contains(b);
        let selected = app.session.selection.iter().any(|x| matches!(x, Sel::Body { name } if name == b));
        let locked = app.ui.locked_bodies.contains(b);
        let badges: &[&str] = if locked { &["lock"] } else { &[] };
        let key = format!("b:{b}");
        app.tree.order.push(key.clone());
        let r = draw_row(
            ui,
            ui.id().with(("body", b)),
            &Row { depth: depth + 1, eye: Some(visible), icon: "body", label: b, selected, dim: !visible, badges, ..Default::default() },
        );
        if app.tree.reveal.as_deref() == Some(b.as_str())
            && let Some(rect) = r.rect
        {
            ui.scroll_to_rect(rect, Some(egui::Align::Center));
            app.tree.reveal = None;
        }
        if r.eye {
            if visible {
                app.ui.hidden_bodies.push(b.clone());
            } else {
                app.ui.hidden_bodies.retain(|n| n != b);
            }
        }
        if r.clicked {
            click_select(app, ui, key, Some(Sel::Body { name: b.clone() }));
        }
        if r.hovered {
            acts.hover_bodies = vec![b.clone()];
        }
        if let Some(p) = r.secondary {
            if !selected {
                let _ = app.run("select.set", json!({"items": [{"type": "body", "name": b}]}));
            }
            acts.menu = Some((p, Target::Body { name: b.clone() }));
        }
    }
}

fn sketch_rows(app: &mut SolveApp, ui: &mut egui::Ui, comp: u64, depth: usize, acts: &mut Actions) {
    let t = Tokens::get();
    let sketches: Vec<(u64, String)> = app
        .session
        .doc
        .features
        .iter()
        .filter(|f| f.component == comp && matches!(f.kind, FeatureKind::Sketch { .. }))
        .map(|f| (f.id, f.name.clone()))
        .collect();
    if sketches.is_empty() {
        return;
    }
    let key = format!("c{comp}/sketches");
    let (open, r) = folder(app, ui, &key, "Sketches", depth, Some(app.ui.show_sketches));
    if r.eye {
        app.ui.show_sketches = !app.ui.show_sketches;
    }
    if !open {
        return;
    }
    for (id, name) in sketches {
        let active = app.session.active_sketch == Some(id);
        let visible = sketch_visible(app, id);
        let selected = active || app.session.selection.iter().any(|x| matches!(x, Sel::Feature { id: f } if *f == id));
        let label = if active { format!("{name}  (editing)") } else { name.clone() };
        let key = format!("s:{id}");
        app.tree.order.push(key.clone());
        let r = draw_row(
            ui,
            ui.id().with(("sketch", id)),
            &Row {
                depth: depth + 1,
                eye: Some(visible),
                icon: "sketch",
                label: &label,
                selected,
                dim: !visible,
                color: active.then_some(t.sketch_accent),
                ..Default::default()
            },
        );
        if r.eye {
            set_sketch_visible(app, id, !visible);
        }
        if r.double {
            app.edit_sketch(id);
        } else if r.clicked {
            click_select(app, ui, key, Some(Sel::Feature { id }));
        }
        if let Some(p) = r.secondary {
            acts.menu = Some((p, Target::Sketch { id }));
        }
    }
}

fn plane_rows(app: &mut SolveApp, ui: &mut egui::Ui, comp: u64, depth: usize) {
    let planes: Vec<String> = app
        .session
        .doc
        .features
        .iter()
        .filter(|f| f.component == comp && matches!(f.kind, FeatureKind::ConstructionPlane { .. }))
        .map(|f| f.name.clone())
        .collect();
    if planes.is_empty() {
        return;
    }
    let key = format!("c{comp}/construction");
    let (open, _) = folder(app, ui, &key, "Construction", depth, None);
    if !open {
        return;
    }
    for p in planes {
        let visible = !app.ui.hidden_origin.contains(&p);
        let sel = Sel::Plane { name: p.clone() };
        let selected = app.session.selection.contains(&sel);
        let r = draw_row(
            ui,
            ui.id().with(("plane", &p)),
            &Row { depth: depth + 1, eye: Some(visible), icon: "plane", label: &p, selected, dim: !visible, ..Default::default() },
        );
        if r.eye {
            if visible {
                app.ui.hidden_origin.push(p.clone());
            } else {
                app.ui.hidden_origin.retain(|h| *h != p);
            }
        }
        if r.clicked {
            pick_from_browser(app, sel);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Panels opened from the menus
// ---------------------------------------------------------------------------------------------

/// Move/Copy of an occurrence: offsets and a rotation about Z, pending until Capture Position.
fn occurrence_panel(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut m) = app.tree.occurrence_move.clone() else { return };
    let name = app.session.doc.occurrences.iter().find(|o| o.id == m.occurrence).map(|o| o.name.clone()).unwrap_or_default();
    let mut open = true;
    let mut action: Option<&str> = None;
    let before = (m.translate, m.angle_deg);
    egui::Window::new(format!("Move: {name}")).id(egui::Id::new("sc_occ_move")).open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
        egui::Grid::new("sc_occ_grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            for (k, axis) in ["X distance", "Y distance", "Z distance"].iter().enumerate() {
                ui.label(*axis);
                if let Some(v) = m.translate.get_mut(k) {
                    ui.add(egui::DragValue::new(v).speed(0.5).suffix(" mm"));
                }
                ui.end_row();
            }
            ui.label("Z angle");
            ui.add(egui::DragValue::new(&mut m.angle_deg).speed(1.0).suffix(" deg"));
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Capture Position").clicked() {
                action = Some("capture");
            }
            if ui.button("Revert").clicked() {
                action = Some("revert");
            }
        });
    });
    if (m.translate, m.angle_deg) != before {
        // The pending move is the typed offset from where the occurrence is now.
        let _ = app.session.execute("AsBuiltPositionsCmd", &json!({}));
        let _ = app.run(
            "occurrence.move",
            json!({ "occurrence": m.occurrence, "translate": m.translate, "axis": [0, 0, 1], "angle": m.angle_deg.to_radians(), "capture": false }),
        );
    }
    match action {
        Some("capture") => {
            let _ = app.run("SnapshotCmd", json!({}));
            app.tree.occurrence_move = None;
        }
        Some(_) => {
            let _ = app.run("AsBuiltPositionsCmd", json!({}));
            app.tree.occurrence_move = None;
        }
        None if !open => {
            let _ = app.run("AsBuiltPositionsCmd", json!({}));
            app.tree.occurrence_move = None;
        }
        None => app.tree.occurrence_move = Some(m),
    }
}

/// Redefine Sketch Plane: pick an origin or construction plane, or the selected planar face.
fn redefine_panel(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(id) = app.tree.redefine else { return };
    let name = app.session.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
    let mut planes: Vec<String> = vec!["XY".into(), "XZ".into(), "YZ".into()];
    planes.extend(app.session.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::ConstructionPlane { .. })).map(|f| f.name.clone()));
    let face = app.session.selection.iter().find_map(|s| if let Sel::Face { point, .. } = s { Some(*point) } else { None });
    let mut open = true;
    let mut pick: Option<Value> = None;
    egui::Window::new(format!("Redefine Sketch Plane: {name}"))
        .id(egui::Id::new("sc_redefine"))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(RichText::new("Move the sketch to").color(Tokens::get().text_dim));
            ui.horizontal_wrapped(|ui| {
                for p in &planes {
                    if ui.button(p).clicked() {
                        pick = Some(json!(p));
                    }
                }
            });
            ui.add_space(4.0);
            let b = ui.add_enabled(face.is_some(), egui::Button::new("Selected face"));
            if b.clicked()
                && let Some(f) = face
            {
                pick = Some(json!({ "face": [f.x, f.y, f.z] }));
            }
            b.on_disabled_hover_text("Select a planar face in the viewport first");
        });
    if let Some(plane) = pick {
        if app.run("sketch.redefine", json!({ "sketch": id, "plane": plane })).is_ok() {
            app.tree.redefine = None;
        }
    } else if !open {
        app.tree.redefine = None;
    }
}
