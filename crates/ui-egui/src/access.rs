//! Accessibility Analysis: the model's height map seen from the access direction, for the GPU
//! to tell reachable surface from surface under something else.

use std::cell::RefCell;
use std::sync::Arc;

use solvecraft_engine::SurfaceAnalysis;
use solvecraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::gpu::AccessMap;

/// Map resolution (pixels per side).
const SIZE: u32 = 1024;

thread_local! {
    /// The last map, by (model revision, direction).
    static CACHE: RefCell<Option<((u64, [u64; 3]), AccessMap)>> = const { RefCell::new(None) };
    static VERSION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The depth map for the active Accessibility Analysis, if any.
pub fn map(app: &SolveApp) -> Option<AccessMap> {
    let Some(SurfaceAnalysis::Access { dir }) = app.session.analysis else { return None };
    let key = (app.session.revision, [dir.x.to_bits(), dir.y.to_bits(), dir.z.to_bits()]);
    if let Some(m) = CACHE.with(|c| c.borrow().as_ref().filter(|(k, _)| *k == key).map(|(_, m)| m.clone())) {
        return Some(m);
    }
    let st = app.session.world_state();
    let meshes: Vec<_> = st.bodies.iter().filter(|b| !app.ui.hidden_bodies.contains(&b.name)).map(|b| b.mesh()).collect();
    // Every new map gets a new version (the GPU uploads it once).
    let version = VERSION.with(|v| {
        v.set(v.get() + 1);
        v.get()
    });
    let m = build(&meshes.iter().map(|m| m.as_ref()).collect::<Vec<_>>(), dir, version)?;
    CACHE.with(|c| *c.borrow_mut() = Some((key, m.clone())));
    Some(m)
}

/// Rasterise the triangles seen along `dir` (orthographic), keeping the highest point per pixel.
fn build(meshes: &[&solvecraft_engine::geom::Mesh], dir: Vec3, version: u64) -> Option<AccessMap> {
    let d = dir.normalized()?;
    let u = d.cross(if d.x.abs() < 0.9 { Vec3::X } else { Vec3::Y }).normalized()?;
    let v = d.cross(u);
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for m in meshes {
        for p in &m.positions {
            let (a, b) = (p.dot(u), p.dot(v));
            lo = [lo[0].min(a), lo[1].min(b)];
            hi = [hi[0].max(a), hi[1].max(b)];
        }
    }
    let ext = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    if !(ext.is_finite() && ext > 1e-9) {
        return None;
    }
    let n = SIZE as usize;
    let scale = (SIZE - 2) as f64 / ext;
    // One pixel of margin on each side.
    let (ou, ov) = (-lo[0] + 1.0 / scale, -lo[1] + 1.0 / scale);
    let mut depth = vec![f32::NEG_INFINITY; n * n];
    for m in meshes {
        for t in &m.triangles {
            let q: Vec<(f64, f64, f64)> =
                t.iter().filter_map(|i| m.positions.get(*i as usize)).map(|p| ((p.dot(u) + ou) * scale, (p.dot(v) + ov) * scale, p.dot(d))).collect();
            let [a, b, c] = q[..] else { continue };
            let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            let (x0, x1) = (a.0.min(b.0).min(c.0).floor().max(0.0) as usize, (a.0.max(b.0).max(c.0).ceil() as usize).min(n - 1));
            let (y0, y1) = (a.1.min(b.1).min(c.1).floor().max(0.0) as usize, (a.1.max(b.1).max(c.1).ceil() as usize).min(n - 1));
            // Edge-on triangles: their vertices still count.
            if area.abs() < 1e-12 {
                for p in [a, b, c] {
                    let (x, y) = (p.0 as usize, p.1 as usize);
                    if let Some(z) = depth.get_mut(y * n + x) {
                        *z = z.max(p.2 as f32);
                    }
                }
                continue;
            }
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                    let w0 = ((b.0 - px) * (c.1 - py) - (b.1 - py) * (c.0 - px)) / area;
                    let w1 = ((c.0 - px) * (a.1 - py) - (c.1 - py) * (a.0 - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-9 || w1 < -1e-9 || w2 < -1e-9 {
                        continue;
                    }
                    let z = (w0 * a.2 + w1 * b.2 + w2 * c.2) as f32;
                    if let Some(cell) = depth.get_mut(y * n + x) {
                        *cell = cell.max(z);
                    }
                }
            }
        }
    }
    Some(AccessMap {
        version,
        size: SIZE,
        depth: Arc::new(depth),
        u: [u.x as f32, u.y as f32, u.z as f32, ou as f32],
        v: [v.x as f32, v.y as f32, v.z as f32, ov as f32],
        scale: scale as f32,
    })
}
