//! View math shared by the GPU viewport and headless rendering: an orbit camera with
//! perspective or orthographic projection, screen ↔ world mapping and picking rays, plus a small
//! CPU rasterizer (z-buffer, shaded triangles, depth-tested lines) for snapshots without a GPU.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod camera;
mod raster;

pub use camera::{Camera, CameraAnim, Mat4, Quat, StandardView, ease_in_out};
pub use raster::{Canvas, Rgb, Scene, SceneLine, SceneMesh, png, render, render_png, render_transparent};
