//! Exchange file providers: one registry for import/open metadata and decode.
//!
//! FreeCAD / Rhino stay on [`crate::native_cad`] (open formats via cadmpeg-registry). Fusion
//! `.f3d`/`.f3z` is a separate commercial-format provider that calls [`crate::f3d`] directly.
//!
//! **Adding a format:** add a submodule with [`ExchangeFileType`] + `read`, register it in
//! [`providers`], extend [`ExchangeProviderId`], and add an arm in [`read_bytes`].

#[cfg(not(target_arch = "wasm32"))]
mod fusion;
mod mesh;
mod step;

use std::path::Path;

use crate::{IoError, MeshFeature, Result, StepFeature, fusion_design_refusal};

/// Stable id for logging, tests and dialog metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExchangeProviderId {
    StepIges,
    #[cfg(not(target_arch = "wasm32"))]
    FusionF3d,
    MeshStl3mf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExchangeCapabilities {
    pub open_new_design: bool,
    pub insert_bodies: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ExchangeFileType {
    pub id: ExchangeProviderId,
    pub dialog_label: &'static str,
    pub extensions: &'static [&'static str],
    pub max_bytes: usize,
    pub capabilities: ExchangeCapabilities,
}

pub fn providers() -> &'static [ExchangeFileType] {
    #[cfg(not(target_arch = "wasm32"))]
    {
        &[step::FILE_TYPE, fusion::FILE_TYPE, mesh::FILE_TYPE]
    }
    #[cfg(target_arch = "wasm32")]
    {
        &[step::FILE_TYPE, mesh::FILE_TYPE]
    }
}

pub fn provider(id: ExchangeProviderId) -> &'static ExchangeFileType {
    match id {
        ExchangeProviderId::StepIges => &step::FILE_TYPE,
        #[cfg(not(target_arch = "wasm32"))]
        ExchangeProviderId::FusionF3d => &fusion::FILE_TYPE,
        ExchangeProviderId::MeshStl3mf => &mesh::FILE_TYPE,
    }
}

fn extension_of(path: &str) -> Option<&str> {
    Path::new(path).extension()?.to_str()
}

fn extension_matches(ext: &str, allowed: &[&str]) -> bool {
    allowed.iter().any(|a| ext.eq_ignore_ascii_case(a))
}

pub fn provider_for_path(path: &str) -> Option<ExchangeProviderId> {
    let ext = extension_of(path)?;
    providers().iter().find(|p| extension_matches(ext, p.extensions)).map(|p| p.id)
}

pub fn matches_path(id: ExchangeProviderId, path: &str) -> bool {
    provider_for_path(path) == Some(id)
}

pub enum ExchangeImport {
    Solid(StepFeature),
    Mesh(MeshFeature),
}

pub fn read_path(path: &str) -> Result<ExchangeImport> {
    let id = provider_for_path(path).ok_or_else(|| IoError::Format(path.to_string()))?;
    let max = provider(id).max_bytes;
    let bytes = crate::vfs::read(path, max).map_err(|e| IoError::Invalid(format!("{path}: {e}")))?;
    read_bytes(id, &bytes, path)
}

pub fn read_bytes(id: ExchangeProviderId, bytes: &[u8], path: &str) -> Result<ExchangeImport> {
    match id {
        ExchangeProviderId::StepIges => step::read(bytes, path),
        #[cfg(not(target_arch = "wasm32"))]
        ExchangeProviderId::FusionF3d => fusion::read(bytes, path),
        ExchangeProviderId::MeshStl3mf => mesh::read(bytes, path),
    }
}

pub fn opens_new_design(path: &str) -> bool {
    provider_for_path(path).is_some_and(|id| provider(id).capabilities.open_new_design)
}

pub fn can_insert(path: &str) -> bool {
    provider_for_path(path).is_some_and(|id| provider(id).capabilities.insert_bodies)
}

pub fn refuse_native_design(bytes: &[u8]) -> Option<IoError> {
    #[cfg(not(target_arch = "wasm32"))]
    if fusion::blocks_native_design(bytes) {
        return Some(fusion_design_refusal());
    }
    #[cfg(target_arch = "wasm32")]
    if crate::looks_like_fusion_archive(bytes) {
        return Some(fusion_design_refusal());
    }
    None
}

/// Extensions for the combined Open filter (native design + exchange + FreeCAD/Rhino).
pub fn open_dialog_combined_extensions() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    let mut push = |e: &'static str| {
        if !out.iter().any(|x| x.eq_ignore_ascii_case(e)) {
            out.push(e);
        }
    };
    push(crate::DESIGN_EXT);
    push(crate::LEGACY_DESIGN_EXT);
    push("json");
    for p in providers() {
        for e in p.extensions {
            push(e);
        }
    }
    for e in crate::NATIVE_CAD_EXTS {
        push(e);
        // File dialogs often list mixed-case FreeCAD extensions.
        if e.eq_ignore_ascii_case("fcstd") {
            push("FCStd");
        }
        if e.eq_ignore_ascii_case("3dm") {
            push("3DM");
        }
    }
    out
}

pub fn insert_command_name(path: &str) -> Option<&'static str> {
    match provider_for_path(path)? {
        ExchangeProviderId::MeshStl3mf => Some("file.insert_mesh"),
        ExchangeProviderId::StepIges => Some("file.insert_step"),
        #[cfg(not(target_arch = "wasm32"))]
        ExchangeProviderId::FusionF3d => Some("file.insert_step"),
    }
}

pub fn is_fusion_path(path: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        matches_path(ExchangeProviderId::FusionF3d, path)
    }
    #[cfg(target_arch = "wasm32")]
    {
        Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("f3d") || e.eq_ignore_ascii_case("f3z"))
    }
}

pub fn dialog_filters() -> Vec<(&'static str, &'static [&'static str])> {
    let mut out = Vec::new();
    out.push(("STEP", &["step", "stp", "STEP", "STP"] as &[&str]));
    out.push(("IGES", &["igs", "iges", "IGS", "IGES"]));
    out.push(("FreeCAD and Rhino", &["FCStd", "fcstd", "3dm", "3DM"]));
    for p in providers() {
        if p.id == ExchangeProviderId::StepIges {
            continue;
        }
        out.push((p.dialog_label, p.extensions));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_for_path_extensions() {
        assert_eq!(provider_for_path("/a/part.STEP"), Some(ExchangeProviderId::StepIges));
        assert_eq!(provider_for_path("m.stl"), Some(ExchangeProviderId::MeshStl3mf));
        assert_eq!(provider_for_path("m.glb"), Some(ExchangeProviderId::MeshStl3mf));
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(provider_for_path("f.F3Z"), Some(ExchangeProviderId::FusionF3d));
        assert_eq!(provider_for_path("design.solidcraft"), None);
        assert_eq!(provider_for_path("a.fcstd"), None, "FreeCAD stays on native_cad, not exchange");
    }

    #[test]
    fn insert_command_routing() {
        assert_eq!(insert_command_name("a.step"), Some("file.insert_step"));
        assert_eq!(insert_command_name("a.stl"), Some("file.insert_mesh"));
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(insert_command_name("a.f3d"), Some("file.insert_step"));
    }
}
