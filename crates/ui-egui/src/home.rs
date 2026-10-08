//! The start page: New Design, Open, the recent designs and the built-in samples, each with a
//! picture of the design rendered from it. Shown on launch without a file and from the house
//! button; it closes when a design is opened or started, or the design changes.

use std::collections::HashMap;

use egui::{Color32, CornerRadius, RichText, Sense, Stroke, vec2};
use solvecraft_engine::Session;
use solvecraft_engine::render::{Camera, StandardView};

use crate::SolveApp;
use crate::theme::Tokens;

/// Recent designs kept.
pub const MAX_RECENT: usize = 12;
const THUMB: [usize; 2] = [240, 150];

/// What a background job makes: a key ("sample:N" or a path), the picture, and for samples the
/// built design (opened at once when picked).
type Made = (String, Option<egui::ColorImage>, Option<Session>);

#[derive(Default)]
pub struct HomeState {
    pub open: bool,
    /// Recent design paths, newest first (kept in the preferences).
    pub recent: Vec<String>,
    /// The session revision when the page opened (a change closes it).
    rev: u64,
    thumbs: HashMap<String, Option<egui::TextureHandle>>,
    samples: HashMap<usize, Session>,
    /// Keys asked for and not made yet.
    queued: Vec<String>,
    #[cfg(not(target_arch = "wasm32"))]
    rx: Option<std::sync::mpsc::Receiver<Made>>,
    /// A sample picked before its design was ready.
    wanted: Option<usize>,
}

impl HomeState {
    pub fn show(&mut self, rev: u64) {
        self.open = true;
        self.rev = rev;
    }

    /// Put a design at the top of the recent list.
    pub fn add_recent(&mut self, path: &str) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_string());
        self.recent.truncate(MAX_RECENT);
        self.thumbs.remove(path);
    }
}

/// A picture of a design: iso view, fitted.
pub fn thumbnail(s: &Session) -> Option<egui::ColorImage> {
    let mut cam = Camera::default();
    cam.set_view(StandardView::Iso);
    let b = solvecraft_engine::view::bounds(s);
    if b.is_empty() {
        return None;
    }
    cam.fit(&b);
    // Fill the card: the fit leaves room for a whole window's margins.
    cam.distance *= 0.7;
    let scene = solvecraft_engine::view::scene(s, &cam);
    let c = solvecraft_engine::render::render(&scene, &cam, THUMB[0], THUMB[1]);
    (c.rgba.len() == c.w * c.h * 4).then(|| egui::ColorImage::from_rgba_unmultiplied([c.w, c.h], &c.rgba))
}

/// Build what a key stands for (runs off the UI thread where threads exist).
fn make(key: &str) -> Made {
    let mut s = Session::default();
    let built = match key.strip_prefix("sample:").and_then(|i| i.parse::<usize>().ok()) {
        Some(i) => solvecraft_engine::sample::samples().get(i).is_some_and(|smp| {
            let ok = s.run_script(&(smp.script)()).is_ok();
            s.doc_mut().name = smp.name.to_string();
            s.undo.clear();
            s.mark_saved();
            ok
        }),
        None => s.execute("doc.open", &serde_json::json!({ "path": key })).is_ok(),
    };
    if !built {
        return (key.to_string(), None, None);
    }
    let img = thumbnail(&s);
    (key.to_string(), img, key.starts_with("sample:").then_some(s))
}

#[cfg(not(target_arch = "wasm32"))]
fn start_jobs(h: &mut HomeState) {
    if h.rx.is_some() || h.queued.is_empty() {
        return;
    }
    let keys = std::mem::take(&mut h.queued);
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("thumbnails".into()).spawn(move || {
        for k in keys {
            let made = std::panic::catch_unwind(|| make(&k)).unwrap_or((k, None, None));
            if tx.send(made).is_err() {
                return;
            }
        }
    });
    if spawned.is_ok() {
        h.rx = Some(rx);
    }
}

/// Take finished pictures and designs (one job per frame on the web, where there are no
/// threads).
fn poll(app: &mut SolveApp, ctx: &egui::Context) {
    let mut done: Vec<Made> = Vec::new();
    #[cfg(not(target_arch = "wasm32"))]
    {
        start_jobs(&mut app.home);
        if let Some(rx) = &app.home.rx {
            loop {
                match rx.try_recv() {
                    Ok(m) => done.push(m),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        app.home.rx = None;
                        break;
                    }
                }
            }
        }
        if app.home.rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    #[cfg(target_arch = "wasm32")]
    if !app.home.queued.is_empty() {
        let k = app.home.queued.remove(0);
        done.push(make(&k));
        ctx.request_repaint();
    }
    for (key, img, session) in done {
        if session.is_none() && key.starts_with("sample:") && app.home.wanted.is_some_and(|w| key == format!("sample:{w}")) {
            app.home.wanted = None;
            app.set_status(format!("the sample could not be built ({key})"), true);
        }
        let tex = img.map(|i| ctx.load_texture(format!("thumb:{key}"), i, egui::TextureOptions::LINEAR));
        app.home.thumbs.insert(key.clone(), tex);
        if let (Some(s), Some(i)) = (session, key.strip_prefix("sample:").and_then(|i| i.parse::<usize>().ok())) {
            app.home.samples.insert(i, s);
        }
    }
    if let Some(i) = app.home.wanted
        && app.home.samples.contains_key(&i)
    {
        app.home.wanted = None;
        open_sample(app, i);
    }
}

/// Ask for a key's picture (once: an empty entry marks it as asked for).
fn want(h: &mut HomeState, key: &str) {
    if !h.thumbs.contains_key(key) {
        h.queued.push(key.to_string());
        h.thumbs.insert(key.to_string(), None);
    }
}

/// Open a built-in sample (a copy of the design made for its picture).
pub fn open_sample(app: &mut SolveApp, i: usize) {
    match app.home.samples.get(&i) {
        Some(s) => {
            let mut copy = s.scratch();
            copy.mark_saved();
            crate::documents::adopt(app, copy);
            app.home.open = false;
        }
        None => {
            app.home.wanted = Some(i);
            want(&mut app.home, &format!("sample:{i}"));
        }
    }
}

fn open_recent(app: &mut SolveApp, path: &str) {
    if !solvecraft_engine::io::vfs::exists(path) {
        app.home.recent.retain(|p| p != path);
        app.set_status(format!("{path} is no longer there"), true);
        return;
    }
    app.home.open = false;
    app.open_path(path);
}

/// The page itself (in place of the toolbar, browser and viewport).
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    if app.session.revision != app.home.rev {
        app.home.open = false;
        return;
    }
    poll(app, ui.ctx());
    let t = Tokens::get();
    let samples = solvecraft_engine::sample::samples();
    // In the browser a picture blocks the page while it builds (no threads) and a kernel
    // failure there can't be caught: samples are built when they are opened.
    if !cfg!(target_arch = "wasm32") {
        for i in 0..samples.len() {
            want(&mut app.home, &format!("sample:{i}"));
        }
    }
    let recent = app.home.recent.clone();
    for p in recent.iter().filter(|p| solvecraft_engine::io::vfs::exists(p)) {
        want(&mut app.home, p);
    }
    let mut action: Option<Box<dyn FnOnce(&mut SolveApp)>> = None;
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin::symmetric(36, 24))).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("SolveCraft").size(26.0).strong().color(t.text));
                ui.add_space(12.0);
                ui.label(RichText::new("Start").size(16.0).color(t.text_dim));
            });
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                let big = |ui: &mut egui::Ui, icon: &str, label: &str, primary: bool| -> bool {
                    let (r, resp) = ui.allocate_exact_size(vec2(170.0, 64.0), Sense::click());
                    let fill = if primary {
                        t.accent
                    } else if resp.hovered() {
                        t.hover
                    } else {
                        t.field
                    };
                    ui.painter().rect(r, 6.0, fill, Stroke::new(1.0, if primary { t.accent } else { t.border }), egui::StrokeKind::Inside);
                    let ink = if primary { Color32::WHITE } else { t.icon };
                    crate::icons::paint(
                        ui.painter(),
                        egui::Rect::from_center_size(r.left_center() + vec2(30.0, 0.0), vec2(30.0, 30.0)),
                        icon,
                        ink,
                        t.icon_fill,
                        ink,
                    );
                    ui.painter().text(
                        r.left_center() + vec2(56.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(15.0),
                        if primary { Color32::WHITE } else { t.text },
                    );
                    resp.clicked()
                };
                if big(ui, "new", "New Design", true) {
                    action = Some(Box::new(|app: &mut SolveApp| crate::documents::new_design(app)));
                }
                ui.add_space(10.0);
                if big(ui, "open", "Open…", false) {
                    action = Some(Box::new(|app: &mut SolveApp| {
                        if let Some(p) = app.services.pick_open.as_ref().and_then(|f| f()) {
                            app.home.open = false;
                            app.open_path(&p);
                        }
                    }));
                }
            });
            ui.add_space(24.0);
            section(ui, "Recent");
            if recent.is_empty() {
                ui.label(RichText::new("Designs you open or save appear here.").color(t.text_dim));
            } else {
                grid(ui, recent.len(), |ui, i| {
                    let Some(p) = recent.get(i) else { return };
                    let path = std::path::Path::new(p);
                    let name = path.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| p.clone());
                    let folder = path.parent().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                    let missing = !solvecraft_engine::io::vfs::exists(p);
                    let sub = if missing {
                        "missing".to_string()
                    } else if p.starts_with("/opfs/") {
                        // The web build keeps designs in the browser's own storage.
                        "in this browser".to_string()
                    } else {
                        folder
                    };
                    if card(ui, Pic::Ready(app.home.thumbs.get(p.as_str()).and_then(Option::as_ref)), &name, &sub, missing)
                        .on_hover_text(p.as_str())
                        .clicked()
                    {
                        let p = p.clone();
                        action = Some(Box::new(move |app: &mut SolveApp| open_recent(app, &p)));
                    }
                });
            }
            ui.add_space(24.0);
            section(ui, "Samples");
            grid(ui, samples.len(), |ui, i| {
                let Some(s) = samples.get(i) else { return };
                let key = format!("sample:{i}");
                let busy = app.home.wanted == Some(i);
                let sub = if busy { "opening…" } else { s.about };
                let tex = match app.home.thumbs.get(&key) {
                    Some(t) => Pic::Ready(t.as_ref()),
                    None => Pic::Icon("box"),
                };
                if card(ui, tex, s.name, sub, false).on_hover_text(s.about).clicked() {
                    action = Some(Box::new(move |app: &mut SolveApp| open_sample(app, i)));
                }
            });
        });
    });
    if let Some(f) = action {
        f(app);
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    let t = Tokens::get();
    ui.label(RichText::new(title).size(15.0).strong().color(t.text));
    ui.add_space(8.0);
}

/// Cards in rows that fill the width.
fn grid(ui: &mut egui::Ui, n: usize, mut each: impl FnMut(&mut egui::Ui, usize)) {
    let w = THUMB[0] as f32 + 16.0;
    let per_row = ((ui.available_width() + 14.0) / (w + 14.0)).floor().max(1.0) as usize;
    for row in 0..n.div_ceil(per_row) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            for i in row * per_row..((row + 1) * per_row).min(n) {
                each(ui, i);
            }
        });
        ui.add_space(14.0);
    }
}

/// A card's picture: rendered (`None` while it renders), or an icon until it is asked for.
enum Pic<'a> {
    Ready(Option<&'a egui::TextureHandle>),
    Icon(&'static str),
}

/// A design card: its picture (a spinner while it renders), name and a line under it.
fn card(ui: &mut egui::Ui, pic: Pic, name: &str, sub: &str, dim: bool) -> egui::Response {
    let t = Tokens::get();
    let size = vec2(THUMB[0] as f32 + 16.0, THUMB[1] as f32 + 62.0);
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let hover = resp.hovered();
    ui.painter().rect(
        r,
        6.0,
        if hover { t.hover } else { t.field },
        Stroke::new(1.0, if hover { t.accent } else { t.border }),
        egui::StrokeKind::Inside,
    );
    let img = egui::Rect::from_min_size(r.min + vec2(8.0, 8.0), vec2(THUMB[0] as f32, THUMB[1] as f32));
    match pic {
        Pic::Icon(icon) => {
            ui.painter().rect_filled(img, CornerRadius::same(4), t.panel_header);
            crate::icons::paint(ui.painter(), egui::Rect::from_center_size(img.center(), vec2(56.0, 56.0)), icon, t.icon, t.icon_fill, t.icon);
        }
        Pic::Ready(Some(tx)) => {
            let tint = if dim { Color32::from_white_alpha(90) } else { Color32::WHITE };
            ui.painter().image(tx.id(), img, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
        }
        Pic::Ready(None) => {
            ui.painter().rect_filled(img, CornerRadius::same(4), t.panel_header);
            if !dim {
                egui::Spinner::new().size(18.0).paint_at(ui, egui::Rect::from_center_size(img.center(), vec2(18.0, 18.0)));
            }
        }
    }
    let ty = img.bottom() + 8.0;
    let clip = egui::Rect::from_min_max(egui::pos2(r.left() + 8.0, ty), egui::pos2(r.right() - 8.0, r.bottom()));
    let p = ui.painter().with_clip_rect(clip);
    p.text(egui::pos2(r.left() + 10.0, ty), egui::Align2::LEFT_TOP, name, egui::FontId::proportional(14.0), if dim { t.text_dim } else { t.text });
    p.text(egui::pos2(r.left() + 10.0, ty + 20.0), egui::Align2::LEFT_TOP, sub, egui::FontId::proportional(11.5), t.text_dim);
    if hover {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_list_is_newest_first_and_bounded() {
        let mut h = HomeState::default();
        for i in 0..20 {
            h.add_recent(&format!("/d/{i}.solvecraft"));
        }
        h.add_recent("/d/5.solvecraft");
        assert_eq!(h.recent.len(), MAX_RECENT);
        assert_eq!(h.recent.first().map(String::as_str), Some("/d/5.solvecraft"));
        assert_eq!(h.recent.iter().filter(|p| p.as_str() == "/d/5.solvecraft").count(), 1);
    }

    /// Samples build in the background and make a picture; picking one opens its design.
    #[test]
    fn sample_card_makes_a_picture_and_opens() {
        let (key, img, session) = make("sample:0");
        assert_eq!(key, "sample:0");
        let img = img.unwrap();
        assert_eq!(img.size, THUMB);
        let s = session.unwrap();
        assert_eq!(s.doc.name, "Sample Plate");
        assert!(!s.is_dirty() && s.undo.is_empty());
        let mut app = SolveApp::new(Session::default(), crate::Services::default());
        app.home.samples.insert(0, s);
        app.home.show(app.session.revision);
        open_sample(&mut app, 0);
        assert!(!app.home.open);
        assert_eq!(app.session.doc.name, "Sample Plate");
        assert!(!app.session.is_dirty());
    }
}
