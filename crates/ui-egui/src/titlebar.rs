//! The window's own title bar on Windows and Linux, where the window has no OS decorations
//! ([`SolveApp::custom_titlebar`]): empty space in the application bar drags the window and a
//! double-click there maximizes or restores it, caption buttons at the far right minimize,
//! maximize/restore and close, and thin invisible zones along the window's edges resize it.
//! Dragging uses the OS's own move loop, so Windows snapping (to the screen edges and corners)
//! works as with a native title bar. macOS keeps its traffic lights over the bar
//! ([`SolveApp::integrated_titlebar`]); the web build has neither.
//!
//! Adapted from VectorCraft's `crates/ui-egui/src/titlebar.rs` (MIT OR Apache-2.0).

use egui::{Color32, CursorIcon, Id, LayerId, Order, PointerButton, Rect, ResizeDirection, Sense, Stroke, Ui, ViewportCommand, pos2, vec2};

use crate::SolveApp;
use crate::theme::Tokens;

/// One caption button: as wide as the native Windows ones.
pub const BUTTON_WIDTH: f32 = 46.0;
/// The three caption buttons together.
pub const WIDTH: f32 = 3.0 * BUTTON_WIDTH;
/// Room for the macOS traffic lights at the bar's left end.
pub const TRAFFIC_LIGHTS: f32 = 76.0;
/// Resize zone thickness along an edge, and the side of a corner zone.
const EDGE: f32 = 5.0;
const CORNER: f32 = 12.0;
/// The Close button's hover colour (as on Windows).
const CLOSE_HOVER: Color32 = Color32::from_rgb(196, 43, 28);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Caption {
    Minimize,
    Maximize,
    Close,
}

/// Left to right.
pub const CAPTIONS: [Caption; 3] = [Caption::Minimize, Caption::Maximize, Caption::Close];

/// Caption button ids (stable, so tests and agents can find them).
pub fn caption_id(c: Caption) -> Id {
    Id::new(("titlebar-caption", c as u8))
}

fn maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false))
}

pub fn toggle_maximize(ctx: &egui::Context) {
    ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized(ctx)));
}

/// Window actions for the control channel (`ui.window`): minimize, maximize, restore, toggle
/// (maximize or restore), close.
pub fn window_action(app: &mut SolveApp, ctx: &egui::Context, action: &str) -> Result<(), String> {
    match action {
        "minimize" => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
        "maximize" => ctx.send_viewport_cmd(ViewportCommand::Maximized(true)),
        "restore" => ctx.send_viewport_cmd(ViewportCommand::Maximized(false)),
        "toggle" => toggle_maximize(ctx),
        "close" => {
            // Unsaved designs are asked about first; the window closes once they are answered.
            crate::documents::request_quit(app);
            if app.quit_requested {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
        other => return Err(format!("unknown window action `{other}` (minimize, maximize, restore, toggle, close)")),
    }
    Ok(())
}

/// The bar's background: a press-and-drag on empty space moves the window, a double-click
/// toggles maximize. Call it before laying out the bar so its buttons, registered later, take
/// their clicks.
pub fn drag_area(ui: &Ui, rect: Rect) {
    let ctx = ui.ctx();
    let resp = ui.interact(rect, Id::new("titlebar-drag"), Sense::click_and_drag());
    // egui hands a drag that starts on a click-only widget to the drag-sensing background below
    // it, so remember whether the press itself landed on bare background.
    if ctx.input(|i| i.pointer.any_pressed()) {
        let bare = ctx.viewport(|v| v.hits.click.is_some_and(|w| w.id == resp.id));
        ctx.data_mut(|d| d.insert_temp(resp.id, bare));
    }
    if resp.double_clicked() {
        toggle_maximize(ctx);
    } else if resp.drag_started_by(PointerButton::Primary) && ctx.data(|d| d.get_temp(resp.id).unwrap_or(false)) {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

/// Minimize, Maximize/Restore and Close at the right end of `bar` (the application bar's
/// rect), full bar height, flush with the window's edge so a maximized window's corner hits
/// Close.
pub fn caption_buttons(app: &mut SolveApp, ui: &mut Ui, bar: Rect) {
    let t = Tokens::get();
    let max = maximized(ui.ctx());
    let mut clicked = None;
    for (i, c) in CAPTIONS.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(bar.right() - WIDTH + i as f32 * BUTTON_WIDTH, bar.top()), vec2(BUTTON_WIDTH, bar.height()));
        let resp = ui.interact(r, caption_id(c), Sense::click());
        let close = c == Caption::Close;
        let fill = match (resp.is_pointer_button_down_on(), resp.hovered()) {
            (true, _) if close => CLOSE_HOVER.gamma_multiply(0.8),
            (_, true) if close => CLOSE_HOVER,
            (true, _) => Color32::from_white_alpha(18),
            (_, true) => Color32::from_white_alpha(30),
            _ => Color32::TRANSPARENT,
        };
        ui.painter().rect_filled(r, 0.0, fill);
        let ink = if close && resp.hovered() { Color32::WHITE } else { t.app_bar_text };
        paint_glyph(ui, c, max, r.center(), Stroke::new(1.0, ink));
        let tip = match c {
            Caption::Minimize => "Minimize",
            Caption::Maximize if max => "Restore",
            Caption::Maximize => "Maximize",
            Caption::Close => "Close",
        };
        if resp.on_hover_text(tip).clicked() {
            clicked = Some(c);
        }
    }
    let action = match clicked {
        Some(Caption::Minimize) => "minimize",
        Some(Caption::Maximize) => "toggle",
        Some(Caption::Close) => "close",
        None => return,
    };
    let _ = window_action(app, &ui.ctx().clone(), action);
}

/// The 10 pt line glyphs of the caption buttons.
fn paint_glyph(ui: &Ui, c: Caption, maximized: bool, at: egui::Pos2, s: Stroke) {
    let p = ui.painter();
    let g = Rect::from_center_size(at, vec2(10.0, 10.0));
    match c {
        Caption::Minimize => {
            p.line_segment([g.left_center(), g.right_center()], s);
        }
        // Restore: a front square with the corner of the one behind it.
        Caption::Maximize if maximized => {
            let front = Rect::from_min_max(g.min + vec2(0.0, 2.0), g.max - vec2(2.0, 0.0));
            p.rect_stroke(front, 0.0, s, egui::StrokeKind::Middle);
            p.line(vec![g.min + vec2(2.0, 2.0), g.min + vec2(2.0, 0.0), g.right_top(), g.max - vec2(0.0, 2.0), g.max - vec2(2.0, 2.0)], s);
        }
        Caption::Maximize => {
            p.rect_stroke(g, 0.0, s, egui::StrokeKind::Middle);
        }
        Caption::Close => {
            p.line_segment([g.left_top(), g.right_bottom()], s);
            p.line_segment([g.right_top(), g.left_bottom()], s);
        }
    }
}

/// The edge and corner zones of a window with content rect `w`: (zone, direction, cursor).
/// Corners come last so they win where they overlap the edges.
fn zones(w: Rect) -> [(Rect, ResizeDirection, CursorIcon); 8] {
    use ResizeDirection::*;
    let (e, c) = (EDGE, CORNER);
    [
        (Rect::from_min_max(w.left_top(), pos2(w.right(), w.top() + e)), North, CursorIcon::ResizeVertical),
        (Rect::from_min_max(pos2(w.left(), w.bottom() - e), w.right_bottom()), South, CursorIcon::ResizeVertical),
        (Rect::from_min_max(w.left_top(), pos2(w.left() + e, w.bottom())), West, CursorIcon::ResizeHorizontal),
        (Rect::from_min_max(pos2(w.right() - e, w.top()), w.right_bottom()), East, CursorIcon::ResizeHorizontal),
        (Rect::from_min_size(w.left_top(), vec2(c, c)), NorthWest, CursorIcon::ResizeNwSe),
        (Rect::from_min_size(w.right_top() - vec2(c, 0.0), vec2(c, c)), NorthEast, CursorIcon::ResizeNeSw),
        (Rect::from_min_size(w.left_bottom() - vec2(0.0, c), vec2(c, c)), SouthWest, CursorIcon::ResizeNeSw),
        (Rect::from_min_size(w.right_bottom() - vec2(c, c), vec2(c, c)), SouthEast, CursorIcon::ResizeNwSe),
    ]
}

/// Invisible resize zones along the window's edges, above everything else; a press in one
/// starts an OS resize in that direction. None while the window is maximized or fullscreen.
pub fn resize_zones(ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    if maximized(&ctx) || ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
        return;
    }
    let w = ctx.content_rect();
    let layer = LayerId::new(Order::Foreground, Id::new("titlebar-resize"));
    let zui = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(w));
    for (i, (r, dir, cursor)) in zones(w).into_iter().enumerate() {
        let resp = zui.interact(r, Id::new(("titlebar-resize", i)), Sense::drag()).on_hover_cursor(cursor);
        if resp.drag_started_by(PointerButton::Primary) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Pos2, RawInput, ViewportId};
    use solvecraft_engine::Session;

    /// A headless window `width` pt wide showing the application bar (as the title bar) and the
    /// resize zones.
    struct Win {
        ctx: egui::Context,
        app: SolveApp,
        width: f32,
        time: f64,
        maximized: bool,
    }

    impl Win {
        fn new(width: f32) -> Self {
            let ctx = egui::Context::default();
            let mut app = SolveApp::new(Session::default(), Default::default());
            app.custom_titlebar = true;
            Self { ctx, app, width, time: 0.0, maximized: false }
        }

        fn frame(&mut self, events: Vec<Event>) -> egui::FullOutput {
            self.time += 0.05;
            let mut input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(self.width, 700.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            if let Some(v) = input.viewports.get_mut(&ViewportId::ROOT) {
                v.maximized = Some(self.maximized);
            }
            let app = &mut self.app;
            let mut out = self.ctx.run_ui(input, |ui| {
                crate::toolbar::app_bar(app, ui);
                if app.custom_titlebar {
                    resize_zones(ui);
                }
            });
            out.textures_delta.clear();
            out
        }

        /// Frames for a pointer gesture: move to `path[0]`, press, move along `path`, release.
        /// The viewport commands the frames sent.
        fn gesture(&mut self, path: &[Pos2]) -> Vec<ViewportCommand> {
            let button = |p: Pos2, pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Default::default() };
            let mut events = vec![vec![Event::PointerMoved(path[0])], vec![button(path[0], true)]];
            events.extend(path[1..].iter().map(|p| vec![Event::PointerMoved(*p)]));
            events.push(vec![button(*path.last().unwrap_or(&path[0]), false)]);
            events.into_iter().flat_map(|e| self.frame(e).viewport_output.remove(&ViewportId::ROOT).map(|v| v.commands).unwrap_or_default()).collect()
        }

        fn click(&mut self, p: Pos2) -> Vec<ViewportCommand> {
            self.gesture(&[p])
        }

        fn caption(&self, c: Caption) -> Rect {
            self.ctx.read_response(caption_id(c)).map(|r| r.rect).unwrap_or(Rect::NOTHING)
        }
    }

    fn texts(out: &egui::FullOutput) -> Vec<(String, Rect)> {
        fn walk(s: &egui::Shape, v: &mut Vec<(String, Rect)>) {
            match s {
                egui::Shape::Text(t) => v.push((t.galley.text().to_string(), Rect::from_min_size(t.pos, t.galley.size()))),
                egui::Shape::Vec(s) => s.iter().for_each(|s| walk(s, v)),
                _ => {}
            }
        }
        let mut v = vec![];
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut v));
        v
    }

    #[test]
    fn caption_buttons_sit_in_the_corner_and_nothing_runs_under_them() {
        for width in [960.0, 1600.0] {
            let mut w = Win::new(width);
            w.frame(vec![]);
            let out = w.frame(vec![]);
            let [min, max, close] = CAPTIONS.map(|c| w.caption(c));
            assert_eq!((close.right(), close.top()), (width, 0.0), "Close sits in the window's corner at {width}");
            assert!([min, max, close].iter().all(|r| r.width() == BUTTON_WIDTH));
            assert_eq!((min.right(), max.right()), (max.left(), close.left()));
            let texts = texts(&out);
            let version = texts.iter().find(|(t, _)| t.starts_with('v')).map(|(_, r)| *r);
            assert!(version.is_some_and(|r| r.right() <= min.left() - 4.0), "the version stays left of the buttons: {version:?}");
            for (t, r) in &texts {
                assert!(r.right() <= min.left() || r.top() >= min.bottom(), "`{t}` runs under the caption buttons at {width}");
            }
        }
    }

    #[test]
    fn no_caption_buttons_without_a_custom_title_bar() {
        let mut w = Win::new(1200.0);
        w.app.custom_titlebar = false;
        w.frame(vec![]);
        w.frame(vec![]);
        assert_eq!(w.caption(Caption::Close), Rect::NOTHING);
    }

    #[test]
    fn minimize_maximize_and_close() {
        let mut w = Win::new(1200.0);
        w.frame(vec![]);
        let (min, max, close) = (w.caption(Caption::Minimize).center(), w.caption(Caption::Maximize).center(), w.caption(Caption::Close).center());
        assert!(w.click(min).contains(&ViewportCommand::Minimized(true)));
        assert!(w.click(max).contains(&ViewportCommand::Maximized(true)));
        w.maximized = true;
        assert!(w.click(max).contains(&ViewportCommand::Maximized(false)), "Restore");
        assert!(w.click(close).contains(&ViewportCommand::Close));
        assert!(w.app.quit_requested);
    }

    #[test]
    fn empty_bar_space_drags_and_double_click_maximizes() {
        let mut w = Win::new(1600.0);
        w.frame(vec![]);
        let empty = pos2(500.0, 17.0);
        let cmds = w.gesture(&[empty, empty + vec2(12.0, 4.0), empty + vec2(30.0, 8.0)]);
        assert_eq!(cmds.iter().filter(|c| matches!(c, ViewportCommand::StartDrag)).count(), 1, "{cmds:?}");
        let mut cmds = w.click(empty);
        cmds.extend(w.click(empty));
        assert!(cmds.contains(&ViewportCommand::Maximized(true)), "{cmds:?}");
        w.maximized = true;
        w.time += 1.0;
        let mut cmds = w.click(empty);
        cmds.extend(w.click(empty));
        assert!(cmds.contains(&ViewportCommand::Maximized(false)), "{cmds:?}");
        // The bar's buttons keep their clicks: dragging from Undo doesn't move the window.
        w.time += 1.0;
        let undo = pos2(128.0 + 2.0 * 30.0, 17.0);
        let cmds = w.gesture(&[undo, undo + vec2(12.0, 4.0), undo + vec2(30.0, 8.0)]);
        assert!(!cmds.iter().any(|c| matches!(c, ViewportCommand::StartDrag)), "{cmds:?}");
    }

    #[test]
    fn edge_zones_resize_unless_maximized() {
        use ResizeDirection::*;
        let mut w = Win::new(1000.0);
        w.frame(vec![]);
        for (p, dir) in [
            (pos2(1.0, 300.0), West),
            (pos2(998.0, 300.0), East),
            (pos2(500.0, 1.0), North),
            (pos2(500.0, 698.0), South),
            (pos2(2.0, 2.0), NorthWest),
            (pos2(997.0, 3.0), NorthEast),
            (pos2(3.0, 697.0), SouthWest),
            (pos2(998.0, 698.0), SouthEast),
        ] {
            let cmds = w.gesture(&[p, p + vec2(6.0, 6.0)]);
            assert_eq!(cmds, vec![ViewportCommand::BeginResize(dir)], "at {p:?}");
        }
        w.maximized = true;
        let cmds = w.gesture(&[pos2(1.0, 300.0), pos2(8.0, 300.0)]);
        assert!(!cmds.iter().any(|c| matches!(c, ViewportCommand::BeginResize(_))), "{cmds:?}");
    }
}
