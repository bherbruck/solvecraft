//! Colours and widget style: a dark theme (the default, a blue-grey in the spirit of desktop CAD
//! tools, viewport #2e3440) and a light one. Every colour the UI uses comes from [`Tokens`].

use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Color32, Stroke};

static DARK: AtomicBool = AtomicBool::new(true);

/// Is the dark theme on?
pub fn is_dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub app_bar: Color32,
    pub toolbar: Color32,
    pub tab_active: Color32,
    pub panel: Color32,
    pub panel_header: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub sketch_accent: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub icon: Color32,
    pub icon_fill: Color32,
    pub timeline: Color32,
    pub hover: Color32,
    // ---- viewport selection feedback ----
    /// Selected face (flat fill).
    pub sel_face: Color32,
    /// Selected edge core and its lighter rim.
    pub sel_edge: Color32,
    pub sel_edge_rim: Color32,
    /// Hovered edge: dark core with a light halo.
    pub hover_edge: Color32,
    pub hover_edge_halo: Color32,
    /// How much lighter a hovered face is (grey levels).
    pub hover_face_lift: u8,
    /// Selected profile fill and outline.
    pub sel_profile: Color32,
    pub sel_profile_edge: Color32,
    /// Hovered profile: faint tint and a bright outline.
    pub hover_profile: Color32,
    pub hover_profile_edge: Color32,
    /// Selected vertex dot.
    pub sel_vertex: Color32,
    // ---- origin ----
    pub origin_plane: Color32,
    pub origin_plane_edge: Color32,
    pub origin_plane_hover: Color32,
    pub origin_point: Color32,
    pub axis_x: Color32,
    pub axis_y: Color32,
    pub axis_z: Color32,
    /// Construction planes.
    pub construction_plane: Color32,
    // ---- bodies and live previews ----
    pub body: Color32,
    pub body_edge: Color32,
    /// Command manipulators (direction arrows, handles).
    pub manipulator: Color32,
    /// New surface in a feature preview.
    pub preview_add: Color32,
    /// Removed material in a feature preview (translucent).
    pub preview_cut: Color32,
    // ---- viewport ----
    /// Viewport background (top and bottom of the gradient; equal for a flat fill).
    pub viewport_top: Color32,
    pub viewport_bottom: Color32,
    pub grid: Color32,
    pub grid_major: Color32,
    /// A whole selected body.
    pub body_selected: Color32,
    /// Sketch curves: under-constrained, fully constrained (and finished sketches).
    pub sketch: Color32,
    pub sketch_fixed: Color32,
    /// Sketch points: fully constrained, free.
    pub sketch_point: Color32,
    pub sketch_point_free: Color32,
    /// Constraint glyph boxes.
    pub glyph_bg: Color32,
    pub glyph_edge: Color32,
    pub glyph_text: Color32,
    /// Chips drawn over the viewport (status, navigation bar, dimension labels).
    pub overlay: Color32,
    /// View cube face (shaded by orientation) and edges.
    pub cube_face: Color32,
    pub cube_edge: Color32,
    /// Rubber band of the shape being drawn.
    pub rubber_band: Color32,
    /// Light text on the application bar.
    pub app_bar_text: Color32,
    /// Timeline items: normal and failed.
    pub timeline_item: Color32,
    pub timeline_item_error: Color32,
    pub timeline_marker: Color32,
    /// Button and field backgrounds on panels.
    pub field: Color32,
    // ---- box selection ----
    pub box_window: Color32,
    pub box_crossing: Color32,
}

impl Tokens {
    /// The tokens of the current theme.
    pub fn get() -> Tokens {
        if is_dark() { Tokens::dark() } else { Tokens::light() }
    }

    pub const fn light() -> Tokens {
        Tokens {
            app_bar: Color32::from_rgb(52, 56, 64),
            toolbar: Color32::from_rgb(246, 247, 249),
            tab_active: Color32::from_rgb(255, 255, 255),
            panel: Color32::from_rgb(250, 251, 252),
            panel_header: Color32::from_rgb(236, 238, 242),
            border: Color32::from_rgb(208, 212, 220),
            text: Color32::from_rgb(38, 42, 50),
            text_dim: Color32::from_rgb(110, 116, 128),
            accent: Color32::from_rgb(38, 120, 218),
            accent_soft: Color32::from_rgb(214, 230, 250),
            sketch_accent: Color32::from_rgb(46, 150, 90),
            error: Color32::from_rgb(214, 54, 54),
            warning: Color32::from_rgb(222, 150, 30),
            icon: Color32::from_rgb(70, 78, 92),
            icon_fill: Color32::from_rgb(178, 196, 222),
            timeline: Color32::from_rgb(232, 235, 240),
            hover: Color32::from_rgb(226, 234, 246),
            sel_face: Color32::from_rgb(76, 127, 203),
            sel_edge: Color32::from_rgb(0, 127, 255),
            sel_edge_rim: Color32::from_rgb(127, 191, 255),
            hover_edge: Color32::from_rgb(0, 0, 0),
            hover_edge_halo: Color32::from_rgb(203, 203, 203),
            hover_face_lift: 40,
            sel_profile: Color32::from_rgb(26, 84, 167),
            sel_profile_edge: Color32::from_rgb(47, 109, 198),
            hover_profile: Color32::from_rgba_unmultiplied_const(75, 129, 253, 48),
            hover_profile_edge: Color32::from_rgb(75, 129, 253),
            sel_vertex: Color32::from_rgb(70, 110, 170),
            origin_plane: Color32::from_rgba_unmultiplied_const(249, 184, 134, 94),
            origin_plane_edge: Color32::from_rgba_unmultiplied_const(214, 140, 84, 200),
            origin_plane_hover: Color32::from_rgba_unmultiplied_const(120, 130, 230, 150),
            origin_point: Color32::from_rgb(214, 215, 218),
            axis_x: Color32::from_rgb(255, 0, 0),
            axis_y: Color32::from_rgb(0, 255, 0),
            axis_z: Color32::from_rgb(2, 2, 248),
            construction_plane: Color32::from_rgba_unmultiplied_const(249, 184, 134, 70),
            body: Color32::from_rgb(176, 186, 198),
            body_edge: Color32::from_rgb(40, 44, 52),
            manipulator: Color32::from_rgb(40, 110, 220),
            preview_add: Color32::from_rgb(112, 160, 226),
            preview_cut: Color32::from_rgba_unmultiplied_const(232, 84, 44, 110),
            viewport_top: Color32::from_rgb(240, 243, 247),
            viewport_bottom: Color32::from_rgb(203, 210, 221),
            grid: Color32::from_rgb(196, 202, 212),
            grid_major: Color32::from_rgb(170, 178, 190),
            body_selected: Color32::from_rgb(110, 170, 235),
            sketch: Color32::from_rgb(30, 90, 200),
            sketch_fixed: Color32::from_rgb(20, 20, 20),
            sketch_point: Color32::BLACK,
            sketch_point_free: Color32::from_rgb(30, 90, 200),
            glyph_bg: Color32::WHITE,
            glyph_edge: Color32::from_rgb(90, 150, 90),
            glyph_text: Color32::from_rgb(40, 110, 40),
            overlay: Color32::from_rgba_unmultiplied_const(255, 255, 255, 235),
            cube_face: Color32::from_rgb(236, 236, 236),
            cube_edge: Color32::from_rgb(140, 148, 160),
            rubber_band: Color32::from_rgb(30, 120, 230),
            app_bar_text: Color32::from_rgb(230, 234, 242),
            timeline_item: Color32::WHITE,
            timeline_item_error: Color32::from_rgb(250, 220, 220),
            timeline_marker: Color32::from_rgb(70, 76, 88),
            field: Color32::WHITE,
            box_window: Color32::from_rgb(38, 120, 218),
            box_crossing: Color32::from_rgb(40, 160, 80),
        }
    }

    /// Dark blue-grey: viewport #2e3440 as measured in the reference tool, chrome a little
    /// darker, light text, a blue accent.
    pub const fn dark() -> Tokens {
        let mut t = Tokens::light();
        t.app_bar = Color32::from_rgb(28, 31, 38);
        t.toolbar = Color32::from_rgb(37, 41, 49);
        t.tab_active = Color32::from_rgb(46, 52, 64);
        t.panel = Color32::from_rgb(39, 43, 52);
        t.panel_header = Color32::from_rgb(47, 52, 62);
        t.border = Color32::from_rgb(63, 69, 82);
        t.text = Color32::from_rgb(222, 226, 232);
        t.text_dim = Color32::from_rgb(148, 155, 168);
        t.accent = Color32::from_rgb(58, 136, 238);
        t.accent_soft = Color32::from_rgb(42, 70, 110);
        t.sketch_accent = Color32::from_rgb(110, 200, 140);
        t.error = Color32::from_rgb(240, 100, 96);
        t.warning = Color32::from_rgb(236, 170, 70);
        t.icon = Color32::from_rgb(208, 214, 226);
        t.icon_fill = Color32::from_rgb(86, 112, 150);
        t.timeline = Color32::from_rgb(33, 37, 45);
        t.hover = Color32::from_rgb(54, 61, 74);
        t.hover_edge_halo = Color32::from_rgb(203, 203, 203);
        t.origin_point = Color32::from_rgb(214, 215, 218);
        t.viewport_top = Color32::from_rgb(46, 52, 64);
        t.viewport_bottom = Color32::from_rgb(46, 52, 64);
        t.grid = Color32::from_rgb(59, 65, 76);
        t.grid_major = Color32::from_rgb(73, 79, 90);
        t.body = Color32::from_rgb(152, 156, 162);
        t.body_edge = Color32::from_rgb(16, 16, 18);
        t.sketch = Color32::from_rgb(92, 150, 255);
        t.sketch_fixed = Color32::from_rgb(232, 234, 238);
        t.sketch_point = Color32::from_rgb(232, 234, 238);
        t.sketch_point_free = Color32::from_rgb(92, 150, 255);
        t.glyph_bg = Color32::from_rgb(44, 50, 60);
        t.glyph_edge = Color32::from_rgb(96, 170, 110);
        t.glyph_text = Color32::from_rgb(150, 220, 160);
        t.overlay = Color32::from_rgba_unmultiplied_const(34, 38, 46, 240);
        t.cube_face = Color32::from_rgb(196, 200, 207);
        t.cube_edge = Color32::from_rgb(100, 108, 122);
        t.rubber_band = Color32::from_rgb(92, 160, 255);
        t.timeline_item = Color32::from_rgb(52, 58, 70);
        t.timeline_item_error = Color32::from_rgb(110, 50, 54);
        t.timeline_marker = Color32::from_rgb(170, 178, 192);
        t.field = Color32::from_rgb(30, 34, 41);
        t.box_window = Color32::from_rgb(70, 150, 255);
        t.box_crossing = Color32::from_rgb(80, 200, 120);
        t
    }
}

/// Switch themes (the visuals follow on the next [`apply`]).
pub fn set_dark(dark: bool) {
    DARK.store(dark, Ordering::Relaxed);
}

pub fn apply(ctx: &egui::Context) {
    let t = Tokens::get();
    let mut v = if is_dark() { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.window_stroke = Stroke::new(1.0, t.border);
    v.extreme_bg_color = t.field;
    v.faint_bg_color = t.panel_header;
    v.override_text_color = Some(t.text);
    v.widgets.inactive.weak_bg_fill = t.panel_header;
    v.widgets.inactive.bg_fill = t.panel_header;
    v.widgets.active.weak_bg_fill = t.accent_soft;
    v.selection.bg_fill = t.accent_soft;
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.hovered.bg_fill = t.hover;
    v.window_corner_radius = egui::CornerRadius::same(6);
    ctx.set_theme(if is_dark() { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(6.0, 3.0);
    });
}
