//! Several open designs, as tabs in the application bar. The active design lives in the app as
//! always (`app.session`, `app.cam`…); the others wait here with their own session (undo,
//! selection), camera and per-design view state, and are swapped in when their tab is picked.
//! Closing a design with unsaved changes asks first, and so does quitting: one "save changes?"
//! per design with unsaved changes ([`request_quit`]).

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solvecraft_engine::Session;
use solvecraft_engine::render::Camera;

use crate::SolveApp;
use crate::theme::Tokens;

/// Most designs open at once.
pub const MAX_OPEN: usize = 32;

/// A design that is not the active one: everything that belongs to it alone.
pub struct DocSlot {
    session: Session,
    cam: Camera,
    pre_sketch_cam: Option<Camera>,
    pre_sketch_marker: Option<Option<usize>>,
    view: DocView,
}

/// The per-design part of the UI state (what is hidden, shown or locked).
#[derive(Clone, Debug, Default)]
struct DocView {
    hidden_bodies: Vec<String>,
    hidden_origin: Vec<String>,
    hidden_sketches: Vec<u64>,
    shown_sketches: Vec<u64>,
    hidden_profiles: Vec<u64>,
    shown_dims: Vec<u64>,
    locked_bodies: Vec<String>,
}

impl DocSlot {
    fn new(session: Session) -> DocSlot {
        DocSlot { session, cam: Camera::default(), pre_sketch_cam: None, pre_sketch_marker: None, view: DocView::default() }
    }
}

/// The open designs: one slot per tab; the active tab's slot is empty (its design is in the app).
#[derive(Default)]
pub struct Documents {
    slots: Vec<Option<DocSlot>>,
    pub active: usize,
    /// A tab waiting for the answer to "save changes?".
    pub closing: Option<usize>,
    /// Quitting: the tabs answered so far with Don't Save (`None`: not quitting).
    pub quitting: Option<Vec<usize>>,
}

impl Documents {
    /// Number of open designs (at least the active one).
    pub fn count(&self) -> usize {
        self.slots.len().max(1)
    }
}

fn take_view(app: &mut SolveApp) -> DocView {
    let u = &mut app.ui;
    DocView {
        hidden_bodies: std::mem::take(&mut u.hidden_bodies),
        hidden_origin: std::mem::take(&mut u.hidden_origin),
        hidden_sketches: std::mem::take(&mut u.hidden_sketches),
        shown_sketches: std::mem::take(&mut u.shown_sketches),
        hidden_profiles: std::mem::take(&mut u.hidden_profiles),
        shown_dims: std::mem::take(&mut u.shown_dims),
        locked_bodies: std::mem::take(&mut u.locked_bodies),
    }
}

fn put_view(app: &mut SolveApp, v: DocView) {
    let u = &mut app.ui;
    u.hidden_bodies = v.hidden_bodies;
    u.hidden_origin = v.hidden_origin;
    u.hidden_sketches = v.hidden_sketches;
    u.shown_sketches = v.shown_sketches;
    u.hidden_profiles = v.hidden_profiles;
    u.shown_dims = v.shown_dims;
    u.locked_bodies = v.locked_bodies;
}

/// Put `slot` into the app and return what was there.
fn swap_in(app: &mut SolveApp, slot: DocSlot) -> DocSlot {
    // Whatever was going on in the design leaves with it.
    crate::dialogs::cancel(app);
    app.tool = None;
    app.params = None;
    app.cam_anim = None;
    app.preview = crate::preview::PreviewState::default();
    app.viewport.hover = None;
    let out_rev = app.session.revision;
    let out = DocSlot {
        session: std::mem::replace(&mut app.session, slot.session),
        cam: std::mem::replace(&mut app.cam, slot.cam),
        pre_sketch_cam: std::mem::replace(&mut app.pre_sketch_cam, slot.pre_sketch_cam),
        pre_sketch_marker: std::mem::replace(&mut app.pre_sketch_marker, slot.pre_sketch_marker),
        view: take_view(app),
    };
    put_view(app, slot.view);
    // Views cache by revision: the incoming design must not look unchanged.
    app.session.revision = app.session.revision.max(out_rev) + 1;
    app.ui.tab = if app.session.active_sketch.is_some() { "SKETCH".into() } else { "SOLID".into() };
    out
}

/// Make tab `i` the active design.
pub fn switch(app: &mut SolveApp, i: usize) {
    if i == app.docs.active || i >= app.docs.slots.len() {
        return;
    }
    let Some(incoming) = app.docs.slots.get_mut(i).and_then(Option::take) else { return };
    let out = swap_in(app, incoming);
    let a = app.docs.active;
    if let Some(s) = app.docs.slots.get_mut(a) {
        *s = Some(out);
    }
    app.docs.active = i;
}

/// Open `session` in a new tab after the active one and switch to it (`fit`: frame its model).
pub fn open_tab(app: &mut SolveApp, session: Session) -> Result<(), String> {
    if app.docs.slots.is_empty() {
        app.docs.slots.push(None);
        app.docs.active = 0;
    }
    if app.docs.slots.len() >= MAX_OPEN {
        return Err(format!("at most {MAX_OPEN} designs can be open"));
    }
    let at = app.docs.active + 1;
    app.docs.slots.insert(at, Some(DocSlot::new(session)));
    switch(app, at);
    app.fit_view();
    Ok(())
}

/// Is the active design an untouched new one (it can be replaced instead of opening a tab)?
pub fn active_is_blank(app: &SolveApp) -> bool {
    let s = &app.session;
    s.path.is_none() && s.doc.features.is_empty() && s.doc.params.is_empty() && s.doc.components.is_empty() && !s.is_dirty()
}

/// Show `session` (a design built or read elsewhere): in place of a blank active design,
/// otherwise in a new tab.
pub fn adopt(app: &mut SolveApp, session: Session) {
    if active_is_blank(app) {
        let rev = app.session.revision;
        crate::dialogs::cancel(app);
        app.tool = None;
        app.session = session;
        app.session.revision = app.session.revision.max(rev) + 1;
        app.cam = Camera::default();
        app.fit_view();
    } else if let Err(e) = open_tab(app, session) {
        app.set_status(e, true);
    }
}

/// A new, empty design in its own tab.
pub fn new_design(app: &mut SolveApp) {
    app.home.open = false;
    if active_is_blank(app) {
        return;
    }
    if let Err(e) = open_tab(app, Session::default()) {
        app.set_status(e, true);
    }
    crate::prefs::apply_new_design(app);
}

/// Before opening a file: a fresh tab unless the active design is blank.
pub fn prepare_open(app: &mut SolveApp) {
    if !active_is_blank(app)
        && let Err(e) = open_tab(app, Session::default())
    {
        app.set_status(e, true);
    }
}

/// Close tab `i`: asks first when it has unsaved changes (`force`: close anyway).
pub fn close(app: &mut SolveApp, i: usize, force: bool) {
    let n = app.docs.count();
    if i >= n {
        return;
    }
    if !force && dirty(app, i) {
        app.docs.closing = Some(i);
        return;
    }
    app.docs.closing = None;
    if n <= 1 {
        // The last design: a blank one takes its place.
        let rev = app.session.revision;
        crate::dialogs::cancel(app);
        app.tool = None;
        app.session = Session::default();
        app.session.revision = rev + 1;
        app.cam = Camera::default();
        app.fit_view();
        put_view(app, DocView::default());
        app.docs = Documents::default();
        app.home.open = true;
        return;
    }
    if i == app.docs.active {
        // Show a neighbour first, then drop this one.
        let next = if i + 1 < n { i + 1 } else { i - 1 };
        switch(app, next);
    }
    app.docs.slots.remove(i);
    if app.docs.active > i {
        app.docs.active -= 1;
    }
}

/// Quit (the title bar's ×, File ▸ Quit, the window being closed): asks "save changes?" for each
/// design with unsaved changes, then sets `app.quit_requested` for the host to close the window.
/// Cancel, or a Save that didn't save, stays open.
pub fn request_quit(app: &mut SolveApp) {
    if app.docs.quitting.is_none() {
        app.docs.quitting = Some(Vec::new());
    }
    quit_step(app);
}

/// Ask about the next design with unsaved changes, or quit when none is left.
fn quit_step(app: &mut SolveApp) {
    let Some(skip) = app.docs.quitting.clone() else { return };
    if app.docs.closing.is_some() {
        return;
    }
    match (0..app.docs.count()).find(|i| !skip.contains(i) && dirty(app, *i)) {
        Some(i) => {
            switch(app, i);
            app.docs.closing = Some(i);
        }
        None => {
            // Don't Save means discard: no recovery copy is kept for the next launch either.
            for i in skip {
                switch(app, i);
                app.session.mark_saved();
            }
            app.docs.quitting = None;
            app.quit_requested = true;
        }
    }
}

/// Is any open design unsaved (the web page asks before it is left)?
pub fn any_dirty(app: &SolveApp) -> bool {
    (0..app.docs.count()).any(|i| dirty(app, i))
}

fn dirty(app: &SolveApp, i: usize) -> bool {
    if i == app.docs.active || app.docs.slots.is_empty() {
        return app.session.is_dirty();
    }
    app.docs.slots.get(i).and_then(Option::as_ref).is_some_and(|s| s.session.is_dirty())
}

/// Tab names (with an unsaved-changes flag), in order.
pub fn tabs_info(app: &SolveApp) -> Vec<(String, bool)> {
    if app.docs.slots.is_empty() {
        return vec![(app.session.doc.name.clone(), app.session.is_dirty())];
    }
    app.docs
        .slots
        .iter()
        .map(|s| match s {
            Some(d) => (d.session.doc.name.clone(), d.session.is_dirty()),
            None => (app.session.doc.name.clone(), app.session.is_dirty()),
        })
        .collect()
}

/// The control channel's `ui.documents`: list, `new`, `switch`, `close` (by index; `force`
/// closes without asking) or `quit`; `answer`: save | dont_save | cancel answers "save changes?".
pub fn control(app: &mut SolveApp, p: &Value) -> Value {
    let i = p.get("index").and_then(Value::as_u64).map(|x| x as usize);
    match p.get("action").and_then(Value::as_str) {
        Some("new") => new_design(app),
        Some("switch") => switch(app, i.unwrap_or(app.docs.active)),
        Some("close") => close(app, i.unwrap_or(app.docs.active), p.get("force").and_then(Value::as_bool).unwrap_or(false)),
        Some("quit") => request_quit(app),
        _ => {}
    }
    let answer = match p.get("answer").and_then(Value::as_str) {
        Some("save") => Some(Answer::Save),
        Some("dont_save") => Some(Answer::DontSave),
        Some("cancel") => Some(Answer::Cancel),
        _ => None,
    };
    if let Some(a) = answer {
        answer_prompt(app, a);
    }
    let tabs: Vec<Value> = tabs_info(app).into_iter().map(|(name, dirty)| json!({"name": name, "dirty": dirty})).collect();
    json!({"documents": tabs, "active": app.docs.active, "closing": app.docs.closing, "quitting": app.docs.quitting.is_some()})
}

/// The document tabs in the application bar, from `x0` to `x1`: click to switch, × to close,
/// + for a new design.
pub fn tabs(app: &mut SolveApp, ui: &mut egui::Ui, r: Rect, x0: f32, x1: f32) {
    let t = Tokens::get();
    let info = tabs_info(app);
    let n = info.len();
    let (first, shown, w) = layout(n, app.docs.active, x1 - x0);
    let mut x = x0;
    let mut pick: Option<usize> = None;
    let mut shut: Option<usize> = None;
    for (i, (name, dirty)) in info.iter().enumerate().skip(first).take(shown) {
        let tr = Rect::from_min_size(pos2(x, r.top() + 5.0), vec2(w - 2.0, r.height() - 5.0));
        let active = i == app.docs.active || app.docs.slots.is_empty();
        let resp = ui.interact(tr, ui.id().with(("doc_tab", i)), Sense::click_and_drag());
        // The tabs are part of the title bar: dragging one moves the window, a double click
        // maximizes it.
        if app.custom_titlebar || app.integrated_titlebar {
            if resp.drag_started_by(egui::PointerButton::Primary) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            } else if resp.double_clicked() {
                crate::titlebar::toggle_maximize(ui.ctx());
            }
        }
        let fill = if active {
            t.toolbar
        } else if resp.hovered() {
            Color32::from_white_alpha(28)
        } else {
            Color32::from_white_alpha(10)
        };
        ui.painter().rect_filled(tr, egui::CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, fill);
        let ink = if active { t.text } else { t.app_bar_text };
        let label = format!("{name}{}", if *dirty { " •" } else { "" });
        let g = ui.painter().layout_no_wrap(label, FontId::proportional(13.0), ink);
        let text_w = (tr.width() - 30.0).max(10.0);
        let clip = Rect::from_min_size(tr.min, vec2(text_w + 10.0, tr.height()));
        ui.painter().with_clip_rect(clip).galley(pos2(tr.left() + 10.0, tr.center().y - g.size().y / 2.0), g, ink);
        // ×
        let xr = Rect::from_center_size(pos2(tr.right() - 13.0, tr.center().y), vec2(16.0, 16.0));
        let xresp = ui.interact(xr, ui.id().with(("doc_close", i)), Sense::click());
        if xresp.hovered() {
            ui.painter().rect_filled(xr, 3.0, Color32::from_white_alpha(40));
        }
        let c = xr.center();
        let s = Stroke::new(1.3, if active { t.text_dim } else { t.app_bar_text });
        ui.painter().line_segment([c - vec2(3.5, 3.5), c + vec2(3.5, 3.5)], s);
        ui.painter().line_segment([c - vec2(3.5, -3.5), c + vec2(3.5, -3.5)], s);
        if xresp.on_hover_text("Close").clicked() {
            shut = Some(i);
        } else if resp.on_hover_text(name.as_str()).clicked() {
            pick = Some(i);
        }
        x += w;
    }
    // More tabs than fit: a chevron lists them all.
    if shown < n {
        let mr = Rect::from_center_size(pos2(x + 12.0, r.center().y + 2.0), vec2(20.0, 22.0));
        let mresp = ui.interact(mr, ui.id().with("doc_more"), Sense::click());
        if mresp.hovered() {
            ui.painter().rect_filled(mr, 4.0, Color32::from_white_alpha(30));
        }
        ui.painter().text(mr.center(), egui::Align2::CENTER_CENTER, format!("»{}", n - shown), FontId::proportional(12.0), t.app_bar_text);
        let id = egui::Id::new("sc_doc_more");
        if mresp.clone().on_hover_text("All open designs").clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(id, !d.get_temp::<bool>(id).unwrap_or(false)));
        }
        if ui.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
            let resp = egui::Area::new(id.with("area")).fixed_pos(pos2(mr.left(), r.bottom())).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
                crate::context_menu::menu_frame(ui, |ui| {
                    for (i, (name, dirty)) in info.iter().enumerate() {
                        let item = crate::context_menu::Item::action(
                            "doc",
                            &format!("{name}{}", if *dirty { " •" } else { "" }),
                            if i == app.docs.active { "box" } else { "" },
                        );
                        if crate::context_menu::list_row(ui, &item).is_some() {
                            pick = Some(i);
                        }
                    }
                })
            });
            let pressed_outside = ui.ctx().input(|i| i.pointer.any_pressed()) && !resp.response.contains_pointer() && !mresp.contains_pointer();
            if pick.is_some() || pressed_outside {
                ui.ctx().data_mut(|d| d.insert_temp(id, false));
            }
        }
        x += 26.0;
    }
    // New design: a new tab showing the start page.
    let pr = Rect::from_center_size(pos2(x + 14.0, r.center().y + 2.0), vec2(22.0, 22.0));
    let presp = ui.interact(pr, ui.id().with("doc_new"), Sense::click());
    if presp.hovered() {
        ui.painter().rect_filled(pr, 4.0, Color32::from_white_alpha(30));
    }
    let c = pr.center();
    let s = Stroke::new(1.6, t.app_bar_text);
    ui.painter().line_segment([c - vec2(6.0, 0.0), c + vec2(6.0, 0.0)], s);
    ui.painter().line_segment([c - vec2(0.0, 6.0), c + vec2(0.0, 6.0)], s);
    if presp.on_hover_text("New Design (Ctrl+N)").clicked() {
        new_design(app);
        app.home.show(app.session.revision);
    }
    if let Some(i) = shut {
        close(app, i, false);
    } else if let Some(i) = pick {
        switch(app, i);
    }
}

/// Which tabs the strip shows in `room` points: (first, how many, tab width). Tabs shrink to
/// `MIN_TAB`; past that a window of tabs around the active one is shown and the rest go in the »
/// list.
pub fn layout(n: usize, active: usize, room: f32) -> (usize, usize, f32) {
    const MIN_TAB: f32 = 90.0;
    let n = n.max(1);
    let avail = (room - 34.0).max(MIN_TAB);
    if avail / n as f32 >= MIN_TAB {
        return (0, n, (avail / n as f32).min(220.0));
    }
    // Leave room for the » button.
    let fit = (((avail - 26.0) / MIN_TAB).floor() as usize).max(1);
    let first = active.saturating_sub(fit - 1).min(n - fit);
    (first, fit, MIN_TAB)
}

/// What "save changes?" was answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Save,
    DontSave,
    Cancel,
}

/// The "save changes?" prompt for a tab being closed, or for each unsaved design when quitting.
pub fn prompt(app: &mut SolveApp, ctx: &egui::Context) {
    quit_step(app);
    let Some(i) = app.docs.closing else { return };
    let name = tabs_info(app).get(i).map(|x| x.0.clone()).unwrap_or_default();
    let mut answer: Option<Answer> = None;
    crate::frame::window(ctx, format!("Save changes to {name}?"), crate::frame::Width::Normal)
        .id(egui::Id::new("sc_close_prompt"))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label(format!("{name} has unsaved changes. If you don't save, they are lost."));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    answer = Some(Answer::Save);
                }
                if ui.button("Don't Save").clicked() {
                    answer = Some(Answer::DontSave);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(Answer::Cancel);
                }
            });
        });
    if ctx.input(|x| x.key_pressed(egui::Key::Escape)) {
        answer = Some(Answer::Cancel);
        app.esc_handled = true;
    }
    if let Some(a) = answer {
        answer_prompt(app, a);
    }
}

/// Act on the "save changes?" answer (also the control channel's `ui.documents {answer}`).
pub fn answer_prompt(app: &mut SolveApp, answer: Answer) {
    let Some(i) = app.docs.closing else { return };
    match answer {
        Answer::Save => {
            switch(app, i);
            crate::toolbar::save(app);
            let saved = !app.session.is_dirty();
            app.docs.closing = None;
            if app.docs.quitting.is_some() {
                // Quit only once it is saved; a cancelled picker or a failed save stays open.
                if !saved {
                    app.docs.quitting = None;
                }
            } else if saved {
                close(app, app.docs.active, true);
            }
        }
        Answer::DontSave => {
            if let Some(skip) = app.docs.quitting.as_mut() {
                skip.push(i);
                app.docs.closing = None;
            } else {
                close(app, i, true);
            }
        }
        Answer::Cancel => {
            app.docs.closing = None;
            app.docs.quitting = None;
        }
    }
    quit_step(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;

    fn app() -> SolveApp {
        SolveApp::new(Session::default(), Services::default())
    }

    /// Each tab keeps its own design, undo, selection and camera; closing asks when there are
    /// unsaved changes; the last tab closing leaves a blank design.
    #[test]
    fn the_tab_strip_overflows_into_a_list_keeping_the_active_tab() {
        assert_eq!(layout(2, 0, 800.0), (0, 2, 220.0));
        let (first, shown, w) = layout(12, 11, 600.0);
        assert!(shown < 12 && w == 90.0);
        assert!((first..first + shown).contains(&11), "the active tab stays visible");
        assert!(shown as f32 * w + 26.0 + 34.0 <= 600.0 + 0.5);
        assert_eq!(layout(12, 0, 600.0).0, 0);
    }

    #[test]
    fn tabs_keep_their_own_design() {
        let mut a = app();
        a.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        a.session.selection = vec![solvecraft_engine::Sel::Body { name: "Body1".into() }];
        a.cam.distance = 123.0;
        new_design(&mut a);
        assert_eq!(a.docs.count(), 2);
        assert_eq!(a.docs.active, 1);
        assert!(a.session.doc.features.is_empty() && a.session.selection.is_empty());
        a.run("solid.sphere", json!({"diameter": 8})).unwrap();
        a.run("solid.sphere", json!({"diameter": 4})).unwrap();
        switch(&mut a, 0);
        assert_eq!(a.session.doc.features.len(), 1);
        assert_eq!(a.session.selection.len(), 1);
        assert!((a.cam.distance - 123.0).abs() < 1e-9);
        a.run("edit.undo", json!({})).unwrap();
        assert!(a.session.doc.features.is_empty(), "undo is this design's");
        switch(&mut a, 1);
        assert_eq!(a.session.doc.features.len(), 2, "the other design kept its features");
        // Unsaved: closing asks, Don't Save closes.
        close(&mut a, 1, false);
        assert_eq!(a.docs.closing, Some(1));
        assert_eq!(a.docs.count(), 2);
        close(&mut a, 1, true);
        assert_eq!(a.docs.count(), 1);
        assert_eq!(a.docs.active, 0);
        close(&mut a, 0, true);
        assert!(active_is_blank(&a) && a.home.open);
    }

    /// Quitting asks once per design with unsaved changes: Don't Save moves on (and keeps no
    /// recovery copy), Save saves and moves on, and the app quits after the last answer.
    #[test]
    fn quit_asks_about_each_unsaved_design_then_quits() {
        let mut a = app();
        request_quit(&mut a);
        assert!(a.quit_requested, "nothing unsaved: quits at once");

        let mut a = app();
        let file = std::env::temp_dir().join(format!("solvecraft-quit-save-{}.solvecraft", std::process::id()));
        let _ = std::fs::remove_file(&file);
        a.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        a.run("file.save_as", json!({"path": file.to_string_lossy()})).unwrap();
        a.run("solid.sphere", json!({"diameter": 4})).unwrap();
        new_design(&mut a);
        a.run("solid.sphere", json!({"diameter": 8})).unwrap();
        assert!(dirty(&a, 0) && dirty(&a, 1));

        request_quit(&mut a);
        assert!(!a.quit_requested);
        assert_eq!(a.docs.closing, Some(0), "the first unsaved design is asked about first");
        assert_eq!(a.docs.active, 0, "and shown");
        answer_prompt(&mut a, Answer::Save);
        assert!(!dirty(&a, 0), "saved");
        assert_eq!(
            std::fs::read(&file).ok().and_then(|b| solvecraft_engine::io::read_design(&b).ok()).map(|d| d.features.len()),
            Some(2),
            "the file holds the change"
        );
        assert_eq!(a.docs.closing, Some(1), "then the next one");
        assert!(!a.quit_requested);
        answer_prompt(&mut a, Answer::DontSave);
        assert_eq!(a.docs.closing, None);
        assert!(a.quit_requested, "every design answered: quit");
        assert!(!any_dirty(&a), "Don't Save discards: nothing is kept for recovery");
        let _ = std::fs::remove_file(&file);
    }

    /// Cancel, Esc's answer, or a Save that saves nothing (a never-saved design whose Save As
    /// picker is cancelled) stay open with the changes.
    #[test]
    fn quit_cancelled_or_unsaved_stays_open() {
        let mut a = app();
        a.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        request_quit(&mut a);
        assert_eq!(a.docs.closing, Some(0));
        answer_prompt(&mut a, Answer::Cancel);
        assert!(!a.quit_requested && a.docs.closing.is_none() && a.docs.quitting.is_none());
        assert!(a.session.is_dirty(), "the changes are still there");

        // No Save As picker answer (the default services have none): not saved, so no quit.
        request_quit(&mut a);
        answer_prompt(&mut a, Answer::Save);
        assert!(!a.quit_requested && a.docs.quitting.is_none() && a.session.is_dirty());

        // A picked file: saved, then quit.
        let file = std::env::temp_dir().join(format!("solvecraft-quit-saveas-{}.solvecraft", std::process::id()));
        let _ = std::fs::remove_file(&file);
        let to = file.to_string_lossy().to_string();
        a.services.pick_save = Some(Box::new(move |_: &str, _: &[&str]| Some(to.clone())));
        request_quit(&mut a);
        answer_prompt(&mut a, Answer::Save);
        assert!(a.quit_requested && file.exists());
        let _ = std::fs::remove_file(&file);
    }

    /// A design built elsewhere replaces a blank one and gets a tab otherwise.
    #[test]
    fn adopt_reuses_a_blank_design() {
        let mut a = app();
        let mut s = Session::default();
        s.run_script(&solvecraft_engine::sample::script()).unwrap();
        adopt(&mut a, s);
        assert_eq!(a.docs.count(), 1);
        assert!(!a.session.doc.features.is_empty());
        adopt(&mut a, Session::default());
        assert_eq!(a.docs.count(), 2);
        let v = control(&mut a, &json!({"action": "switch", "index": 0}));
        assert_eq!(v["active"], 0);
        assert_eq!(v["documents"].as_array().map(Vec::len), Some(2));
    }
}
