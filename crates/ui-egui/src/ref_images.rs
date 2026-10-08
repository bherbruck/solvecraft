//! Canvases and decals: the design's reference images, drawn on their planes under the sketch
//! overlays.

use std::cell::RefCell;
use std::collections::HashMap;

use egui::{Color32, Mesh, Shape, TextureHandle, TextureId, pos2};
use solvecraft_engine::geom::Vec2;

use crate::SolveApp;
use crate::viewport::Proj;

/// Largest texture side; bigger images are scaled down for display.
const MAX_SIDE: u32 = 2048;
/// Grid cells per side, so perspective views bend the image only slightly.
const CELLS: usize = 8;

thread_local! {
    /// Textures by canvas id, with the length of the data they were made from.
    static TEX: RefCell<HashMap<u64, (usize, Option<TextureHandle>)>> = RefCell::new(HashMap::new());
}

fn texture(ctx: &egui::Context, id: u64, data: &str, bytes: impl FnOnce() -> Option<Vec<u8>>) -> Option<TextureId> {
    TEX.with(|t| {
        let mut t = t.borrow_mut();
        if let Some((len, h)) = t.get(&id)
            && *len == data.len()
        {
            return h.as_ref().map(|h| h.id());
        }
        let img = bytes().and_then(|b| image::load_from_memory(&b).ok()).map(|i| {
            let i = if i.width() > MAX_SIDE || i.height() > MAX_SIDE { i.thumbnail(MAX_SIDE, MAX_SIDE) } else { i };
            let rgba = i.to_rgba8();
            egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())
        });
        let h = img.map(|img| ctx.load_texture(format!("sc_canvas_{id}"), img, egui::TextureOptions::LINEAR));
        let out = h.as_ref().map(|h| h.id());
        t.insert(id, (data.len(), h));
        out
    })
}

pub fn show(app: &SolveApp, ui: &egui::Ui, painter: &egui::Painter, proj: &Proj) {
    let doc = &app.session.doc;
    if doc.canvases.is_empty() {
        TEX.with(|t| t.borrow_mut().clear());
        return;
    }
    let (vals, _) = doc.param_values();
    for c in doc.canvases.iter().filter(|c| c.visible) {
        let Ok(plane) = doc.resolve_plane(&vals, &c.plane, 0) else { continue };
        let Some(tex) = texture(ui.ctx(), c.id, &c.data, || c.bytes()) else { continue };
        let [bl, br, _, tl] = c.corners();
        let (u, v) = (br - bl, tl - bl);
        let tint = Color32::from_white_alpha((c.opacity.clamp(0.0, 1.0) * 255.0) as u8);
        let mut mesh = Mesh::with_texture(tex);
        let mut ok = true;
        for j in 0..=CELLS {
            for i in 0..=CELLS {
                let (s, t) = (i as f64 / CELLS as f64, j as f64 / CELLS as f64);
                let p: Vec2 = bl + u * s + v * t;
                let Some(q) = proj.to_screen(plane.to_world(p)) else {
                    ok = false;
                    continue;
                };
                mesh.vertices.push(egui::epaint::Vertex { pos: q, uv: pos2(s as f32, 1.0 - t as f32), color: tint });
            }
        }
        // Behind the camera: skip rather than draw a torn image.
        if !ok {
            continue;
        }
        let w = (CELLS + 1) as u32;
        for j in 0..CELLS as u32 {
            for i in 0..CELLS as u32 {
                let a = j * w + i;
                mesh.add_triangle(a, a + 1, a + w + 1);
                mesh.add_triangle(a, a + w + 1, a + w);
            }
        }
        painter.add(Shape::mesh(mesh));
    }
    TEX.with(|t| t.borrow_mut().retain(|id, _| doc.canvases.iter().any(|c| c.id == *id)));
}
