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
const UNIFORM_SIZE: u64 = 112;

/// CPU-side geometry waiting to be uploaded. The model scene changes with the design; the
/// highlight scene (hover, selection, the origin widget) changes often and is small.
#[derive(Default, Clone)]
pub struct GpuScene {
    /// Opaque triangles. A zero normal draws the colour flat (unlit).
    pub tris: Vec<u8>,
    /// Translucent triangles (alpha from the colour), drawn after the opaque ones without
    /// writing depth.
    pub trans: Vec<u8>,
    /// Depth-tested lines.
    pub lines: Vec<u8>,
    /// Lines drawn over everything (active sketch, highlights).
    pub overlay: Vec<u8>,
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

/// Per-frame parameters.
pub struct ViewportCallback {
    pub key: u64,
    pub slot: SceneSlot,
    /// The highlight scene and its key.
    pub hl_key: u64,
    pub hl_slot: SceneSlot,
    /// Column-major view-projection.
    pub view_proj: [[f32; 4]; 4],
    /// Direction toward the eye (for lighting).
    pub back: [f32; 3],
    /// Viewport size in physical pixels.
    pub size_px: [f32; 2],
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
    lines: Option<Batch>,
    overlays: Option<Batch>,
}

impl Batches {
    fn take(&mut self, device: &wgpu::Device, key: u64, slot: &SceneSlot) {
        if self.key == Some(key) {
            return;
        }
        if let Some(sc) = slot.lock().ok().and_then(|mut s| s.take()) {
            self.tris = upload(device, "sc_tris", &sc.tris, TRI_SIZE);
            self.trans = upload(device, "sc_trans", &sc.trans, TRI_SIZE);
            self.lines = upload(device, "sc_lines", &sc.lines, LINE_SIZE);
            self.overlays = upload(device, "sc_overlay", &sc.overlay, LINE_SIZE);
            self.key = Some(key);
        }
    }
}

struct Resources {
    tri: wgpu::RenderPipeline,
    /// Highlight triangles over the model (depth test ≤, no depth write).
    tri_hl: wgpu::RenderPipeline,
    trans: wgpu::RenderPipeline,
    line: wgpu::RenderPipeline,
    overlay: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    linear_out: bool,
    model: Batches,
    highlight: Batches,
}

const SHADER: &str = r#"
struct U {
    vp: mat4x4<f32>,
    back: vec4<f32>,   // xyz = toward the eye, w = 1 for linear output
    screen: vec4<f32>, // viewport width, height (px), depth bias
};
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
};

@vertex
fn vs_tri(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec4<f32>) -> TOut {
    var o: TOut;
    o.pos = u.vp * vec4<f32>(p, 1.0);
    o.n = n;
    o.c = c;
    return o;
}

@fragment
fn fs_tri(i: TOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
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

struct LOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) c: vec4<f32>,
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
    return o;
}

@fragment
fn fs_line(i: LOut) -> @location(0) vec4<f32> {
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
        Resources {
            tri: pipeline("sc_tris", "vs_tri", "fs_tri", tri_layout(), depth(wgpu::CompareFunction::Less, true), None),
            tri_hl: pipeline("sc_tris_hl", "vs_tri", "fs_tri", tri_layout(), depth(wgpu::CompareFunction::LessEqual, false), alpha),
            trans: pipeline("sc_trans", "vs_tri", "fs_tri", tri_layout(), depth(wgpu::CompareFunction::LessEqual, false), alpha),
            line: pipeline("sc_lines", "vs_line", "fs_line", line_layout(), depth(wgpu::CompareFunction::LessEqual, false), alpha),
            overlay: pipeline("sc_overlay", "vs_line", "fs_line", line_layout(), depth(wgpu::CompareFunction::Always, false), alpha),
            uniform,
            bind,
            linear_out: t.format.is_srgb(),
            model: Batches::default(),
            highlight: Batches::default(),
        }
    }
}

fn upload(device: &wgpu::Device, label: &str, bytes: &[u8], stride: usize) -> Option<Batch> {
    if bytes.len() < stride {
        return None;
    }
    Some(Batch {
        buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytes,
            usage: wgpu::BufferUsages::VERTEX,
        }),
        count: u32::try_from(bytes.len() / stride).unwrap_or(0),
    })
}

/// Create the pipelines. Call once with the app's render state and its depth/MSAA settings.
pub fn install(rs: &egui_wgpu::RenderState, depth_bits: u8, samples: u32) -> GpuTarget {
    let t = GpuTarget { format: rs.target_format, depth: egui_wgpu::depth_format_from_bits(depth_bits, 0), samples };
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
        queue.write_buffer(&res.uniform, 0, &uniform_bytes(self, self.size_px[0], self.size_px[1], res.linear_out));
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
        let (m, h) = (&res.model, &res.highlight);
        tris(pass, &res.tri, &m.tris);
        tris(pass, &res.tri_hl, &h.tris);
        lines(pass, &res.line, &m.lines);
        lines(pass, &res.line, &h.lines);
        tris(pass, &res.trans, &m.trans);
        tris(pass, &res.trans, &h.trans);
        lines(pass, &res.overlay, &m.overlays);
        lines(pass, &res.overlay, &h.overlays);
    }
}

/// Uniform bytes for a frame.
pub fn uniform_bytes(cb: &ViewportCallback, w: f32, h: f32, linear: bool) -> Vec<u8> {
    let mut v: Vec<f32> = cb.view_proj.iter().flatten().copied().collect();
    v.extend([cb.back[0], cb.back[1], cb.back[2], if linear { 1.0 } else { 0.0 }]);
    v.extend([w.max(1.0), h.max(1.0), 2e-4, 0.0]);
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}
