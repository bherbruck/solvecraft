//! CPU rasterizer for headless snapshots: z-buffered, smooth-shaded triangles and depth-tested
//! lines, rendered at 2× and downsampled for anti-aliasing.

use std::sync::Arc;

use solvecraft_geom::{Mesh, Vec3};

use crate::Camera;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    fn f(self) -> [f64; 3] {
        [self.0 as f64 / 255.0, self.1 as f64 / 255.0, self.2 as f64 / 255.0]
    }
}

pub struct SceneMesh {
    pub mesh: Arc<Mesh>,
    pub color: Rgb,
    /// Faces in colours of their own: (B-rep face index, colour).
    pub face_colors: Vec<(u32, Rgb)>,
}

pub struct SceneLine {
    pub points: Vec<Vec3>,
    pub color: Rgb,
    /// Width in output pixels.
    pub width: f64,
    /// Draw over everything (sketch geometry being edited).
    pub on_top: bool,
}

#[derive(Default)]
pub struct Scene {
    pub meshes: Vec<SceneMesh>,
    pub lines: Vec<SceneLine>,
    /// Background gradient top → bottom.
    pub background: Option<(Rgb, Rgb)>,
    /// Scene radius for clipping planes.
    pub radius: f64,
}

/// RGBA pixels plus depth.
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
    depth: Vec<f32>,
}

const SS: usize = 2;
const MAX_DIM: usize = 8192;

impl Canvas {
    fn new(w: usize, h: usize) -> Canvas {
        Canvas { w, h, rgba: vec![255; w * h * 4], depth: vec![f32::INFINITY; w * h] }
    }
    fn put(&mut self, x: usize, y: usize, c: [f64; 3], a: f64) {
        let i = (y * self.w + x) * 4;
        if let Some(px) = self.rgba.get_mut(i..i + 3) {
            for (k, v) in px.iter_mut().enumerate() {
                let old = *v as f64 / 255.0;
                *v = ((old * (1.0 - a) + c[k] * a).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
}

fn shade(n: Vec3, view_dir: Vec3, base: [f64; 3]) -> [f64; 3] {
    let n = if n.dot(view_dir) < 0.0 { -n } else { n };
    let key = (view_dir * 0.8 + Vec3::new(-0.3, 0.2, 0.6)).normalized().unwrap_or(view_dir);
    let diff = n.dot(key).max(0.0);
    let fill = n.dot(view_dir).max(0.0);
    let h = (key + view_dir).normalized().unwrap_or(view_dir);
    let spec = n.dot(h).max(0.0).powi(40) * 0.25;
    let k = 0.42 + 0.38 * diff + 0.25 * fill;
    [(base[0] * k + spec).min(1.0), (base[1] * k + spec).min(1.0), (base[2] * k + spec).min(1.0)]
}

/// Render a scene with the camera into an RGBA canvas of `w × h` pixels.
pub fn render(scene: &Scene, cam: &Camera, w: usize, h: usize) -> Canvas {
    let (w, h) = (w.clamp(1, MAX_DIM), h.clamp(1, MAX_DIM));
    let (sw, sh) = (w * SS, h * SS);
    let mut c = Canvas::new(sw, sh);
    let (top, bot) = scene.background.unwrap_or((Rgb(236, 240, 245), Rgb(205, 212, 222)));
    for y in 0..sh {
        let t = y as f64 / sh.max(1) as f64;
        let (a, b) = (top.f(), bot.f());
        let col = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
        for x in 0..sw {
            c.put(x, y, col, 1.0);
        }
    }
    let vp = cam.view_proj(sw as f64 / sh as f64, scene.radius.max(1.0));
    let project = |p: Vec3| -> Option<[f64; 3]> {
        let q = vp.apply(p);
        if q[3] <= 1e-9 {
            return None;
        }
        Some([(q[0] / q[3] + 1.0) * 0.5 * sw as f64, (1.0 - q[1] / q[3]) * 0.5 * sh as f64, q[2] / q[3]])
    };
    let view_dir = cam.back();
    for sm in &scene.meshes {
        let m = &sm.mesh;
        let body = sm.color.f();
        let pts: Vec<Option<[f64; 3]>> = m.positions.iter().map(|p| project(*p)).collect();
        for (ti, t) in m.triangles.iter().enumerate() {
            let face = m.tri_face.get(ti).copied();
            let base = sm.face_colors.iter().find(|(f, _)| Some(*f) == face).map_or(body, |(_, c)| c.f());
            let (Some(Some(a)), Some(Some(b)), Some(Some(cc))) = (pts.get(t[0] as usize), pts.get(t[1] as usize), pts.get(t[2] as usize)) else {
                continue;
            };
            let ns: Vec<Vec3> = t.iter().map(|k| m.normals.get(*k as usize).copied().unwrap_or(Vec3::Z)).collect();
            let (x0, x1) = (a[0].min(b[0]).min(cc[0]).floor().max(0.0), a[0].max(b[0]).max(cc[0]).ceil().min(sw as f64 - 1.0));
            let (y0, y1) = (a[1].min(b[1]).min(cc[1]).floor().max(0.0), a[1].max(b[1]).max(cc[1]).ceil().min(sh as f64 - 1.0));
            if !(x0 <= x1 && y0 <= y1) {
                continue;
            }
            let area = (b[0] - a[0]) * (cc[1] - a[1]) - (b[1] - a[1]) * (cc[0] - a[0]);
            if area.abs() < 1e-12 {
                continue;
            }
            for y in y0 as usize..=y1 as usize {
                for x in x0 as usize..=x1 as usize {
                    let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                    let w0 = ((b[0] - px) * (cc[1] - py) - (b[1] - py) * (cc[0] - px)) / area;
                    let w1 = ((cc[0] - px) * (a[1] - py) - (cc[1] - py) * (a[0] - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-9 || w1 < -1e-9 || w2 < -1e-9 {
                        continue;
                    }
                    let z = (w0 * a[2] + w1 * b[2] + w2 * cc[2]) as f32;
                    let i = y * sw + x;
                    if c.depth.get(i).is_some_and(|d| z < *d) {
                        if let Some(d) = c.depth.get_mut(i) {
                            *d = z;
                        }
                        let n = (ns[0] * w0 + ns[1] * w1 + ns[2] * w2).normalized().unwrap_or(Vec3::Z);
                        c.put(x, y, shade(n, view_dir, base), 1.0);
                    }
                }
            }
        }
    }
    for l in &scene.lines {
        let col = l.color.f();
        let half = (l.width * SS as f64 * 0.5).max(0.5);
        for seg in l.points.windows(2) {
            let (Some(p), Some(q)) = (seg.first(), seg.get(1)) else { continue };
            let (Some(a), Some(b)) = (project(*p), project(*q)) else { continue };
            let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            let steps = (len.ceil() as usize).clamp(1, 20_000);
            for k in 0..=steps {
                let t = k as f64 / steps as f64;
                let (x, y, z) = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t);
                let r = half.ceil() as i64;
                for dy in -r..=r {
                    for dx in -r..=r {
                        let (ix, iy) = (x.floor() as i64 + dx, y.floor() as i64 + dy);
                        if ix < 0 || iy < 0 || ix >= sw as i64 || iy >= sh as i64 {
                            continue;
                        }
                        let d = ((ix as f64 + 0.5 - x).powi(2) + (iy as f64 + 0.5 - y).powi(2)).sqrt();
                        let cov = (half + 0.5 - d).clamp(0.0, 1.0);
                        if cov <= 0.0 {
                            continue;
                        }
                        let i = iy as usize * sw + ix as usize;
                        let visible = l.on_top || c.depth.get(i).is_some_and(|dd| (z as f32) <= *dd + 2e-4);
                        if visible {
                            c.put(ix as usize, iy as usize, col, cov);
                        }
                    }
                }
            }
        }
    }
    // Downsample.
    let mut out = Canvas::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let i = ((y * SS + sy) * sw + x * SS + sx) * 4;
                    for (k, a) in acc.iter_mut().enumerate() {
                        *a += c.rgba.get(i + k).copied().unwrap_or(0) as u32;
                    }
                }
            }
            let o = (y * w + x) * 4;
            for (k, a) in acc.iter().enumerate() {
                if let Some(v) = out.rgba.get_mut(o + k) {
                    *v = (*a / (SS * SS) as u32) as u8;
                }
            }
        }
    }
    out
}

/// Render to PNG bytes.
pub fn render_png(scene: &Scene, cam: &Camera, w: usize, h: usize) -> Option<Vec<u8>> {
    let c = render(scene, cam, w, h);
    png(c.w, c.h, c.rgba)
}

/// RGBA pixels (`w × h`) to PNG bytes.
pub fn png(w: usize, h: usize, rgba: Vec<u8>) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(u32::try_from(w).ok()?, u32::try_from(h).ok()?, rgba)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).ok()?;
    Some(buf.into_inner())
}

/// Render on a transparent background: the scene drawn over black and over white, and the
/// alpha recovered from the difference (exact for the anti-aliased edges).
pub fn render_transparent(scene: &mut Scene, cam: &Camera, w: usize, h: usize) -> Canvas {
    let keep = scene.background;
    scene.background = Some((Rgb(0, 0, 0), Rgb(0, 0, 0)));
    let black = render(scene, cam, w, h);
    scene.background = Some((Rgb(255, 255, 255), Rgb(255, 255, 255)));
    let mut out = render(scene, cam, w, h);
    scene.background = keep;
    for (o, b) in out.rgba.as_chunks_mut::<4>().0.iter_mut().zip(black.rgba.as_chunks::<4>().0) {
        // white - black = 255 (1 - alpha) in every channel; average them.
        let d: u32 = (0..3).map(|k| u32::from(o[k].saturating_sub(b[k]))).sum();
        let a = 255.0 - d as f64 / 3.0;
        for k in 0..3 {
            o[k] = if a > 0.5 { (f64::from(b[k]) * 255.0 / a).round().clamp(0.0, 255.0) as u8 } else { 0 };
        }
        o[3] = a.round().clamp(0.0, 255.0) as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StandardView;

    #[test]
    fn renders_a_triangle() {
        let mesh = Mesh {
            positions: vec![Vec3::new(-10.0, 0.0, -10.0), Vec3::new(10.0, 0.0, -10.0), Vec3::new(0.0, 0.0, 10.0)],
            normals: vec![Vec3::new(0.0, -1.0, 0.0); 3],
            triangles: vec![[0, 1, 2]],
            tri_face: vec![0],
            edges: vec![],
            seams: vec![],
            edge_faces: vec![],
        };
        let scene = Scene {
            meshes: vec![SceneMesh { mesh: Arc::new(mesh), color: Rgb(200, 30, 30), face_colors: Vec::new() }],
            lines: vec![],
            background: None,
            radius: 20.0,
        };
        let mut cam = Camera { distance: 50.0, ..Default::default() };
        cam.set_view(StandardView::Front);
        let c = render(&scene, &cam, 64, 64);
        let i = (32 * 64 + 32) * 4;
        assert!(c.rgba[i] > c.rgba[i + 1] + 50, "centre pixel is red: {:?}", &c.rgba[i..i + 4]);
        assert!(render_png(&scene, &cam, 16, 16).unwrap().starts_with(&[0x89, b'P', b'N', b'G']));
        // Hostile sizes are clamped.
        assert_eq!(render(&scene, &cam, 0, usize::MAX).w, 1);
    }
}
