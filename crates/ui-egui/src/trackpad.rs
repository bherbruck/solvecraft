//! Multi-finger trackpad drags: a three-finger drag orbits the view. winit forwards a trackpad's
//! pinch and rotation but not its touches, so each app reads them its own way (the desktop app
//! through AppKit on macOS, `apps/solidcraft/src/trackpad.rs`) and hands a three-finger drag's
//! motion to [`three_finger_drag`]. The page showing the camera turns the view with it
//! (`navigation::three_finger_orbit`) about the configured orbit centre; a drag with the pointer
//! over a dialog, on a flat page or on the start page does nothing, as the other trackpad
//! gestures do.

use crate::SolveApp;

/// A three-finger drag's motion waiting for the page that shows the camera.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ThreeFingerDrag {
    /// The fingers' motion since the page last took it, as a fraction of the trackpad's width:
    /// x to the right, y down (the screen's directions), equal for equal distances on the
    /// trackpad whatever its shape.
    pub delta: egui::Vec2,
    /// The fingers lifted (or the system took the gesture) after this motion.
    pub ended: bool,
}

/// How far a mouse drag would go, in points, for the fingers crossing the whole trackpad: a
/// full-width swipe turns the view about 200°.
pub(crate) const POINTS_PER_WIDTH: f64 = 440.0;

/// Hands a three-finger drag's motion since the last call (`delta`, see [`ThreeFingerDrag`]) and
/// its end to the page showing the camera. Nothing happens with the preference off, on the start
/// page or while a file picker is open.
pub fn three_finger_drag(app: &mut SolveApp, ctx: &egui::Context, delta: egui::Vec2, ended: bool) {
    if !app.preferences.trackpad_orbit || app.home.open || app.file_picker_busy() {
        app.viewport.touch_orbit = None;
        app.viewport.touch_orbiting = false;
        return;
    }
    let delta = if delta.is_finite() { delta } else { egui::Vec2::ZERO };
    let queued = app.viewport.touch_orbit.get_or_insert_default();
    queued.delta += delta;
    queued.ended |= ended;
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    /// Motion accumulates until the page takes it; the preference off drops it.
    #[test]
    fn motion_waits_for_the_page_and_the_preference_gates_it() {
        let ctx = egui::Context::default();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        assert!(app.preferences.trackpad_orbit);
        three_finger_drag(&mut app, &ctx, vec2(0.1, 0.0), false);
        three_finger_drag(&mut app, &ctx, vec2(0.1, -0.2), false);
        three_finger_drag(&mut app, &ctx, vec2(f32::NAN, 0.0), true);
        assert_eq!(app.viewport.touch_orbit, Some(ThreeFingerDrag { delta: vec2(0.2, -0.2), ended: true }));
        app.viewport.touch_orbiting = true;
        app.preferences.trackpad_orbit = false;
        three_finger_drag(&mut app, &ctx, vec2(0.1, 0.0), false);
        assert_eq!(app.viewport.touch_orbit, None, "the preference off drops the drag");
        assert!(!app.viewport.touch_orbiting);
    }
}
