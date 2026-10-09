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

thread_local! {
    /// The rows drawn this frame: (label, row, fold arrow area, open). For automation
    /// (`ui.at {browser | chevron}`, `ui.browser`).
    static DRAWN: std::cell::RefCell<Vec<(String, Rect, Option<Rect>, Option<bool>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The rows on screen: (label, row rect, fold arrow area, open).
pub fn drawn_rows() -> Vec<(String, Rect, Option<Rect>, Option<bool>)> {
    DRAWN.with(|d| d.borrow().clone())
}

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
    /// Canvases picked in the tree.
    pub picked_canvases: Vec<u64>,
    /// The Edit Canvas / Calibrate panel.
    pub canvas_panel: Option<CanvasPanel>,
    /// What waits for the Capture Position question: a command id, or [`SAVE`].
    pub capture_prompt: Option<String>,
    /// The last row clicked (for Shift ranges).
    anchor: Option<String>,
    /// Row keys in drawing order this frame (for Shift ranges).
    order: Vec<String>,
}

/// Moving an occurrence: the offsets typed so far (pending until Capture Position).
#[derive(Clone, Debug)]
pub struct OccMove {
    pub occurrence: u64,
    /// Distances along X, Y, Z (mm) and turns about X, Y, Z (degrees) from where it was.
    pub translate: [f64; 3],
    pub angles: [f64; 3],
    /// The occurrence's origin when the move began: what it turns about.
    pub pivot: Option<solvecraft_engine::geom::Vec3>,
    /// The values when the current triad drag began.
    drag0: Option<([f64; 3], [f64; 3])>,
}

impl OccMove {
    pub fn new(occurrence: u64) -> Self {
        OccMove { occurrence, translate: [0.0; 3], angles: [0.0; 3], pivot: None, drag0: None }
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

fn find_group(app: &SolveApp, id: u64) -> Option<&solvecraft_engine::doc::BrowserGroup> {
    app.session.doc.browser_groups.iter().find(|g| g.id == id)
}

/// The bodies in a group (none for sketch and plane groups).
pub fn group_bodies(app: &SolveApp, id: u64) -> Vec<String> {
    let st = app.session.world_state();
    find_group(app, id)
        .filter(|g| g.folder == "bodies")
        .map(|g| g.items.iter().filter(|b| st.body(b).is_some()).cloned().collect())
        .unwrap_or_default()
}

pub fn group_name(app: &SolveApp, id: u64) -> Option<String> {
    find_group(app, id).map(|g| g.name.clone())
}

/// The menu of a group row.
pub fn group_items(app: &SolveApp, id: u64) -> Vec<Item> {
    let Some(g) = find_group(app, id) else { return Vec::new() };
    let mut v = vec![
        Item::action("ui.rename", "Rename", "").with(json!({ "group": id })),
        Item::action("ui.ungroup", "Ungroup", "folder").with(json!({ "group": id })),
    ];
    if g.folder == "bodies" {
        let bodies = group_bodies(app, id);
        let hidden = !bodies.is_empty() && bodies.iter().all(|b| app.ui.hidden_bodies.contains(b));
        v.push(Item::sep());
        v.push(Item::action(if hidden { "ui.show" } else { "ui.hide" }, "Show/Hide", "eye").key("V").with(json!({ "bodies": bodies })));
        v.push(Item::action("ui.isolate", "Isolate", "").with(json!({ "bodies": bodies })));
    }
    v
}

/// The menu of a folder row (Bodies, Sketches, Construction).
pub fn folder_items(app: &SolveApp, component: u64, folder: &str) -> Vec<Item> {
    let sel: Vec<String> = selected_keys(app, component, folder);
    let all = entries(app, component, folder);
    let mut v = Vec::new();
    if folder == "bodies" {
        let bodies: Vec<String> = all.iter().map(|e| e.key.clone()).collect();
        v.push(Item::action("component.from_bodies", "Create Components from Bodies", "component").with(json!({ "bodies": bodies })));
    }
    v.push(
        Item::action("ui.newGroup", if folder == "sketches" { "New Sketch Group" } else { "New Group" }, "folder")
            .with(json!({ "component": component, "folder": folder })),
    );
    if sel.len() > 1 {
        v.push(Item::action("ui.groupSelected", "Group Selected", "folder").with(json!({ "component": component, "folder": folder, "items": sel })));
    }
    v.push(Item::sep());
    v.push(Item::action("ui.folderVisible", "Show/Hide", "eye").key("V").with(json!({ "component": component, "folder": folder })));
    v.push(Item::action("ui.showAll", "Show All", "eye"));
    v
}

/// The menu of the Origin folder.
pub fn origin_items(app: &SolveApp) -> Vec<Item> {
    let planes = ["XY", "XZ", "YZ"].iter().all(|p| !app.ui.hidden_origin.iter().any(|h| h == p));
    let axes = ["X", "Y", "Z"].iter().all(|p| !app.ui.hidden_origin.iter().any(|h| h == p));
    vec![
        Item::action("ui.origin", "Show/Hide", "eye").key("V"),
        Item::action("ui.originAll", "Show All", "eye"),
        Item::action("ui.originPart", if planes { "Hide Planes" } else { "Show Planes" }, "plane")
            .with(json!({ "keys": ["XY", "XZ", "YZ"], "show": !planes })),
        Item::action("ui.originPart", if axes { "Hide Axes" } else { "Show Axes" }, "axis").with(json!({ "keys": ["X", "Y", "Z"], "show": !axes })),
    ]
}

/// Show or hide everything in a component folder.
pub fn toggle_folder(app: &mut SolveApp, component: u64, folder: &str) {
    if folder == "sketches" {
        app.ui.show_sketches = !app.ui.show_sketches;
        return;
    }
    let all = entries(app, component, folder);
    let any = all.iter().any(|e| e.visible);
    let refs: Vec<&Entry> = all.iter().collect();
    set_visible(app, folder, &refs, !any);
}

/// Selected browser items of the folder of a body or sketch, when more than one (for Group
/// Selected in their menus).
pub fn selection_group(app: &SolveApp, target: &Target) -> Option<Value> {
    let (comp, folder) = match target {
        Target::Body { name } => {
            let st = app.session.world_state();
            let b = st.body(name)?;
            (app.session.doc.body_component(name, b.feature), "bodies")
        }
        Target::Sketch { id } => (app.session.doc.feature(*id)?.component, "sketches"),
        _ => return None,
    };
    let sel = selected_keys(app, comp, folder);
    (sel.len() > 1).then(|| json!({ "component": comp, "folder": folder, "items": sel }))
}

/// Group actions from the menus.
pub fn group_action(app: &mut SolveApp, action: &str, p: &Value) {
    match action {
        "ui.newGroup" | "ui.groupSelected" => {
            let folder = p.get("folder").and_then(Value::as_str).unwrap_or("bodies");
            let comp = p.get("component").and_then(Value::as_u64).unwrap_or(0);
            let items = p.get("items").cloned().unwrap_or(json!([]));
            if let Ok(r) = app.run("browser.group", json!({ "component": comp, "folder": folder, "items": items }))
                && let Some(g) = r.get("group").and_then(Value::as_u64)
            {
                app.tree.collapsed.remove(&format!("c{comp}/{folder}"));
                app.tree.collapsed.remove(&format!("g{g}"));
            }
        }
        "ui.ungroup" => {
            if let Some(g) = p.get("group") {
                let _ = app.run("browser.ungroup", json!({ "group": g }));
            }
        }
        "ui.renameGroup" => {
            let _ = app.run("browser.rename_group", p.clone());
        }
        _ => {}
    }
}

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
    /// The row's response (drag and drop).
    resp: Option<egui::Response>,
    /// The press began on the fold arrow, eye or radio (not a drag of the row).
    on_control: bool,
}

fn draw_row(ui: &mut egui::Ui, id: egui::Id, row: &Row) -> RowResp {
    let t = Tokens::get();
    // Rows scrolled out of view only take their space: no widgets, no painting.
    let r = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), ROW_H));
    if !ui.is_rect_visible(r) {
        ui.allocate_space(r.size());
        return RowResp { rect: Some(r), ..Default::default() };
    }
    // A stable id per row (not the layout position), so a press and its release stay on the
    // same row while rows above open or close.
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let resp = ui.interact(r, id, Sense::click_and_drag());
    // While something is dragged (an appearance swatch, drawn under the pointer) `hovered` is
    // off; the row under the pointer still counts, so a drop lands on it.
    let dragged_over = egui::DragAndDrop::has_any_payload(ui.ctx()) && ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| r.contains(p));
    let mut out = RowResp { rect: Some(r), hovered: resp.hovered() || dragged_over, ..Default::default() };
    if row.selected {
        ui.painter().rect_filled(r, 3.0, t.accent_soft);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 3.0, t.hover);
    }
    let mut x = r.left() + 2.0 + row.depth as f32 * INDENT;
    let cy = r.center().y;
    let ink = if row.dim { t.text_dim } else { t.icon };
    // The row is one widget: where a click lands decides what it does (the fold arrow with
    // padding, the eye, the radio, or the row itself), so a click on the arrow never also
    // selects the row or starts a drag.
    let fold_r = Rect::from_min_max(pos2(x - 4.0, r.top()), pos2(x + 15.0, r.bottom()));
    let eye_r = Rect::from_center_size(pos2(x + 14.0 + 8.0, cy), vec2(19.0, r.height()));
    let radio_r = Rect::from_center_size(pos2(x + 32.0 + 6.0, cy), vec2(15.0, r.height()));
    let at = |p: Pos2| -> u8 {
        if row.fold.is_some() && fold_r.contains(p) {
            1
        } else if row.eye.is_some() && eye_r.contains(p) {
            2
        } else if row.radio.is_some() && radio_r.contains(p) {
            3
        } else {
            0
        }
    };
    DRAWN.with(|d| d.borrow_mut().push((row.label.to_string(), r, row.fold.map(|_| fold_r), row.fold)));
    let press_on = ui.input(|i| i.pointer.press_origin()).map_or(0, at);
    let click_on = resp.interact_pointer_pos().map_or(0, at);
    let hover_on = resp.hover_pos().map_or(0, at);
    // Fold arrow.
    if let Some(open) = row.fold {
        let ar = Rect::from_center_size(pos2(x + 6.0, cy), vec2(12.0, 12.0));
        let pts = if open {
            vec![pos2(ar.left() + 2.0, ar.top() + 4.0), pos2(ar.right() - 2.0, ar.top() + 4.0), pos2(ar.center().x, ar.bottom() - 3.0)]
        } else {
            vec![pos2(ar.left() + 4.0, ar.top() + 2.0), pos2(ar.right() - 3.0, ar.center().y), pos2(ar.left() + 4.0, ar.bottom() - 2.0)]
        };
        let ink = if hover_on == 1 { t.text } else { t.text_dim };
        ui.painter().add(egui::Shape::convex_polygon(pts, ink, Stroke::NONE));
        out.fold = resp.clicked() && click_on == 1;
        crate::scenario::publish_handle(&format!("fold:{}", row.label), fold_r.center());
    }
    x += 14.0;
    // Eye.
    if let Some(vis) = row.eye {
        let er = Rect::from_center_size(pos2(x + 8.0, cy), vec2(15.0, 15.0));
        icons::paint(ui.painter(), er, "eye", if vis { ink } else { t.border }, t.icon_fill, t.accent);
        if !vis {
            ui.painter().line_segment([er.left_bottom() + vec2(2.0, -2.0), er.right_top() + vec2(-2.0, 2.0)], Stroke::new(1.4, t.text_dim));
        }
        out.eye = resp.clicked() && click_on == 2;
    }
    x += 18.0;
    // Activation radio (components).
    if let Some(active) = row.radio {
        let rc = pos2(x + 6.0, cy);
        ui.painter().circle_stroke(rc, 5.0, Stroke::new(1.2, if active { t.accent } else { t.text_dim }));
        if active {
            ui.painter().circle_filled(rc, 2.8, t.accent);
        }
        out.radio = resp.clicked() && click_on == 3;
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
    crate::scenario::publish_handle(&format!("row:{}", row.label), r.center());
    let tip = match hover_on {
        1 => Some(if row.fold == Some(true) { "Collapse" } else { "Expand" }),
        2 => Some(if row.eye == Some(true) { "Hide" } else { "Show" }),
        3 => Some(if row.radio == Some(true) { "Active component" } else { "Activate" }),
        _ => None,
    };
    let resp = match tip {
        Some(t) => resp.on_hover_text(t),
        None => resp,
    };
    out.on_control = press_on != 0;
    out.clicked = resp.clicked() && click_on == 0;
    out.double = resp.double_clicked() && click_on == 0;
    out.resp = Some(resp.clone());
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
    drop: Option<DropAction>,
}

pub fn browser(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    app.tree.order.clear();
    DRAWN.with(|d| d.borrow_mut().clear());
    let mut acts = Actions::default();
    egui::Panel::left("sc_browser")
        .exact_size(250.0)
        .resizable(false)
        .frame(egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin::symmetric(6, 6)))
        .show(ui, |ui| {
            ui.label(RichText::new("BROWSER").size(11.0).strong().color(t.text_dim));
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .scroll_source(egui::scroll_area::ScrollSource { scroll_bar: true, mouse_wheel: true, ..egui::scroll_area::ScrollSource::NONE })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    component_rows(app, ui, 0, 0, &mut acts);
                });
        });
    app.viewport.hover_bodies = acts.hover_bodies;
    if let Some(d) = acts.drop {
        apply_drop(app, d);
    }
    if let Some((at, target)) = acts.menu {
        crate::context_menu::open_for(app, at, target);
    }
    occurrence_panel(app, ui);
    capture_prompt(app, ui.ctx());
    canvas_panel(app, ui.ctx());
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
    // Bodies and sketches dropped on a component move into it.
    if let Some(resp) = &r.resp
        && let Some(d) = drop_target(ui, resp, |d: &Drag| d.comp != id && d.folder != "construction", DropMark::Box)
    {
        acts.drop = Some(DropAction::Component { to: id, folder: d.folder, keys: d.keys.clone() });
    }
    if !open {
        return;
    }
    let d = depth + 1;
    if id == 0 {
        doc_settings(app, ui, d);
        named_view_rows(app, ui, d, acts);
        origin_rows(app, ui, d);
        canvas_rows(app, ui, d, acts);
    }
    let _ = t;
    for folder in ["bodies", "sketches", "construction"] {
        folder_rows(app, ui, id, folder, d, acts);
    }
    if id == 0 {
        crate::dialogs_assembly::browser_joints(app, ui, d);
    }
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
    let r = draw_row(ui, ui.id().with("units"), &Row { depth: depth + 1, icon: "settings", label: &units, ..Default::default() });
    if let Some(p) = r.secondary.or(r.clicked.then(|| r.rect.map(|x| x.left_bottom())).flatten()) {
        crate::context_menu::open_for(app, p, Target::Units);
    }
    let prec = format!(
        "Precision: {} {}, {}°",
        format_args!("{:.*}", usize::from(app.preferences.length_decimals), 0.0),
        crate::prefs::unit_scale(&app.session.doc.units).1,
        format_args!("{:.*}", usize::from(app.preferences.angle_decimals), 0.0)
    );
    if draw_row(ui, ui.id().with("precision"), &Row { depth: depth + 1, icon: "settings", label: &prec, ..Default::default() }).clicked {
        app.prefs_window.open = true;
    }
    let params = format!("Parameters ({})", app.session.doc.params.len());
    if draw_row(ui, ui.id().with("params"), &Row { depth: depth + 1, icon: "params", label: &params, ..Default::default() }).clicked {
        app.start("parameters.change");
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
    if let Some(p) = r.secondary {
        crate::context_menu::open_for(app, p, Target::Origin);
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

/// The standard views listed under Named Views: (label, view).
pub const STANDARD_VIEWS: [(&str, &str); 4] = [("Top", "top"), ("Front", "front"), ("Right", "right"), ("Home", "home")];

/// Named Views: the standard views, then the design's saved cameras. A double-click restores
/// one (animated).
fn named_view_rows(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize, acts: &mut Actions) {
    let key = "namedviews";
    let open = is_open(app, key, false);
    let r = draw_row(ui, ui.id().with(key), &Row { depth, fold: Some(open), icon: "folder", label: "Named Views", ..Default::default() });
    if r.fold || r.clicked {
        toggle(app, key, false);
    }
    if let Some(p) = r.secondary {
        acts.menu = Some((p, Target::NamedViews));
    }
    if !open {
        return;
    }
    for (label, view) in STANDARD_VIEWS {
        let r = draw_row(ui, ui.id().with(("stdview", view)), &Row { depth: depth + 1, icon: "home", label, ..Default::default() });
        if r.double {
            app.animate_view(view);
        }
        if let Some(p) = r.secondary {
            acts.menu = Some((p, Target::NamedView { name: label.to_string() }));
        }
    }
    let names: Vec<String> = app.session.doc.named_views.iter().map(|v| v.name.clone()).collect();
    for name in names {
        let r = draw_row(ui, ui.id().with(("view", &name)), &Row { depth: depth + 1, icon: "perspective", label: &name, ..Default::default() });
        if r.double {
            restore_view(app, &name);
        }
        if let Some(p) = r.secondary {
            acts.menu = Some((p, Target::NamedView { name }));
        }
    }
}

/// Turn the camera to a named view (a saved one or a standard one), animated.
pub fn restore_view(app: &mut SolveApp, name: &str) {
    if let Some((_, v)) = STANDARD_VIEWS.iter().find(|(l, _)| *l == name) {
        app.animate_view(v);
        return;
    }
    let Some(v) = app.session.doc.named_views.iter().find(|v| v.name == name) else { return };
    let j = json!({ "target": v.target, "yaw": v.yaw, "pitch": v.pitch, "distance": v.distance, "fov": v.fov });
    if let Ok(cam) = serde_json::from_value::<solvecraft_engine::render::Camera>(j) {
        app.animate_to(cam);
    }
}

/// Save the current camera as a named view.
pub fn save_view(app: &mut SolveApp, name: Option<&str>) -> Option<String> {
    let cam = serde_json::to_value(app.cam).ok()?;
    let r = app.run("view.save", json!({ "name": name, "camera": cam })).ok()?;
    app.tree.collapsed.remove("namedviews");
    app.tree.expanded.insert("namedviews".into());
    r.get("view").and_then(Value::as_str).map(str::to_string)
}

/// The menu of a named view.
pub fn view_items(app: &SolveApp, name: &str) -> Vec<Item> {
    let saved = app.session.doc.named_views.iter().any(|v| v.name == name);
    vec![
        Item::action("ui.restoreView", "Restore", "home").with(json!({ "view": name })),
        Item::action("ui.updateView", "Update to Current View", "perspective").with(json!({ "view": name })).on_if(saved),
        Item::sep(),
        Item::action("ui.rename", "Rename", "").with(json!({ "view": name })).on_if(saved),
        Item::action("view.delete", "Delete", "delete").with(json!({ "view": name })).on_if(saved),
    ]
}

/// The Canvases folder (reference images) with an eye per canvas.
fn canvas_rows(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize, acts: &mut Actions) {
    let canvases: Vec<(u64, String, bool)> = app.session.doc.canvases.iter().map(|c| (c.id, c.name.clone(), c.visible)).collect();
    if canvases.is_empty() {
        return;
    }
    let key = "canvases";
    let open = is_open(app, key, true);
    let any = canvases.iter().any(|c| c.2);
    let r =
        draw_row(ui, ui.id().with(key), &Row { depth, fold: Some(open), eye: Some(any), icon: "folder", label: "Canvases", ..Default::default() });
    if r.fold || r.clicked {
        toggle(app, key, true);
    }
    if r.eye {
        for (id, _, _) in &canvases {
            let _ = app.run("canvas.edit", json!({ "canvas": id, "visible": !any }));
        }
    }
    if !open {
        return;
    }
    for (id, name, visible) in canvases {
        let selected = app.tree.picked_canvases.contains(&id);
        let r = draw_row(
            ui,
            ui.id().with(("canvas", id)),
            &Row { depth: depth + 1, eye: Some(visible), icon: "canvas", label: &name, selected, dim: !visible, ..Default::default() },
        );
        if r.eye {
            let _ = app.run("canvas.edit", json!({ "canvas": id, "visible": !visible }));
        }
        if r.clicked {
            if !ui.input(|i| i.modifiers.command || i.modifiers.ctrl) {
                app.tree.picked_canvases.clear();
                app.tree.picked_components.clear();
                let _ = app.run("select.clear", json!({}));
            }
            if selected {
                app.tree.picked_canvases.retain(|c| *c != id);
            } else {
                app.tree.picked_canvases.push(id);
            }
        }
        if r.double {
            app.tree.canvas_panel = Some(CanvasPanel::new(app, id, false));
        }
        if let Some(p) = r.secondary {
            if !selected {
                app.tree.picked_canvases = vec![id];
            }
            acts.menu = Some((p, Target::Canvas { id }));
        }
    }
}

/// The menu of a canvas.
pub fn canvas_items(app: &SolveApp, id: u64) -> Vec<Item> {
    let visible = app.session.doc.canvases.iter().find(|c| c.id == id).is_some_and(|c| c.visible);
    vec![
        Item::action("ui.editCanvas", "Edit Canvas", "canvas").with(json!({ "canvas": id })),
        Item::action("ui.calibrateCanvas", "Calibrate", "measure").with(json!({ "canvas": id })),
        Item::sep(),
        Item::action("ui.delete", "Delete", "delete").with(json!({ "canvases": [id] })),
        Item::action("ui.rename", "Rename", "").with(json!({ "canvas": id })),
        Item::sep(),
        Item::action("ui.canvasVisible", "Show/Hide", "eye").key("V").with(json!({ "canvas": id, "visible": !visible })),
    ]
}

/// Editing a canvas (position, size, angle, opacity, flip) or calibrating it (two points on
/// the image and their true distance). Live edits of one opening are one undo step.
#[derive(Clone, Debug)]
pub struct CanvasPanel {
    pub canvas: u64,
    pub calibrate: bool,
    /// Undo depth when the panel opened.
    depth: usize,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub distance: f64,
}

impl CanvasPanel {
    pub fn new(app: &SolveApp, canvas: u64, calibrate: bool) -> Self {
        // Calibration starts from the image's left and right edge midpoints.
        let (a, b, d) = app
            .session
            .doc
            .canvases
            .iter()
            .find(|c| c.id == canvas)
            .map(|c| ([c.center.x - c.width / 2.0, c.center.y], [c.center.x + c.width / 2.0, c.center.y], c.width))
            .unwrap_or(([0.0, 0.0], [100.0, 0.0], 100.0));
        CanvasPanel { canvas, calibrate, depth: app.session.undo.len(), a, b, distance: d }
    }
}

fn canvas_panel(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut m) = app.tree.canvas_panel.clone() else { return };
    let Some(c) = app.session.doc.canvases.iter().find(|c| c.id == m.canvas).cloned() else {
        app.tree.canvas_panel = None;
        return;
    };
    let mut open = true;
    let mut edit: Option<Value> = None;
    let mut done = false;
    let title = if m.calibrate { format!("Calibrate: {}", c.name) } else { format!("Edit Canvas: {}", c.name) };
    crate::frame::window(ctx, title, crate::frame::Width::Normal)
        .id(egui::Id::new("sc_canvas_panel"))
        .pivot(egui::Align2::LEFT_TOP)
        .default_pos(panel_pos(app))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::Grid::new("sc_canvas_grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                if m.calibrate {
                    for (label, p) in [("Point A", &mut m.a), ("Point B", &mut m.b)] {
                        ui.label(label);
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut p[0]).speed(0.5).suffix(" mm"));
                            ui.add(egui::DragValue::new(&mut p[1]).speed(0.5).suffix(" mm"));
                        });
                        ui.end_row();
                    }
                    ui.label("Distance");
                    ui.add(egui::DragValue::new(&mut m.distance).speed(0.5).range(0.001..=1e6).suffix(" mm"));
                    ui.end_row();
                } else {
                    let (mut x, mut y, mut w, mut ang, mut op, mut flip) = (c.center.x, c.center.y, c.width, c.angle.to_degrees(), c.opacity, c.flip);
                    ui.label("Center X");
                    let r1 = ui.add(egui::DragValue::new(&mut x).speed(0.5).suffix(" mm"));
                    ui.end_row();
                    ui.label("Center Y");
                    let r2 = ui.add(egui::DragValue::new(&mut y).speed(0.5).suffix(" mm"));
                    ui.end_row();
                    ui.label("Width");
                    let r3 = ui.add(egui::DragValue::new(&mut w).speed(0.5).range(0.01..=1e6).suffix(" mm"));
                    ui.end_row();
                    ui.label("Angle");
                    let r4 = ui.add(egui::DragValue::new(&mut ang).speed(1.0).suffix(" deg"));
                    ui.end_row();
                    ui.label("Opacity");
                    let r5 = ui.add(egui::Slider::new(&mut op, 0.0..=1.0));
                    ui.end_row();
                    ui.label("Flip");
                    let r6 = ui.checkbox(&mut flip, "");
                    ui.end_row();
                    if [r1, r2, r3, r4, r5, r6].iter().any(|r| r.changed()) {
                        edit = Some(json!({ "canvas": c.id, "center": [x, y], "width": w, "angle": ang, "opacity": op, "flip": flip }));
                    }
                }
            });
            ui.add_space(6.0);
            if ui.button(if m.calibrate { "Calibrate" } else { "OK" }).clicked() {
                done = true;
            }
        });
    if let Some(p) = edit
        && app.run("canvas.edit", p).is_ok()
    {
        // One undo step for the whole edit.
        if app.session.undo.len() > m.depth + 1 {
            let keep = app.session.undo.get(m.depth).cloned();
            app.session.undo.truncate(m.depth);
            app.session.undo.extend(keep);
        }
    }
    if done && m.calibrate {
        if app.run("canvas.calibrate", json!({ "canvas": c.id, "a": m.a, "b": m.b, "distance": m.distance })).is_ok() {
            app.tree.canvas_panel = None;
        }
        return;
    }
    app.tree.canvas_panel = if done || !open { None } else { Some(m) };
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

/// One item of a component folder.
struct Entry {
    /// The browser key (body name, or the feature id of a sketch or plane).
    key: String,
    /// The key in [`TreeState`] row order (`b:`, `s:`, `p:`).
    row_key: String,
    label: String,
    icon: &'static str,
    visible: bool,
    selected: bool,
    color: Option<Color32>,
    locked: bool,
    sel: Option<Sel>,
    target: Option<Target>,
    /// Bodies lit when the row is hovered.
    bodies: Vec<String>,
}

/// What is being dragged in the tree: items of one component folder.
#[derive(Clone, Debug)]
struct Drag {
    comp: u64,
    folder: &'static str,
    keys: Vec<String>,
}

/// The items of a component folder, in model order.
fn entries(app: &SolveApp, comp: u64, folder: &str) -> Vec<Entry> {
    let t = Tokens::get();
    let doc = &app.session.doc;
    let sel = &app.session.selection;
    match folder {
        "bodies" => app
            .session
            .model
            .state()
            .bodies
            .iter()
            .filter(|b| doc.body_component(&b.name, b.feature) == comp)
            .map(|b| Entry {
                key: b.name.clone(),
                row_key: format!("b:{}", b.name),
                label: b.name.clone(),
                icon: "body",
                visible: !app.ui.hidden_bodies.contains(&b.name),
                selected: sel.iter().any(|x| matches!(x, Sel::Body { name } if *name == b.name)),
                color: None,
                locked: app.ui.locked_bodies.contains(&b.name),
                sel: Some(Sel::Body { name: b.name.clone() }),
                target: Some(Target::Body { name: b.name.clone() }),
                bodies: vec![b.name.clone()],
            })
            .collect(),
        "sketches" => doc
            .features
            .iter()
            .filter(|f| f.component == comp && matches!(f.kind, FeatureKind::Sketch { .. }))
            .map(|f| {
                let active = app.session.active_sketch == Some(f.id);
                Entry {
                    key: f.id.to_string(),
                    row_key: format!("s:{}", f.id),
                    label: if active { format!("{}  (editing)", f.name) } else { f.name.clone() },
                    icon: "sketch",
                    visible: sketch_visible(app, f.id),
                    selected: active || sel.iter().any(|x| matches!(x, Sel::Feature { id } if *id == f.id)),
                    color: active.then_some(t.sketch_accent),
                    locked: false,
                    sel: Some(Sel::Feature { id: f.id }),
                    target: Some(Target::Sketch { id: f.id }),
                    bodies: Vec::new(),
                }
            })
            .collect(),
        _ => doc
            .features
            .iter()
            .filter(|f| f.component == comp && matches!(f.kind, FeatureKind::ConstructionPlane { .. }))
            .map(|f| Entry {
                key: f.id.to_string(),
                row_key: format!("p:{}", f.id),
                label: f.name.clone(),
                icon: "plane",
                visible: !app.ui.hidden_origin.contains(&f.name),
                selected: sel.iter().any(|x| matches!(x, Sel::Plane { name } if *name == f.name)),
                color: None,
                locked: false,
                sel: Some(Sel::Plane { name: f.name.clone() }),
                target: None,
                bodies: Vec::new(),
            })
            .collect(),
    }
}

/// A folder's groups (with their items) and the items in no group, in browser order.
fn arrange(app: &SolveApp, comp: u64, folder: &str, all: Vec<Entry>) -> (Vec<(solvecraft_engine::doc::BrowserGroup, Vec<Entry>)>, Vec<Entry>) {
    let doc = &app.session.doc;
    let groups: Vec<solvecraft_engine::doc::BrowserGroup> =
        doc.browser_groups.iter().filter(|g| g.component == comp && g.folder == folder).cloned().collect();
    let mut rest: Vec<Option<Entry>> = all.into_iter().map(Some).collect();
    let mut take = |key: &str| rest.iter_mut().find(|e| e.as_ref().is_some_and(|e| e.key == key)).and_then(Option::take);
    let grouped: Vec<_> = groups
        .into_iter()
        .map(|g| {
            let items = g.items.iter().filter_map(|k| take(k)).collect();
            (g, items)
        })
        .collect();
    let mut loose: Vec<Entry> = rest.into_iter().flatten().collect();
    if let Some(order) = doc.browser_order.get(&format!("{comp}/{folder}")) {
        let pos = |k: &str| order.iter().position(|x| x == k).unwrap_or(usize::MAX);
        loose.sort_by_key(|e| pos(&e.key));
    }
    (grouped, loose)
}

/// Show or hide items of a folder.
fn set_visible(app: &mut SolveApp, folder: &str, items: &[&Entry], on: bool) {
    for e in items {
        match folder {
            "bodies" => {
                app.ui.hidden_bodies.retain(|b| *b != e.key);
                if !on {
                    app.ui.hidden_bodies.push(e.key.clone());
                }
            }
            "sketches" => {
                if let Ok(id) = e.key.parse() {
                    set_sketch_visible(app, id, on);
                }
            }
            _ => {
                app.ui.hidden_origin.retain(|p| *p != e.label);
                if !on {
                    app.ui.hidden_origin.push(e.label.clone());
                }
            }
        }
    }
}

fn folder_label(folder: &str) -> &'static str {
    match folder {
        "bodies" => "Bodies",
        "sketches" => "Sketches",
        _ => "Construction",
    }
}

fn folder_static(folder: &str) -> &'static str {
    match folder {
        "bodies" => "bodies",
        "sketches" => "sketches",
        _ => "construction",
    }
}

/// A component's folder: its row, its groups and its items. Rows are drag sources; items,
/// groups and the folder row are drop targets (reorder, into a group, out of groups).
fn folder_rows(app: &mut SolveApp, ui: &mut egui::Ui, comp: u64, folder: &str, depth: usize, acts: &mut Actions) {
    let t = Tokens::get();
    let all = entries(app, comp, folder);
    if all.is_empty() {
        return;
    }
    let fstatic = folder_static(folder);
    let key = format!("c{comp}/{folder}");
    if let Some(r) = app.tree.reveal.clone()
        && folder == "bodies"
        && all.iter().any(|e| e.key == r)
    {
        app.tree.collapsed.remove(&key);
        app.tree.collapsed.remove(&format!("c{comp}"));
        for g in app.session.doc.browser_groups.iter().filter(|g| g.items.contains(&r)) {
            app.tree.collapsed.remove(&format!("g{}", g.id));
        }
    }
    let open = is_open(app, &key, true);
    let any_visible = if folder == "sketches" { app.ui.show_sketches } else { all.iter().any(|e| e.visible) };
    let r = draw_row(
        ui,
        ui.id().with(&key),
        &Row { depth, fold: Some(open), eye: Some(any_visible), icon: "folder", label: folder_label(folder), ..Default::default() },
    );
    if r.fold || r.clicked {
        toggle(app, &key, true);
    }
    if r.eye {
        if folder == "sketches" {
            app.ui.show_sketches = !app.ui.show_sketches;
        } else {
            let refs: Vec<&Entry> = all.iter().collect();
            set_visible(app, folder, &refs, !any_visible);
        }
    }
    if r.hovered {
        acts.hover_bodies = all.iter().flat_map(|e| e.bodies.clone()).collect();
    }
    if let Some(p) = r.secondary {
        acts.menu = Some((p, Target::Folder { component: comp, folder: fstatic.to_string() }));
    }
    // Dropping on the folder row takes items out of their groups, to the end.
    if let Some(resp) = &r.resp
        && let Some(d) = drop_target(ui, resp, |d: &Drag| d.comp == comp && d.folder == fstatic, DropMark::Box)
    {
        acts.drop = Some(DropAction::Move { comp, folder: fstatic, keys: d.keys.clone(), group: None, before: None });
    }
    if !open {
        return;
    }
    let (groups, loose) = arrange(app, comp, folder, all);
    let loose_keys: Vec<String> = loose.iter().map(|e| e.key.clone()).collect();
    for (g, items) in &groups {
        let gkey = format!("g{}", g.id);
        let gopen = is_open(app, &gkey, true);
        let gvis = items.is_empty() || items.iter().any(|e| e.visible);
        let label = format!("{} ({})", g.name, items.len());
        let r = draw_row(
            ui,
            ui.id().with(&gkey),
            &Row { depth: depth + 1, fold: Some(gopen), eye: Some(gvis), icon: "folder", label: &label, dim: !gvis, ..Default::default() },
        );
        if r.fold || r.clicked {
            toggle(app, &gkey, true);
        }
        if r.eye {
            let refs: Vec<&Entry> = items.iter().collect();
            set_visible(app, folder, &refs, !gvis);
        }
        if r.hovered {
            acts.hover_bodies = items.iter().flat_map(|e| e.bodies.clone()).collect();
        }
        if let Some(p) = r.secondary {
            acts.menu = Some((p, Target::Group { id: g.id }));
        }
        if let Some(resp) = &r.resp
            && let Some(d) = drop_target(ui, resp, |d: &Drag| d.comp == comp && d.folder == fstatic, DropMark::Box)
        {
            acts.drop = Some(DropAction::Move { comp, folder: fstatic, keys: d.keys.clone(), group: Some(g.id), before: None });
        }
        if gopen {
            for e in items {
                entry_row(app, ui, comp, fstatic, e, depth + 2, Some(g.id), &loose_keys, acts);
            }
        }
    }
    for e in &loose {
        entry_row(app, ui, comp, fstatic, e, depth + 1, None, &loose_keys, acts);
    }
    let _ = t;
}

/// One item row of a folder.
#[allow(clippy::too_many_arguments)]
fn entry_row(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    comp: u64,
    folder: &'static str,
    e: &Entry,
    depth: usize,
    group: Option<u64>,
    loose: &[String],
    acts: &mut Actions,
) {
    app.tree.order.push(e.row_key.clone());
    let badges: &[&str] = if e.locked { &["lock"] } else { &[] };
    let r = draw_row(
        ui,
        ui.id().with(("item", &e.row_key)),
        &Row {
            depth,
            eye: Some(e.visible),
            icon: e.icon,
            label: &e.label,
            selected: e.selected,
            dim: !e.visible,
            color: e.color,
            badges,
            ..Default::default()
        },
    );
    if folder == "bodies"
        && app.tree.reveal.as_deref() == Some(e.key.as_str())
        && let Some(rect) = r.rect
    {
        ui.scroll_to_rect(rect, Some(egui::Align::Center));
        app.tree.reveal = None;
    }
    if r.eye {
        set_visible(app, folder, &[e], !e.visible);
    }
    if r.double && folder == "sketches" {
        if let Ok(id) = e.key.parse() {
            app.edit_sketch(id);
        }
    } else if r.clicked {
        if folder == "construction" {
            if let Some(s) = e.sel.clone() {
                pick_from_browser(app, s);
            }
        } else {
            click_select(app, ui, e.row_key.clone(), e.sel.clone());
        }
    }
    if r.hovered && !e.bodies.is_empty() {
        acts.hover_bodies = e.bodies.clone();
    }
    if let Some(p) = r.secondary
        && let Some(t) = e.target.clone()
    {
        if !e.selected
            && let Some(s) = &e.sel
        {
            let _ = app.run("select.set", json!({ "items": [s] }));
        }
        acts.menu = Some((p, t));
    }
    let Some(resp) = &r.resp else { return };
    // Drag: the selected items of this folder when this one is selected, else just this one.
    if resp.drag_started() && !r.on_control {
        let keys: Vec<String> = if e.selected { selected_keys(app, comp, folder) } else { vec![e.key.clone()] };
        resp.dnd_set_drag_payload(Drag { comp, folder, keys });
    }
    if let Some(d) = drop_target(ui, resp, |d: &Drag| d.comp == comp && d.folder == folder && !d.keys.contains(&e.key), DropMark::Above) {
        acts.drop = Some(match group {
            Some(g) => DropAction::Move { comp, folder, keys: d.keys.clone(), group: Some(g), before: Some(e.key.clone()) },
            None => {
                let mut order: Vec<String> = loose.iter().filter(|k| !d.keys.contains(k)).cloned().collect();
                let at = order.iter().position(|k| *k == e.key).unwrap_or(order.len());
                for (i, k) in d.keys.iter().enumerate() {
                    order.insert(at + i, k.clone());
                }
                DropAction::Order { comp, folder, keys: d.keys.clone(), order }
            }
        });
    }
}

/// Keys of the selected items of a component folder.
fn selected_keys(app: &SolveApp, comp: u64, folder: &str) -> Vec<String> {
    entries(app, comp, folder).into_iter().filter(|e| e.selected).map(|e| e.key).collect()
}

#[derive(Clone, Copy, PartialEq)]
enum DropMark {
    /// A line above the row: the items go before it.
    Above,
    /// A box around the row: the items go into it.
    Box,
}

/// While a matching payload hovers the row, mark where it would land; on release, the payload.
fn drop_target(ui: &egui::Ui, resp: &egui::Response, accept: impl Fn(&Drag) -> bool, mark: DropMark) -> Option<std::sync::Arc<Drag>> {
    let t = Tokens::get();
    let hovering = resp.dnd_hover_payload::<Drag>().filter(|d| accept(d));
    if hovering.is_some() {
        let r = resp.rect;
        match mark {
            DropMark::Above => ui.painter().hline(r.x_range(), r.top(), Stroke::new(2.0, t.accent)),
            DropMark::Box => ui.painter().rect_stroke(r.shrink(0.5), 3.0, Stroke::new(1.5, t.accent), egui::StrokeKind::Inside),
        };
    }
    // Taking a payload takes it whatever its type: leave other drags (appearance swatches) be.
    if !egui::DragAndDrop::has_payload_of_type::<Drag>(ui.ctx()) {
        return None;
    }
    resp.dnd_release_payload::<Drag>().filter(|d| accept(d))
}

/// A drop, applied after the tree is drawn.
enum DropAction {
    /// Into a group (before an item of it), or out of groups.
    Move { comp: u64, folder: &'static str, keys: Vec<String>, group: Option<u64>, before: Option<String> },
    /// Out of groups, in this order of the folder's loose items.
    Order { comp: u64, folder: &'static str, keys: Vec<String>, order: Vec<String> },
    /// Into another component (bodies and sketches).
    Component { to: u64, folder: &'static str, keys: Vec<String> },
}

fn apply_drop(app: &mut SolveApp, d: DropAction) {
    match d {
        DropAction::Move { comp, folder, keys, group, before } => {
            let _ = app.run("browser.move", json!({ "component": comp, "folder": folder, "items": keys, "group": group, "before": before }));
        }
        DropAction::Order { comp, folder, keys, order } => {
            let _ = app.run("browser.move", json!({ "component": comp, "folder": folder, "items": keys }));
            let _ = app.run("browser.order", json!({ "component": comp, "folder": folder, "order": order }));
        }
        DropAction::Component { to, folder, keys } => {
            let _ = match folder {
                "bodies" => app.run("component.move_bodies", json!({ "bodies": keys, "component": to })),
                "sketches" => app.run("component.move_sketches", json!({ "sketches": keys, "component": to })),
                _ => Ok(serde_json::Value::Null),
            };
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Panels opened from the menus
// ---------------------------------------------------------------------------------------------

/// Move/Copy of an occurrence: offsets and a rotation about Z, pending until Capture Position.
fn occurrence_panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(mut m) = app.tree.occurrence_move.clone() else { return };
    let ctx = ui.ctx().clone();
    if m.pivot.is_none() {
        m.pivot = occurrence_origin(app, m.occurrence);
    }
    let name = app.session.doc.occurrences.iter().find(|o| o.id == m.occurrence).map(|o| o.name.clone()).unwrap_or_default();
    let mut open = true;
    let mut action: Option<&str> = None;
    let before = (m.translate, m.angles);
    crate::frame::window(&ctx, format!("Move: {name}"), crate::frame::Width::Normal)
        .id(egui::Id::new("sc_occ_move"))
        .pivot(egui::Align2::LEFT_TOP)
        .default_pos(panel_pos(app))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(&ctx, |ui| {
            egui::Grid::new("sc_occ_grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                for (k, axis) in ["X distance", "Y distance", "Z distance"].iter().enumerate() {
                    ui.label(*axis);
                    if let Some(v) = m.translate.get_mut(k) {
                        ui.add(egui::DragValue::new(v).speed(0.5).suffix(" mm"));
                    }
                    ui.end_row();
                }
                for (k, axis) in ["X angle", "Y angle", "Z angle"].iter().enumerate() {
                    ui.label(*axis);
                    if let Some(v) = m.angles.get_mut(k) {
                        ui.add(egui::DragValue::new(v).speed(1.0).suffix(" deg"));
                    }
                    ui.end_row();
                }
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
    triad(app, ui, &mut m);
    if (m.translate, m.angles) != before {
        apply_occurrence_move(app, &m);
    }
    match action {
        Some("capture") => {
            let _ = app.run("component.capture_position", json!({}));
            app.tree.occurrence_move = None;
        }
        Some(_) => {
            let _ = app.run("component.revert_position", json!({}));
            app.tree.occurrence_move = None;
        }
        None if !open => {
            let _ = app.run("component.revert_position", json!({}));
            app.tree.occurrence_move = None;
        }
        None => app.tree.occurrence_move = Some(m),
    }
}

/// The pending move: turned about X, then Y, then Z through the starting origin, then moved.
pub fn apply_occurrence_move(app: &mut SolveApp, m: &OccMove) {
    use crate::gizmo;
    use solvecraft_engine::geom::Vec3;
    let a = m.angles.map(f64::to_radians);
    let r = gizmo::mul(&gizmo::mul(&gizmo::rotation(Vec3::Z, a[2]), &gizmo::rotation(Vec3::Y, a[1])), &gizmo::rotation(Vec3::X, a[0]));
    let (axis, angle) = gizmo::axis_angle(&r);
    let o = m.pivot.unwrap_or(Vec3::ZERO);
    let _ = app.session.execute("component.revert_position", &json!({}));
    let _ = app.run(
        "occurrence.move",
        json!({ "occurrence": m.occurrence, "translate": m.translate, "axis": [axis.x, axis.y, axis.z], "angle": angle, "origin": [o.x, o.y, o.z], "capture": false }),
    );
}

/// The shared move triad at the occurrence's origin: arrows and squares move it, rings turn it.
fn triad(app: &mut SolveApp, ui: &mut egui::Ui, m: &mut OccMove) {
    use crate::gizmo::{self, Change};
    let (Some(rect), Some(pivot)) = (app.viewport.rect, m.pivot) else { return };
    let proj = crate::viewport::projection(app, rect);
    let center = pivot + solvecraft_engine::geom::Vec3::new(m.translate[0], m.translate[1], m.translate[2]);
    let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::new("sc_occ_triad_layer"));
    let mut gui = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(rect));
    let painter = gui.painter().with_clip_rect(rect);
    let t = gizmo::Triad { center, translate: true, rotate: true };
    let step = crate::canvas::snap_step(app.cam.half_height());
    let Some(g) = gizmo::show(&mut gui, &painter, &proj, egui::Id::new("sc_occ_triad"), &t, step) else { return };
    if g.started || m.drag0.is_none() {
        m.drag0 = Some((m.translate, m.angles));
    }
    let (t0, a0) = m.drag0.unwrap_or((m.translate, m.angles));
    match g.change {
        Change::Translate(v) => m.translate = [t0[0] + v.x, t0[1] + v.y, t0[2] + v.z],
        Change::Rotate { axis, angle } => {
            if let Some(slot) = m.angles.get_mut(axis) {
                *slot = a0.get(axis).copied().unwrap_or(0.0) + angle.to_degrees();
            }
        }
    }
    if g.done {
        m.drag0 = None;
    }
}

/// Dimensions of finished sketches whose dimensions are shown (Show Dimensions), drawn over the
/// viewport like the sketch being edited draws its own (`crate::dim_view`), but not editable.
pub fn dims_overlay(app: &SolveApp, ctx: &egui::Context) {
    use solvecraft_engine::geom::Vec2;
    use solvecraft_engine::sketch::{DimFrame, chain_centres, default_text, dim_frame, dim_layout};
    let Some(rect) = app.viewport.rect else { return };
    if app.ui.shown_dims.is_empty() {
        return;
    }
    let tk = Tokens::get();
    let proj = crate::viewport::projection(app, rect);
    let painter = ctx.layer_painter(egui::LayerId::background()).with_clip_rect(rect);
    let st = app.session.world_state();
    for id in app.ui.shown_dims.iter().filter(|id| app.session.active_sketch != Some(**id) && sketch_visible(app, **id)) {
        let Some(ss) = st.sketch(*id) else { continue };
        let sk = &ss.sketch;
        let to = |p: Vec2| proj.to_screen(ss.plane.to_world(p));
        // Unplaced dimensions sit away from the middle of the sketch.
        let centres = chain_centres(sk);
        let n = centres.len().max(1) as f64;
        let middle = centres.iter().fold(Vec2::ZERO, |a, p| a + *p) * (1.0 / n);
        for c in &sk.constraints {
            let Some(frame) = dim_frame(sk, &c.kind) else { continue };
            let anchor = match frame {
                DimFrame::Linear { p0, p1, .. } => (p0 + p1) * 0.5,
                DimFrame::Radial { center, .. } | DimFrame::ArcLength { center, .. } => center,
                DimFrame::Angular { vertex, .. } => vertex,
            };
            let (Some(a), Some(b)) = (to(anchor), to(anchor + Vec2::X)) else { continue };
            let px = 1.0 / f64::from(a.distance(b)).max(1e-9);
            if !px.is_finite() {
                continue;
            }
            let expr = c.param.as_ref().and_then(|p| app.session.doc.param(p)).map(|p| p.expr.as_str());
            let col = if c.driven { tk.dimension_driven } else { tk.dimension };
            let galley = painter.layout_no_wrap(crate::dim_view::label(&c.kind, expr, c.driven), FontId::proportional(11.5), col);
            let size = galley.size() + vec2(6.0, 2.0);
            let half = Vec2::new(f64::from(size.x) * 0.5 * px, f64::from(size.y) * 0.5 * px);
            let lay = dim_layout(&frame, c.text.or_else(|| default_text(&frame, middle, px)), px, half);
            for line in &lay.lines {
                painter.add(egui::Shape::line(line.iter().filter_map(|q| to(*q)).collect(), Stroke::new(1.0, col)));
            }
            for (tip, dir) in &lay.arrows {
                if let (Some(t), Some(back)) = (to(*tip), to(*tip - *dir * px)) {
                    let d = (t - back).normalized();
                    let n = vec2(-d.y, d.x);
                    painter.add(egui::Shape::convex_polygon(vec![t, t - d * 9.0 + n * 3.0, t - d * 9.0 - n * 3.0], col, Stroke::NONE));
                }
            }
            let Some(tc) = to(lay.text) else { continue };
            let r = Rect::from_center_size(tc, size);
            painter.rect_filled(r, 2.0, tk.viewport_top.gamma_multiply(0.85));
            painter.galley(r.min + vec2(3.0, 1.0), galley, col);
        }
    }
}

/// Where the browser's panels open: the viewport's top left corner.
fn panel_pos(app: &SolveApp) -> Pos2 {
    app.viewport.rect.map(|r| r.left_top() + vec2(16.0, 16.0)).unwrap_or(pos2(270.0, 140.0))
}

/// The Capture Position question stands for saving.
pub const SAVE: &str = "__save";

/// Does starting `what` (a command id or [`SAVE`]) need the Capture Position question first?
/// Yes while components were moved without capturing their positions, except for the commands
/// that capture, revert or keep moving.
pub fn needs_capture(app: &SolveApp, what: &str) -> bool {
    !app.session.pending_moves.is_empty()
        && !matches!(what, "component.capture_position" | "component.revert_position" | "occurrence.move" | "edit.undo" | "edit.redo")
        && app.tree.capture_prompt.is_none()
}

/// Answer the Capture Position question: `capture`, `revert` or `cancel`. The waiting command
/// (or save) then goes ahead, unless cancelled.
pub fn answer_capture(app: &mut SolveApp, action: &str) -> Result<(), String> {
    let Some(what) = app.tree.capture_prompt.clone() else { return Err("no Capture Position question is open".into()) };
    match action {
        "capture" => drop(app.run("component.capture_position", json!({}))),
        "revert" => drop(app.run("component.revert_position", json!({}))),
        "cancel" => {
            app.tree.capture_prompt = None;
            return Ok(());
        }
        other => return Err(format!("unknown answer `{other}` (capture, revert, cancel)")),
    }
    app.tree.capture_prompt = None;
    app.tree.occurrence_move = None;
    if what == SAVE {
        crate::toolbar::save(app);
    } else {
        app.start(&what);
    }
    Ok(())
}

fn capture_prompt(app: &mut SolveApp, ctx: &egui::Context) {
    if app.tree.capture_prompt.is_none() {
        return;
    }
    let n = app.session.pending_moves.len();
    let mut answer: Option<&str> = None;
    crate::frame::window(ctx, "Capture Position", crate::frame::Width::Normal)
        .id(egui::Id::new("sc_capture_prompt"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -80.0))
        .show(ctx, |ui| {
            ui.label(if n == 1 {
                "A component was moved but its position isn't captured.".to_string()
            } else {
                format!("{n} components were moved but their positions aren't captured.")
            });
            ui.label(RichText::new("Capture keeps the new position; Revert puts it back.").color(Tokens::get().text_dim));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Capture Position").clicked() {
                    answer = Some("capture");
                }
                if ui.button("Revert").clicked() {
                    answer = Some("revert");
                }
                if ui.button("Cancel").clicked() {
                    answer = Some("cancel");
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                answer = Some("cancel");
            }
        });
    if let Some(a) = answer {
        let _ = answer_capture(app, a);
    }
}

/// Where an occurrence's origin is now (with its pending move).
fn occurrence_origin(app: &SolveApp, occurrence: u64) -> Option<solvecraft_engine::geom::Vec3> {
    let o = app.session.doc.occurrences.iter().find(|o| o.id == occurrence)?;
    let parent = app.session.doc.component_transform(o.parent);
    let own = app.session.pending_moves.get(&occurrence).copied().unwrap_or(o.transform);
    Some(solvecraft_engine::doc::apply_point(&solvecraft_engine::doc::mat_mul(&parent, &own), solvecraft_engine::geom::Vec3::ZERO))
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
    crate::frame::window(ctx, format!("Redefine Sketch Plane: {name}"), crate::frame::Width::Normal)
        .id(egui::Id::new("sc_redefine"))
        .pivot(egui::Align2::LEFT_TOP)
        .default_pos(panel_pos(app))
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
