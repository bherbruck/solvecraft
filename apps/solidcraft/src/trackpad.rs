//! macOS: a three-finger drag on the trackpad orbits the view.
//!
//! winit forwards the trackpad's pinch and rotation gestures but not its touches, and nothing
//! else tells three fingers from two. AppKit delivers the touches to the view under the pointer
//! through `touchesBegan/Moved/Ended/CancelledWithEvent:` once the view allows indirect touches;
//! winit's view implements none of the four, so [`install`] allows the touches on the window's
//! view and adds the methods to its class (as `open_documents` adds one to the application
//! delegate's). Each reads the fingers touching the trackpad; while exactly three are down, the
//! motion of their centre is queued for the UI thread, which takes it once per frame ([`take`])
//! and hands it to the page showing the camera (`solidcraft_ui_egui::trackpad`).
//!
//! The system's own three-finger swipes (Mission Control, switching full-screen apps) take the
//! gesture first and cancel the touches, so they must be set to four fingers in System Settings
//! › Trackpad › More Gestures for a three-finger drag to reach SolidCraft.
//!
//! Reading the window's view from its raw handle and adding methods to another crate's
//! Objective-C class have no safe API, hence the `unsafe_code` allowance on [`install`].

use std::sync::{Mutex, OnceLock, PoisonError};

use objc2::runtime::{AnyObject, Imp, Sel};
use objc2::sel;
use objc2_app_kit::{NSEvent, NSTouchPhase, NSTouchTypeMask, NSView};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// A three-finger drag's motion since the UI last took it, as a fraction of the trackpad's
/// width with y down (`solidcraft_ui_egui::trackpad::ThreeFingerDrag`), and whether it ended.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Drag {
    pub delta: egui::Vec2,
    pub ended: bool,
}

/// The trackpad's height over its width when it doesn't say: a MacBook's.
const DEFAULT_ASPECT: f64 = 0.65;

/// Motion that arrived and hasn't been taken yet.
static PENDING: Mutex<Drag> = Mutex::new(Drag { delta: egui::Vec2::ZERO, ended: false });
/// Where the fingers were last seen.
static TRACKER: Mutex<Tracker> = Mutex::new(Tracker::new());
/// The UI to wake when three fingers move.
static UI: OnceLock<egui::Context> = OnceLock::new();

/// Read the trackpad's touches on `window` from now on and wake `ctx` when three fingers move.
/// Call it once, on the main thread, with the window eframe just created. `false` if it isn't
/// an AppKit window or its view already handles touches.
#[allow(unsafe_code)]
pub fn install(window: &impl HasWindowHandle, ctx: &egui::Context) -> bool {
    let _ = UI.set(ctx.clone());
    let Ok(handle) = window.window_handle() else { return false };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else { return false };
    // SAFETY: the handle names the live NSView of the window eframe just created on this thread,
    // AppKit's main thread (the only one that may use a view); the reference ends with this call.
    let view = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    view.setAllowedTouchTypes(NSTouchTypeMask::Indirect);
    let object: &AnyObject = view;
    let class = object.class();
    let handler: extern "C-unwind" fn(&AnyObject, Sel, &NSEvent) = touches;
    // SAFETY: `Imp` is the untyped form of a method implementation; the Objective-C runtime calls
    // it with the arguments the type encoding declares, which match `touches`: no return value
    // (`v`), the receiver (`@`), the selector (`:`) and the event (`@`).
    let imp = unsafe { std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, &NSEvent), Imp>(handler) };
    let mut added = true;
    for selector in [sel!(touchesBeganWithEvent:), sel!(touchesMovedWithEvent:), sel!(touchesEndedWithEvent:), sel!(touchesCancelledWithEvent:)] {
        // SAFETY: `class` is the view's live class and `imp` matches the encoding (above).
        added &= unsafe { objc2::ffi::class_addMethod(std::ptr::from_ref(class).cast_mut(), selector, imp, c"v@:@".as_ptr()) }.as_bool();
    }
    if !added {
        log::warn!("the window's view already handles trackpad touches; three-finger drags won't orbit");
    }
    added
}

/// The motion since the last call, if any.
pub fn take() -> Option<Drag> {
    let drag = std::mem::take(&mut *PENDING.lock().unwrap_or_else(PoisonError::into_inner));
    (drag != Drag::default()).then_some(drag)
}

/// AppKit calls this for each of the four touch methods; a panic stops here instead of
/// unwinding through AppKit's frames.
extern "C-unwind" fn touches(_view: &AnyObject, _cmd: Sel, event: &NSEvent) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sample(event))).is_err() {
        log::warn!("a trackpad touch was dropped after an internal error");
    }
}

/// The fingers on the trackpad after `event` go to the tracker; three of them moving wake the UI.
fn sample(event: &NSEvent) {
    let touching = event.touchesMatchingPhase_inView(NSTouchPhase::Touching, None);
    let mut fingers = Vec::new();
    let mut aspect = None;
    // More than a hand's worth is noise, not a gesture.
    for touch in touching.allObjects().iter().take(16) {
        let at = touch.normalizedPosition();
        fingers.push((at.x, at.y));
        let size = touch.deviceSize();
        if size.width > 0.0 && size.height > 0.0 {
            aspect = Some(size.height / size.width);
        }
    }
    let drag = TRACKER.lock().unwrap_or_else(PoisonError::into_inner).sample(&fingers, aspect);
    if drag == Drag::default() {
        return;
    }
    let mut pending = PENDING.lock().unwrap_or_else(PoisonError::into_inner);
    pending.delta += drag.delta;
    pending.ended |= drag.ended;
    if let Some(ctx) = UI.get() {
        ctx.request_repaint();
    }
}

/// Follows three fingers on the trackpad.
#[derive(Debug)]
struct Tracker {
    /// The centre of the three fingers at the last sample (normalized trackpad coordinates,
    /// y up), while exactly three touch.
    centre: Option<(f64, f64)>,
}

impl Tracker {
    const fn new() -> Self {
        Tracker { centre: None }
    }

    /// One touch event: the fingers touching the trackpad (normalized positions, x right and
    /// y up) and the trackpad's height over its width. While exactly three touch, the motion of
    /// their centre since the last sample as a fraction of the trackpad's width with y down (the
    /// screen's directions); otherwise the end of a drag that was under way.
    fn sample(&mut self, fingers: &[(f64, f64)], aspect: Option<f64>) -> Drag {
        let centre = (fingers.len() == 3)
            .then(|| {
                let (x, y) = fingers.iter().fold((0.0, 0.0), |(sx, sy), (x, y)| (sx + x, sy + y));
                (x / 3.0, y / 3.0)
            })
            .filter(|(x, y)| x.is_finite() && y.is_finite());
        let Some((x, y)) = centre else {
            return Drag { delta: egui::Vec2::ZERO, ended: self.centre.take().is_some() };
        };
        let aspect = aspect.filter(|a| a.is_finite()).map_or(DEFAULT_ASPECT, |a| a.clamp(0.25, 4.0));
        let delta = match self.centre {
            Some((px, py)) => egui::vec2((x - px) as f32, ((py - y) * aspect) as f32),
            None => egui::Vec2::ZERO,
        };
        self.centre = Some((x, y));
        Drag { delta, ended: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn near(a: egui::Vec2, b: egui::Vec2) -> bool {
        (a - b).length() < 1e-6
    }

    /// Three fingers moving together report their centre's motion, in the screen's directions
    /// and in the trackpad's width whichever way they move; the first sample sets the start.
    #[test]
    fn three_fingers_move_the_centre_in_screen_directions() {
        let mut tracker = Tracker::new();
        let three = |dx: f64, dy: f64| [(0.3 + dx, 0.5 + dy), (0.4 + dx, 0.6 + dy), (0.5 + dx, 0.4 + dy)];
        assert_eq!(tracker.sample(&three(0.0, 0.0), Some(0.5)), Drag::default(), "the first sample only places the fingers");
        let right = tracker.sample(&three(0.1, 0.0), Some(0.5));
        assert!(near(right.delta, vec2(0.1, 0.0)) && !right.ended, "{right:?}");
        // Up on the trackpad is up on screen (negative y), scaled by the trackpad's shape.
        let up = tracker.sample(&three(0.1, 0.1), Some(0.5));
        assert!(near(up.delta, vec2(0.0, -0.05)) && !up.ended, "{up:?}");
        let unknown = tracker.sample(&three(0.1, 0.2), None);
        assert!(near(unknown.delta, vec2(0.0, -0.1 * DEFAULT_ASPECT as f32)), "{unknown:?}");
    }

    /// Two or four fingers are not the gesture: a drag ends when the count changes, a fourth
    /// finger included, and starts again from rest when three are back.
    #[test]
    fn other_finger_counts_end_the_drag() {
        let mut tracker = Tracker::new();
        let two = [(0.3, 0.5), (0.4, 0.6)];
        assert_eq!(tracker.sample(&two, Some(0.65)), Drag::default());
        let three = [(0.3, 0.5), (0.4, 0.6), (0.5, 0.4)];
        assert_eq!(tracker.sample(&three, Some(0.65)), Drag::default());
        let moved = [(0.4, 0.5), (0.5, 0.6), (0.6, 0.4)];
        assert!(near(tracker.sample(&moved, Some(0.65)).delta, vec2(0.1, 0.0)));
        let four = [(0.4, 0.5), (0.5, 0.6), (0.6, 0.4), (0.7, 0.3)];
        assert_eq!(tracker.sample(&four, Some(0.65)), Drag { delta: egui::Vec2::ZERO, ended: true });
        assert_eq!(tracker.sample(&[], Some(0.65)), Drag::default(), "nothing was under way");
        assert_eq!(tracker.sample(&three, Some(0.65)), Drag::default(), "three again: a new drag from rest");
        assert!(near(tracker.sample(&moved, Some(0.65)).delta, vec2(0.1, 0.0)));
        assert_eq!(tracker.sample(&[], Some(0.65)), Drag { delta: egui::Vec2::ZERO, ended: true });
    }

    /// A position that isn't a number ends the drag instead of moving the view somewhere.
    #[test]
    fn bad_positions_end_the_drag() {
        let mut tracker = Tracker::new();
        let three = [(0.3, 0.5), (0.4, 0.6), (0.5, 0.4)];
        tracker.sample(&three, Some(0.65));
        let bad = [(f64::NAN, 0.5), (0.4, 0.6), (0.5, 0.4)];
        assert_eq!(tracker.sample(&bad, Some(0.65)), Drag { delta: egui::Vec2::ZERO, ended: true });
        assert_eq!(tracker.sample(&three, Some(f64::INFINITY)), Drag::default());
        let moved = [(0.3, 0.6), (0.4, 0.7), (0.5, 0.5)];
        assert!(near(tracker.sample(&moved, Some(f64::INFINITY)).delta, vec2(0.0, -0.1 * DEFAULT_ASPECT as f32)));
    }
}
