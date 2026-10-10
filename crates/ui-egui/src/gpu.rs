//! GPU viewport: shaded triangles and screen-space wide lines with a depth buffer, drawn in an
//! `egui_wgpu` paint callback. Geometry is uploaded when the scene changes; each frame only the
//! camera uniform changes.

use std::sync::{Arc, Mutex};

use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;

/// Triangle vertex: position (3 × f32), normal (3 × f32), sRGB colour (4 × u8).
pub const TRI_SIZE: usize = 28;
/// Line instance: a, b (3 × f32 each), colour (4 × u8), width in pixels (f32).
pub const LINE_SIZE: usize = 32;
const UNIFORM_SIZE: u64 = 208;

/// CPU-side geometry waiting to be uploaded. The model scene changes with the design; the
/// highlight scene (hover, selection, the origin widget) changes often and is small.
#[derive(Default, Clone)]
pub struct GpuScene {
    /// Opaque triangles. A zero normal draws the colour flat (unlit).
    pub tris: Vec<u8>,
    /// Translucent triangles (alpha from the colour), drawn after the opaque ones without
    /// writing depth.
    pub trans: Vec<u8>,
    /// See-through bodies and faces (appearance opacity below 1), closed and outward-wound:
    /// their far sides are drawn first, then their near sides, so they read like glass.
    pub glass: Vec<u8>,
    /// Depth-tested lines.
    pub lines: Vec<u8>,
    /// Lines drawn only where the model hides them (hidden edges).
    pub hidden: Vec<u8>,
    /// Lines drawn over everything (active sketch, highlights).
    pub overlay: Vec<u8>,
    /// Translucent triangles pushed slightly back in depth, so they only show where nothing
    /// coincides with them (removed material in a preview).
    pub ghost: Vec<u8>,
    /// Translucent triangles drawn through everything (a cut's tool body).
    pub xray: Vec<u8>,
    /// Lines drawn first, under everything (the ground grid: the model hides it, it never
    /// hides the model).
    pub under: Vec<u8>,
}

impl GpuScene {
    pub fn tri(&mut self, p: [f32; 3], n: [f32; 3], c: [u8; 4]) {
        for v in p.iter().chain(&n) {
            self.tris.extend_from_slice(&v.to_le_bytes());
        }
        self.tris.extend_from_slice(&c);
    }
    pub fn trans_tri(&mut self, p: [f32; 3], n: [f32; 3], c: [u8; 4]) {
        for v in p.iter().chain(&n) {
            self.trans.extend_from_slice(&v.to_le_bytes());
        }
        self.trans.extend_from_slice(&c);
    }
    /// A line shown only where the model hides it.
    pub fn hidden_line(&mut self, a: [f32; 3], b: [f32; 3], c: [u8; 4], width: f32) {
        for v in a.iter().chain(&b) {
            self.hidden.extend_from_slice(&v.to_le_bytes());
        }
        self.hidden.extend_from_slice(&c);
        self.hidden.extend_from_slice(&width.to_le_bytes());
    }
    pub fn glass_tri(&mut self, p: [f32; 3], n: [f32; 3], c: [u8; 4]) {
        for v in p.iter().chain(&n) {
            self.glass.extend_from_slice(&v.to_le_bytes());
        }
        self.glass.extend_from_slice(&c);
    }
    pub fn ghost_tri(&mut self, p: [f32; 3], n: [f32; 3], c: [u8; 4]) {
        for v in p.iter().chain(&n) {
            self.ghost.extend_from_slice(&v.to_le_bytes());
        }
        self.ghost.extend_from_slice(&c);
    }
    pub fn xray_tri(&mut self, p: [f32; 3], n: [f32; 3], c: [u8; 4]) {
        for v in p.iter().chain(&n) {
            self.xray.extend_from_slice(&v.to_le_bytes());
        }
        self.xray.extend_from_slice(&c);
    }
    /// A line under everything (see [`GpuScene::under`]).
    pub fn under_line(&mut self, a: [f32; 3], b: [f32; 3], c: [u8; 4], width: f32) {
        for v in a.iter().chain(&b) {
            self.under.extend_from_slice(&v.to_le_bytes());
        }
        self.under.extend_from_slice(&c);
        self.under.extend_from_slice(&width.to_le_bytes());
    }
    pub fn line(&mut self, a: [f32; 3], b: [f32; 3], c: [u8; 4], width: f32, on_top: bool) {
        let out = if on_top { &mut self.overlay } else { &mut self.lines };
        for v in a.iter().chain(&b) {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&c);
        out.extend_from_slice(&width.to_le_bytes());
    }
}

pub type SceneSlot = Arc<Mutex<Option<GpuScene>>>;

/// A reference image (canvas or decal): a textured quad, depth-tested against the model.
#[derive(Clone)]
pub struct GpuImage {
    pub id: u64,
    /// Changes when the pixels change (the texture is re-uploaded).
    pub version: u64,
    pub size: [u32; 2],
    /// Straight RGBA8, `size[0] * size[1] * 4` bytes.
    pub rgba: Arc<Vec<u8>>,
    /// Bottom-left, bottom-right, top-right, top-left of the image.
    pub corners: [[f32; 3]; 4],
    pub opacity: f32,
}

/// Image vertex: position (3 × f32), uv (2 × f32), opacity (f32).
const IMG_SIZE: usize = 24;

/// Per-frame parameters.
pub struct ViewportCallback {
    pub key: u64,
    pub slot: SceneSlot,
    /// The highlight scene and its key.
    pub hl_key: u64,
    pub hl_slot: SceneSlot,
    /// The live preview scene and its key.
    pub pv_key: u64,
    pub pv_slot: SceneSlot,
    /// Column-major view-projection.
    pub view_proj: [[f32; 4]; 4],
    /// Direction toward the eye (for lighting).
    pub back: [f32; 3],
    /// Viewport size in physical pixels.
    pub size_px: [f32; 2],
    /// Section cut: (normal, d); fragments with normal·p > d are hidden. A zero normal: none.
    pub clip: [f32; 4],
    /// Colour of the inside of cut bodies (straight sRGBA, 0..1).
    pub cap: [f32; 4],
    /// Canvases and decals.
    pub images: Vec<GpuImage>,
    /// Surface analysis: (mode: 0 none, 1 zebra, 2 draft, 3 curvature, 4 environment,
    /// 5 accessibility; parameter; scale; size) and the pull / access direction.
    pub analysis: [f32; 8],
    /// Accessibility: the depth map seen from the access direction.
    pub access: Option<AccessMap>,
    /// Draw silhouette outlines around the model (visual styles with edges).
    pub outline: bool,
    /// Eye position and 1 for a perspective camera (0: orthographic), for the section cap's depth.
    pub eye: [f32; 4],
}

/// Heights of the model seen from a direction (an orthographic depth map): a point is reachable
/// from that direction when nothing lies above it.
#[derive(Clone)]
pub struct AccessMap {
    pub version: u64,
    pub size: u32,
    /// Row-major heights along the direction (−∞ where nothing is).
    pub depth: Arc<Vec<f32>>,
    /// Map axes: pixel = (p·u + u.w, p·v + v.w) × scale.
    pub u: [f32; 4],
    pub v: [f32; 4],
    pub scale: f32,
}

struct Batch {
    buffer: wgpu::Buffer,
    count: u32,
}

#[derive(Default)]
struct Batches {
    key: Option<u64>,
    tris: Option<Batch>,
    trans: Option<Batch>,
    glass: Option<Batch>,
    /// The glass triangles as built, and the view direction they were last sorted for.
    glass_cpu: Vec<u8>,
    glass_sorted_for: Option<[f32; 3]>,
    lines: Option<Batch>,
    hidden: Option<Batch>,
    overlays: Option<Batch>,
    ghost: Option<Batch>,
    xray: Option<Batch>,
    under: Option<Batch>,
}

impl Batches {
    fn take(&mut self, device: &wgpu::Device, key: u64, slot: &SceneSlot) {
        if self.key == Some(key) {
            return;
        }
        if let Some(sc) = slot.lock().ok().and_then(|mut s| s.take()) {
            self.tris = upload(device, "sc_tris", &sc.tris, TRI_SIZE);
            self.trans = upload(device, "sc_trans", &sc.trans, TRI_SIZE);
            self.glass = upload_with(device, "sc_glass", &sc.glass, TRI_SIZE, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST);
            self.glass_cpu = sc.glass;
            self.glass_sorted_for = None;
            self.lines = upload(device, "sc_lines", &sc.lines, LINE_SIZE);
            self.hidden = upload(device, "sc_hidden", &sc.hidden, LINE_SIZE);
            self.overlays = upload(device, "sc_overlay", &sc.overlay, LINE_SIZE);
            self.ghost = upload(device, "sc_ghost", &sc.ghost, TRI_SIZE);
            self.xray = upload(device, "sc_xray", &sc.xray, TRI_SIZE);
            self.under = upload(device, "sc_under", &sc.under, LINE_SIZE);
            self.key = Some(key);
        }
    }
}

struct Resources {
    tri: wgpu::RenderPipeline,
    /// While a section is shown (when the depth buffer has a stencil): `tri` marking where the
    /// cap shows, then the cap again at the section plane's depth, so it hides the inside.
    tri_cut: Option<(wgpu::RenderPipeline, wgpu::RenderPipeline)>,
    /// Highlight triangles over the model (depth test ≤, no depth write).
    tri_hl: wgpu::RenderPipeline,
    trans: wgpu::RenderPipeline,
    /// Glass: far sides (front faces culled), then near sides (back faces culled).
    glass_far: wgpu::RenderPipeline,
    glass_near: wgpu::RenderPipeline,
    ghost: wgpu::RenderPipeline,
    xray: wgpu::RenderPipeline,
    line: wgpu::RenderPipeline,
    /// Lines behind the model (depth test >).
    line_hidden: wgpu::RenderPipeline,
    overlay: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    linear_out: bool,
    model: Batches,
    highlight: Batches,
    preview: Batches,
    image: wgpu::RenderPipeline,
    image_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Textures by image id: (version, bind group).
    textures: std::collections::HashMap<u64, (u64, wgpu::BindGroup)>,
    /// This frame's quads: (image id, vertices).
    quads: Vec<(u64, Batch)>,
    access_bgl: wgpu::BindGroupLayout,
    /// The accessibility depth map (a 1 × 1 stand-in when there is none) and its version.
    access_bind: wgpu::BindGroup,
    access_version: u64,
}

fn access_texture(device: &wgpu::Device, queue: Option<&wgpu::Queue>, bgl: &wgpu::BindGroupLayout, size: u32, depth: &[f32]) -> wgpu::BindGroup {
    let extent = wgpu::Extent3d { width: size.max(1), height: size.max(1), depth_or_array_layers: 1 };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sc_access"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    if let Some(q) = queue
        && depth.len() == (size as usize) * (size as usize)
    {
        let bytes: Vec<u8> = depth.iter().flat_map(|x| x.to_le_bytes()).collect();
        q.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * size), rows_per_image: Some(size) },
            extent,
        );
    }
    let view = tex.create_view(&Default::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sc_access"),
        layout: bgl,
        entries: &[wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&view) }],
    })
}

const SHADER: &str = r#"
struct U {
    vp: mat4x4<f32>,
    back: vec4<f32>,   // xyz = toward the eye, w = 1 for linear output
    screen: vec4<f32>, // viewport width, height (px), depth bias, silhouettes on (1)
    clip: vec4<f32>,   // section plane: normal, d (zero normal: no section)
    cap: vec4<f32>,    // colour of the inside of cut bodies
    ana: vec4<f32>,    // surface analysis: mode (1 zebra, 2 draft, 3 curvature), parameter
    pull: vec4<f32>,   // draft pull direction
    acc_u: vec4<f32>,  // accessibility map axes (xyz, offset)
    acc_v: vec4<f32>,
    eye: vec4<f32>,    // eye position, w = 1 for perspective (0: orthographic, looking along -back)
};

@group(1) @binding(2) var acc_t: texture_2d<f32>;

fn clipped(p: vec3<f32>) -> bool {
    return length(u.clip.xyz) > 0.5 && dot(u.clip.xyz, p) > u.clip.w;
}
@group(0) @binding(0) var<uniform> u: U;

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(c, pow((c + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4)), c > vec3<f32>(0.04045));
}

fn out_color(c: vec4<f32>) -> vec4<f32> {
    if (u.back.w > 0.5) {
        return vec4<f32>(to_linear(c.rgb), c.a);
    }
    return c;
}

struct TOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) n: vec3<f32>,
    @location(1) c: vec4<f32>,
    @location(2) wp: vec3<f32>,
};

@vertex
fn vs_tri(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec4<f32>) -> TOut {
    var o: TOut;
    o.pos = u.vp * vec4<f32>(p, 1.0);
    o.n = n;
    o.c = c;
    o.wp = p;
    return o;
}

/// Opaque model surfaces: cut by the section plane, the inside shows as a flat cap colour.
@fragment
fn fs_solid(i: TOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let c = solid(i, front);
    return c;
}

struct CutOut {
    @location(0) c: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

/// The section cap (the inside seen through the cut, drawn on the far walls' back faces where
/// they are the visible surface) again, at the depth of the section plane rather than of the
/// far wall, so the cut face hides what lies inside the body instead of showing it like a
/// pane of glass.
@fragment
fn fs_solid_cut(i: TOut, @builtin(front_facing) front: bool) -> CutOut {
    var o: CutOut;
    o.c = solid(i, front);
    o.depth = cap_depth(i.wp, i.pos.z);
    return o;
}

/// Depth of the section plane on the view ray through `wp` (a far wall seen through the cut),
/// never behind the wall itself (`z`).
fn cap_depth(wp: vec3<f32>, z: f32) -> f32 {
    var dir = -u.back.xyz;
    if (u.eye.w > 0.5) { dir = wp - u.eye.xyz; }
    let dn = dot(u.clip.xyz, dir);
    if (abs(dn) < 1e-9) { return z; }
    let p = wp + dir * ((u.clip.w - dot(u.clip.xyz, wp)) / dn);
    let c = u.vp * vec4<f32>(p, 1.0);
    if (c.w <= 0.0) { return z; }
    return clamp(c.z / c.w, 0.0, z);
}

fn solid(i: TOut, front: bool) -> vec4<f32> {
    // Derivatives first, in uniform control flow (browsers' WGSL compilers reject them after
    // a branch that returns; the whole shader module then fails and nothing is drawn).
    let k = curvature(i);
    let ndv = dot(normalize(i.n), normalize(u.back.xyz));
    let fw = fwidth(ndv);
    if (clipped(i.wp)) { discard; }
    if (!front && length(u.clip.xyz) > 0.5) {
        // The cut face in its body's colour (as Fusion's "from component"), tinted toward the
        // cap colour so it reads as a cut, hatched at 45 degrees on screen.
        let base = mix(i.c.rgb, u.cap.rgb, 0.35);
        let h = fract((i.pos.x + i.pos.y) / 9.0);
        return out_color(vec4<f32>(select(base, base * 0.6, h < 0.22), 1.0));
    }
    if (u.ana.x > 0.5 && length(i.n) > 0.5) {
        return analysis(i, k);
    }
    let c = shade(i);
    // Silhouettes (styles with edges): where a curved surface turns away from the eye, a line
    // about 1.5 px wide, on the visible surface only (depth-correct).
    if (u.screen.w > 0.5 && length(i.n) > 0.5 && fw > 1e-6) {
        let a = 1.0 - smoothstep(fw * 0.8, fw * 2.0, abs(ndv));
        return vec4<f32>(mix(c.rgb, out_color(vec4<f32>(0.07, 0.08, 0.10, 1.0)).rgb, a), c.a);
    }
    return c;
}

/// How fast the normal turns across the pixel (per mm).
fn curvature(i: TOut) -> f32 {
    let nn = normalize(i.n);
    let kx = length(dpdx(nn)) / max(length(dpdx(i.wp)), 1e-6);
    let ky = length(dpdy(nn)) / max(length(dpdy(i.wp)), 1e-6);
    return max(kx, ky);
}

/// Surface analysis colours: zebra stripes, draft angle bands, curvature map (`k`: the
/// surface's curvature here, worked out by the caller).
fn analysis(i: TOut, k: f32) -> vec4<f32> {
    let v = normalize(u.back.xyz);
    var n = normalize(i.n);
    if (dot(n, v) < 0.0) { n = -n; }
    let lit = 0.55 + 0.45 * max(dot(n, v), 0.0);
    if (u.ana.x < 1.5) {
        // Reflections of upright light bars standing around the viewer: the stripe follows
        // the reflected ray's sideways direction (horizontal on screen).
        let r = reflect(-v, n);
        var side = cross(v, vec3<f32>(0.0, 0.0, 1.0));
        if (length(side) < 1e-3) { side = vec3<f32>(1.0, 0.0, 0.0); }
        let s = sin(dot(r, normalize(side)) * u.ana.y * 3.14159265);
        let c = select(0.08, 0.95, s > 0.0);
        return out_color(vec4<f32>(vec3<f32>(c), 1.0));
    }
    if (u.ana.x > 4.5) {
        // Reachable from the access direction: facing it and nothing above.
        let d = normalize(u.pull.xyz);
        let size = i32(u.ana.w);
        let px = vec2<i32>(floor(vec2<f32>(dot(i.wp, u.acc_u.xyz) + u.acc_u.w, dot(i.wp, u.acc_v.xyz) + u.acc_v.w) * u.ana.z));
        var top = -1e30;
        if (px.x >= 0 && px.y >= 0 && px.x < size && px.y < size) {
            top = textureLoad(acc_t, px, 0).x;
        }
        let cn = dot(normalize(i.n), d);
        let facing = cn > 0.02;
        // Slack of a pixel and a half, more on slopes (the map holds the pixel's highest point).
        let slack = u.ana.y * (1.0 + sqrt(max(1.0 - cn * cn, 0.0)) / max(cn, 0.05));
        let clear = dot(i.wp, d) >= top - slack;
        var c = vec3<f32>(0.85, 0.22, 0.20);
        if (facing && clear) { c = vec3<f32>(0.20, 0.75, 0.30); }
        return out_color(vec4<f32>(c * lit, 1.0));
    }
    if (u.ana.x > 3.5) {
        // A studio around the model: sky above, a dark floor below, a bright horizon band and
        // two soft light panels, seen in a mirror.
        let r = reflect(-v, n);
        var c = mix(vec3<f32>(0.32, 0.34, 0.38), vec3<f32>(0.80, 0.86, 0.95), smoothstep(-0.05, 0.6, r.z));
        c = c + vec3<f32>(0.6) * exp(-abs(r.z) * 40.0);
        let side = atan2(r.y, r.x);
        c = c + vec3<f32>(0.9) * smoothstep(0.92, 0.98, cos(side * 2.0)) * smoothstep(0.1, 0.3, r.z) * smoothstep(0.8, 0.6, r.z);
        if (r.z < -0.02) {
            // The floor: lighter toward the horizon, with light-panel streaks along it.
            c = mix(vec3<f32>(0.55, 0.56, 0.60), vec3<f32>(0.20, 0.20, 0.22), clamp(-r.z * 1.6, 0.0, 1.0));
            c = c + vec3<f32>(0.35) * smoothstep(0.95, 0.99, cos(side * 3.0));
        }
        return out_color(vec4<f32>(min(c, vec3<f32>(1.0)), 1.0));
    }
    if (u.ana.x < 2.5) {
        let a = degrees(asin(clamp(dot(normalize(i.n), normalize(u.pull.xyz)), -1.0, 1.0)));
        var c = vec3<f32>(0.95, 0.80, 0.15);
        if (a >= u.ana.y) { c = vec3<f32>(0.20, 0.75, 0.30); }
        if (a <= -u.ana.y) { c = vec3<f32>(0.85, 0.22, 0.20); }
        return out_color(vec4<f32>(c * lit, 1.0));
    }
    // Curvature from how fast the normal turns across the pixel.
    let t = clamp(k * u.ana.y, 0.0, 1.0);
    let c = select(mix(vec3<f32>(0.10, 0.80, 0.25), vec3<f32>(0.90, 0.15, 0.10), t * 2.0 - 1.0),
                   mix(vec3<f32>(0.15, 0.35, 0.95), vec3<f32>(0.10, 0.80, 0.25), t * 2.0), t < 0.5);
    return out_color(vec4<f32>(c * lit, 1.0));
}

@fragment
fn fs_tri(i: TOut) -> @location(0) vec4<f32> {
    if (clipped(i.wp)) { discard; }
    return shade(i);
}

fn shade(i: TOut) -> vec4<f32> {
    // A zero normal marks flat colour (selection fills, translucent planes).
    if (length(i.n) < 0.5) {
        return out_color(i.c);
    }
    let v = normalize(u.back.xyz);
    var n = normalize(i.n);
    if (dot(n, v) < 0.0) { n = -n; }
    let key = normalize(v * 0.8 + vec3<f32>(-0.3, 0.2, 0.6));
    let diff = max(dot(n, key), 0.0);
    let fill = max(dot(n, v), 0.0);
    let h = normalize(key + v);
    let spec = pow(max(dot(n, h), 0.0), 40.0) * 0.25;
    let k = 0.42 + 0.38 * diff + 0.25 * fill;
    return out_color(vec4<f32>(min(i.c.rgb * k + vec3<f32>(spec), vec3<f32>(1.0)), i.c.a));
}

@vertex
fn vs_ghost(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec4<f32>) -> TOut {
    var o: TOut;
    o.pos = u.vp * vec4<f32>(p, 1.0);
    o.pos.z = o.pos.z + u.screen.z * 4.0 * o.pos.w;
    o.n = n;
    o.c = c;
    o.wp = p;
    return o;
}

struct LOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) c: vec4<f32>,
    @location(1) wp: vec3<f32>,
};

@vertex
fn vs_line(@builtin(vertex_index) vi: u32, @location(0) a: vec3<f32>, @location(1) b: vec3<f32>, @location(2) c: vec4<f32>, @location(3) w: f32) -> LOut {
    let ca = u.vp * vec4<f32>(a, 1.0);
    let cb = u.vp * vec4<f32>(b, 1.0);
    let half = u.screen.xy * 0.5;
    let sa = ca.xy / max(ca.w, 1e-6) * half;
    let sb = cb.xy / max(cb.w, 1e-6) * half;
    var dir = sb - sa;
    let len = length(dir);
    if (len < 1e-6) { dir = vec2<f32>(1.0, 0.0); } else { dir = dir / len; }
    let nrm = vec2<f32>(-dir.y, dir.x);
    // Two triangles: corners (t, side).
    var ts = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    var ss = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let t = ts[vi];
    let s = ss[vi];
    let base = select(ca, cb, t > 0.5);
    let off = nrm * s * w * 0.5 + dir * (t * 2.0 - 1.0) * w * 0.5;
    var o: LOut;
    o.pos = vec4<f32>(base.xy + off / half * base.w, base.z - u.screen.z * base.w, base.w);
    o.c = c;
    o.wp = select(a, b, t > 0.5);
    return o;
}

@fragment
fn fs_line(i: LOut) -> @location(0) vec4<f32> {
    if (clipped(i.wp)) { discard; }
    return out_color(i.c);
}

struct IOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) a: f32,
    @location(2) wp: vec3<f32>,
};

@group(1) @binding(0) var img_t: texture_2d<f32>;
@group(1) @binding(1) var img_s: sampler;

@vertex
fn vs_img(@location(0) p: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) a: f32) -> IOut {
    var o: IOut;
    o.pos = u.vp * vec4<f32>(p, 1.0);
    // Pulled toward the eye like lines, so a decal on a face is not lost in it.
    o.pos.z = o.pos.z - u.screen.z * o.pos.w;
    o.uv = uv;
    o.a = a;
    o.wp = p;
    return o;
}

@fragment
fn fs_img(i: IOut) -> @location(0) vec4<f32> {
    let c = textureSample(img_t, img_s, i.uv);
    if (clipped(i.wp)) { discard; }
    return out_color(vec4<f32>(c.rgb, c.a * i.a));
}

/// Lines the section never cuts (the grid, highlights drawn on top).
@fragment
fn fs_line_all(i: LOut) -> @location(0) vec4<f32> {
    return out_color(i.c);
}
"#;

fn tri_layout() -> wgpu::VertexBufferLayout<'static> {
    const A: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4];
    wgpu::VertexBufferLayout { array_stride: TRI_SIZE as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &A }
}

fn line_layout() -> wgpu::VertexBufferLayout<'static> {
    const A: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Float32];
    wgpu::VertexBufferLayout { array_stride: LINE_SIZE as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &A }
}

/// The target configuration: colour format, depth format and MSAA sample count of the app.
#[derive(Clone, Copy, Debug)]
pub struct GpuTarget {
    pub format: wgpu::TextureFormat,
    pub depth: Option<wgpu::TextureFormat>,
    pub samples: u32,
}

impl Resources {
    fn new(device: &wgpu::Device, t: GpuTarget) -> Self {
        let module =
            device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sc_viewport"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_viewport"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_viewport"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sc_viewport_u"),
            size: UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sc_viewport"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
        });
        let depth = |compare: wgpu::CompareFunction, write: bool| -> Option<wgpu::DepthStencilState> {
            t.depth.map(|format| wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(write),
                depth_compare: Some(compare),
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        let pipeline = |label: &str,
                        vs: &str,
                        fs: &str,
                        buf: wgpu::VertexBufferLayout<'static>,
                        ds: Option<wgpu::DepthStencilState>,
                        blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some(vs), buffers: &[Some(buf)], compilation_options: Default::default() },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
                depth_stencil: ds,
                multisample: wgpu::MultisampleState { count: t.samples.max(1), mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState { format: t.format, blend, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let alpha = Some(wgpu::BlendState::ALPHA_BLENDING);
        let glass = |label: &str, cull: wgpu::Face| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_tri"),
                    buffers: &[Some(tri_layout())],
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: Some(cull), ..Default::default() },
                depth_stencil: depth(wgpu::CompareFunction::LessEqual, false),
                multisample: wgpu::MultisampleState { count: t.samples.max(1), mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_tri"),
                    targets: &[Some(wgpu::ColorTargetState { format: t.format, blend: alpha, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let image_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_image"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let image_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_image"),
            bind_group_layouts: &[Some(&bgl), Some(&image_bgl)],
            immediate_size: 0,
        });
        const IA: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32];
        let image = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sc_image"),
            layout: Some(&image_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_img"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: IMG_SIZE as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &IA,
                })],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: depth(wgpu::CompareFunction::LessEqual, false),
            multisample: wgpu::MultisampleState { count: t.samples.max(1), mask: !0, alpha_to_coverage_enabled: false },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_img"),
                targets: &[Some(wgpu::ColorTargetState { format: t.format, blend: alpha, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let access_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_access"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let solid_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_solid"),
            bind_group_layouts: &[Some(&bgl), Some(&access_bgl)],
            immediate_size: 0,
        });
        let solid = |label: &str, fs: &str, cull: Option<wgpu::Face>, ds: Option<wgpu::DepthStencilState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&solid_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_tri"),
                    buffers: &[Some(tri_layout())],
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: cull, ..Default::default() },
                depth_stencil: ds,
                multisample: wgpu::MultisampleState { count: t.samples.max(1), mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState { format: t.format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let access_bind = access_texture(device, None, &access_bgl, 1, &[]);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sc_image"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Resources {
            image,
            image_bgl,
            sampler,
            textures: Default::default(),
            quads: Vec::new(),
            tri: solid("sc_tris", "fs_solid", None, depth(wgpu::CompareFunction::Less, true)),
            tri_cut: t.depth.filter(|f| f.has_stencil_aspect()).map(|format| {
                // The visible surface marks the stencil: 1 where it is a back face (the cap).
                use wgpu::StencilOperation::{Keep, Replace, Zero};
                let face = |pass_op| wgpu::StencilFaceState { compare: wgpu::CompareFunction::Always, fail_op: Keep, depth_fail_op: Keep, pass_op };
                let mark = wgpu::DepthStencilState {
                    format,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: wgpu::StencilState { front: face(Zero), back: face(Replace), read_mask: 0xff, write_mask: 0xff },
                    bias: Default::default(),
                };
                // Then the cap's back faces again, only there, at the section plane's depth.
                let only = wgpu::StencilFaceState { compare: wgpu::CompareFunction::Equal, ..face(Keep) };
                let cap = wgpu::DepthStencilState {
                    format,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: wgpu::StencilState { front: only, back: only, read_mask: 0xff, write_mask: 0 },
                    bias: Default::default(),
                };
                (solid("sc_tris_mark", "fs_solid", None, Some(mark)), solid("sc_tris_cap", "fs_solid_cut", Some(wgpu::Face::Front), Some(cap)))
            }),
            access_bgl,
            access_bind,
            access_version: 0,
            tri_hl: pipeline("sc_tris_hl", "vs_tri", "fs_tri", tri_layout(), depth(wgpu::CompareFunction::LessEqual, false), alpha),
            trans: pipeline("sc_trans", "vs_tri", "fs_tri", tri_layout(), depth(wgpu::CompareFunction::LessEqual, false), alpha),
            glass_far: glass("sc_glass_far", wgpu::Face::Front),
            glass_near: glass("sc_glass_near", wgpu::Face::Back),
            ghost: pipeline("sc_ghost", "vs_ghost", "fs_tri", tri_layout(), depth(wgpu::CompareFunction::LessEqual, false), alpha),
            xray: pipeline("sc_xray", "vs_tri", "fs_tri", tri_layout(), depth(wgpu::CompareFunction::Always, false), alpha),
            line: pipeline("sc_lines", "vs_line", "fs_line", line_layout(), depth(wgpu::CompareFunction::LessEqual, false), alpha),
            line_hidden: pipeline("sc_lines_hidden", "vs_line", "fs_line", line_layout(), depth(wgpu::CompareFunction::Greater, false), alpha),
            overlay: pipeline("sc_overlay", "vs_line", "fs_line_all", line_layout(), depth(wgpu::CompareFunction::Always, false), alpha),
            uniform,
            bind,
            linear_out: t.format.is_srgb(),
            model: Batches::default(),
            highlight: Batches::default(),
            preview: Batches::default(),
        }
    }
}

fn upload(device: &wgpu::Device, label: &str, bytes: &[u8], stride: usize) -> Option<Batch> {
    upload_with(device, label, bytes, stride, wgpu::BufferUsages::VERTEX)
}

fn upload_with(device: &wgpu::Device, label: &str, bytes: &[u8], stride: usize, usage: wgpu::BufferUsages) -> Option<Batch> {
    if bytes.len() < stride {
        return None;
    }
    Some(Batch {
        buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytes, usage }),
        count: u32::try_from(bytes.len() / stride).unwrap_or(0),
    })
}

/// Triangles (3 vertices of `TRI_SIZE` bytes each, position first) ordered far to near along
/// the view direction `back` (toward the eye), so blended glass draws back to front.
pub fn sort_back_to_front(bytes: &[u8], back: [f32; 3]) -> Vec<u8> {
    let tri = TRI_SIZE * 3;
    let depth = |t: &[u8]| -> f32 {
        let f = |o: usize| t.get(o..o + 4).and_then(|b| b.try_into().ok()).map_or(0.0, f32::from_le_bytes);
        (0..3).map(|v| (0..3).map(|k| f(v * TRI_SIZE + k * 4) * back[k]).sum::<f32>()).sum::<f32>()
    };
    let mut tris: Vec<(f32, &[u8])> = bytes.chunks_exact(tri).map(|t| (depth(t), t)).collect();
    tris.sort_by(|a, b| a.0.total_cmp(&b.0));
    tris.into_iter().flat_map(|(_, t)| t.iter().copied()).collect()
}

impl Batches {
    /// Re-sort the glass for a new view direction (when it turned more than about 2 degrees).
    fn sort_glass(&mut self, queue: &wgpu::Queue, back: [f32; 3]) {
        let (Some(g), false) = (&self.glass, self.glass_cpu.is_empty()) else { return };
        let turned = self.glass_sorted_for.is_none_or(|s| s[0] * back[0] + s[1] * back[1] + s[2] * back[2] < 0.9994);
        if turned {
            queue.write_buffer(&g.buffer, 0, &sort_back_to_front(&self.glass_cpu, back));
            self.glass_sorted_for = Some(back);
        }
    }
}

/// Create the pipelines. Call once with the app's render state and its depth/stencil/MSAA
/// settings. Without a stencil the section cap does not hide the inside of cut bodies.
pub fn install(rs: &egui_wgpu::RenderState, depth_bits: u8, stencil_bits: u8, samples: u32) -> GpuTarget {
    let t = GpuTarget { format: rs.target_format, depth: egui_wgpu::depth_format_from_bits(depth_bits, stencil_bits), samples };
    let res = Resources::new(&rs.device, t);
    rs.renderer.write().callback_resources.insert(res);
    t
}

impl egui_wgpu::CallbackTrait for ViewportCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(res) = resources.get_mut::<Resources>() else { return Vec::new() };
        res.model.take(device, self.key, &self.slot);
        res.highlight.take(device, self.hl_key, &self.hl_slot);
        res.preview.take(device, self.pv_key, &self.pv_slot);
        res.model.sort_glass(queue, self.back);
        queue.write_buffer(&res.uniform, 0, &uniform_bytes(self, self.size_px[0], self.size_px[1], res.linear_out));
        if let Some(a) = &self.access
            && a.version != res.access_version
        {
            res.access_bind = access_texture(device, Some(queue), &res.access_bgl, a.size, &a.depth);
            res.access_version = a.version;
        }
        // Images: upload new or changed textures, drop removed ones, rebuild the quads.
        res.textures.retain(|id, _| self.images.iter().any(|i| i.id == *id));
        res.quads.clear();
        for img in &self.images {
            let [w, h] = img.size;
            if w == 0 || h == 0 || img.rgba.len() != (w as usize) * (h as usize) * 4 {
                continue;
            }
            if res.textures.get(&img.id).is_none_or(|t| t.0 != img.version) {
                let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
                let tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("sc_image"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    &img.rgba,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
                    size,
                );
                let view = tex.create_view(&Default::default());
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("sc_image"),
                    layout: &res.image_bgl,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&res.sampler) },
                    ],
                });
                res.textures.insert(img.id, (img.version, bind));
            }
            let c = img.corners;
            let uv = [[0.0_f32, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
            let mut bytes = Vec::with_capacity(6 * IMG_SIZE);
            for k in [0, 1, 2, 0, 2, 3] {
                for v in c[k].iter().chain(&uv[k]).chain(std::iter::once(&img.opacity)) {
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
            }
            if let Some(b) = upload(device, "sc_image_quad", &bytes, IMG_SIZE) {
                res.quads.push((img.id, b));
            }
        }
        Vec::new()
    }

    fn paint(&self, info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, resources: &egui_wgpu::CallbackResources) {
        let Some(res) = resources.get::<Resources>() else { return };
        let vp = info.viewport_in_pixels();
        if vp.width_px <= 0 || vp.height_px <= 0 {
            return;
        }
        pass.set_viewport(vp.left_px as f32, vp.top_px as f32, vp.width_px as f32, vp.height_px as f32, 0.0, 1.0);
        pass.set_bind_group(0, &res.bind, &[]);
        let tris = |pass: &mut wgpu::RenderPass<'static>, p: &wgpu::RenderPipeline, b: &Option<Batch>| {
            if let Some(b) = b {
                pass.set_pipeline(p);
                pass.set_vertex_buffer(0, b.buffer.slice(..));
                pass.draw(0..b.count, 0..1);
            }
        };
        let lines = |pass: &mut wgpu::RenderPass<'static>, p: &wgpu::RenderPipeline, b: &Option<Batch>| {
            if let Some(b) = b {
                pass.set_pipeline(p);
                pass.set_vertex_buffer(0, b.buffer.slice(..));
                pass.draw(0..6, 0..b.count);
            }
        };
        let (m, h, pv) = (&res.model, &res.highlight, &res.preview);
        lines(pass, &res.overlay, &m.under);
        pass.set_bind_group(1, &res.access_bind, &[]);
        let cut = res.tri_cut.as_ref().filter(|_| self.clip[..3].iter().map(|c| c * c).sum::<f32>() > 0.25);
        match cut {
            Some((mark, cap)) => {
                pass.set_stencil_reference(1);
                tris(pass, mark, &m.tris);
                tris(pass, mark, &pv.tris);
                tris(pass, cap, &m.tris);
                tris(pass, cap, &pv.tris);
            }
            None => {
                tris(pass, &res.tri, &m.tris);
                tris(pass, &res.tri, &pv.tris);
            }
        }
        tris(pass, &res.tri_hl, &h.tris);
        lines(pass, &res.line, &m.lines);
        lines(pass, &res.line_hidden, &m.hidden);
        lines(pass, &res.line_hidden, &h.hidden);
        lines(pass, &res.line, &pv.lines);
        lines(pass, &res.line, &h.lines);
        for (id, q) in &res.quads {
            if let Some((_, bind)) = res.textures.get(id) {
                pass.set_pipeline(&res.image);
                pass.set_bind_group(1, bind, &[]);
                pass.set_vertex_buffer(0, q.buffer.slice(..));
                pass.draw(0..q.count, 0..1);
            }
        }
        tris(pass, &res.ghost, &pv.ghost);
        tris(pass, &res.glass_far, &m.glass);
        tris(pass, &res.glass_near, &m.glass);
        tris(pass, &res.trans, &m.trans);
        tris(pass, &res.trans, &h.trans);
        tris(pass, &res.xray, &pv.xray);
        lines(pass, &res.overlay, &m.overlays);
        lines(pass, &res.overlay, &h.overlays);
    }
}

/// Uniform bytes for a frame.
pub fn uniform_bytes(cb: &ViewportCallback, w: f32, h: f32, linear: bool) -> Vec<u8> {
    let mut v: Vec<f32> = cb.view_proj.iter().flatten().copied().collect();
    v.extend([cb.back[0], cb.back[1], cb.back[2], if linear { 1.0 } else { 0.0 }]);
    v.extend([w.max(1.0), h.max(1.0), 2e-4, if cb.outline { 1.0 } else { 0.0 }]);
    v.extend(cb.clip);
    v.extend(cb.cap);
    v.extend(cb.analysis);
    match &cb.access {
        Some(a) => {
            v.extend(a.u);
            v.extend(a.v);
        }
        None => v.extend([0.0; 8]),
    }
    v.extend(cb.eye);
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use egui_wgpu::wgpu::naga;

    /// The viewport shader parses and validates (a bad shader would only fail at run time).
    #[test]
    fn shader_is_valid() {
        let module = naga::front::wgsl::parse_str(super::SHADER).unwrap_or_else(|e| panic!("{}", e.emit_to_string(super::SHADER)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap();
    }

    /// Derivatives and implicit-lod sampling run only in uniform control flow: browsers' WGSL
    /// compilers (Tint) reject them after a branch, and the whole viewport shader then fails,
    /// a black screen on the web (naga accepts it). Every fragment entry point makes such calls
    /// (directly or through a helper) before its first branch or return.
    #[test]
    fn derivatives_only_in_uniform_control_flow() {
        // Without comments (their words are not code).
        let src: String = super::SHADER.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n");
        let src = src.as_str();
        let derivs = ["dpdx(", "dpdy(", "fwidth(", "textureSample("];
        // Functions: name -> body.
        let mut fns: Vec<(String, String, bool)> = Vec::new();
        let mut rest = src;
        let mut fragment = false;
        while let Some(at) = rest.find("fn ") {
            let before = &rest[..at];
            if before.contains("@fragment") {
                fragment = true;
            }
            let Some(open) = rest[at..].find('{') else { break };
            let name = rest[at + 3..].split('(').next().unwrap_or("").trim().to_string();
            let start = at + open;
            let mut depth = 0;
            let mut end = start;
            for (k, ch) in rest[start..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = start + k;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            fns.push((name, rest[start..=end].to_string(), fragment));
            fragment = false;
            rest = &rest[end + 1..];
        }
        // Helpers that take derivatives (directly or through another helper).
        let mut uses: Vec<String> = Vec::new();
        for _ in 0..8 {
            for (n, body, _) in &fns {
                if !uses.contains(n) && (derivs.iter().any(|d| body.contains(d)) || uses.iter().any(|u| body.contains(&format!("{u}(")))) {
                    uses.push(n.clone());
                }
            }
        }
        for (n, body, frag) in &fns {
            let calls: Vec<usize> = derivs
                .iter()
                .map(|d| d.to_string())
                .chain(uses.iter().map(|u| format!("{u}(")))
                .flat_map(|c| body.match_indices(&c).map(|(i, _)| i).collect::<Vec<_>>())
                .collect();
            if calls.is_empty() {
                continue;
            }
            if !*frag {
                // A helper: its own derivatives must come before any branch or return in it.
                let first = ["if (", "return", "discard"].iter().filter_map(|k| body.find(k)).min().unwrap_or(usize::MAX);
                for d in derivs {
                    if let Some(i) = body.find(d) {
                        assert!(i < first, "{n}: {d} after a branch");
                    }
                }
                continue;
            }
            let first = ["if (", "return", "discard"].iter().filter_map(|k| body.find(k)).min().unwrap_or(usize::MAX);
            for c in calls {
                assert!(c < first, "fragment {n}: a derivative after a branch or return: {}", &body[c..(c + 40).min(body.len())]);
            }
        }
    }
}

#[cfg(test)]
mod glass_tests {
    use super::*;

    #[test]
    fn glass_is_drawn_far_to_near() {
        // Two triangles at z = 5 and z = -5, given near first; looking down from +Z.
        let mut sc = GpuScene::default();
        for z in [5.0f32, -5.0] {
            for p in [[0.0, 0.0, z], [1.0, 0.0, z], [0.0, 1.0, z]] {
                sc.glass_tri(p, [0.0, 0.0, 1.0], [255, 0, 0, 100]);
            }
        }
        let sorted = sort_back_to_front(&sc.glass, [0.0, 0.0, 1.0]);
        let z0 = f32::from_le_bytes(sorted[8..12].try_into().unwrap());
        assert_eq!(z0, -5.0, "the far triangle first");
        assert_eq!(sorted.len(), sc.glass.len());
        // Seen from below, the order flips.
        let sorted = sort_back_to_front(&sc.glass, [0.0, 0.0, -1.0]);
        assert_eq!(f32::from_le_bytes(sorted[8..12].try_into().unwrap()), 5.0);
    }
}
