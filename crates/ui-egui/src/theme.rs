//! Colours and widget style. A light theme in the spirit of desktop CAD tools; every colour
//! the UI uses comes from [`Tokens`].

use egui::{Color32, Stroke};

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
    /// New surface in a feature preview.
    pub preview_add: Color32,
    /// Removed material in a feature preview (translucent).
    pub preview_cut: Color32,
    // ---- box selection ----
    pub box_window: Color32,
    pub box_crossing: Color32,
}

impl Tokens {
    pub const fn get() -> Tokens {
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
            preview_add: Color32::from_rgb(112, 160, 226),
            preview_cut: Color32::from_rgba_unmultiplied_const(232, 84, 44, 110),
            box_window: Color32::from_rgb(38, 120, 218),
            box_crossing: Color32::from_rgb(40, 160, 80),
        }
    }
}

pub fn apply(ctx: &egui::Context) {
    let t = Tokens::get();
    let mut v = egui::Visuals::light();
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.extreme_bg_color = Color32::WHITE;
    v.selection.bg_fill = t.accent_soft;
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.hovered.bg_fill = t.hover;
    v.window_corner_radius = egui::CornerRadius::same(6);
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(6.0, 3.0);
    });
}
