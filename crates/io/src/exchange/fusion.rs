//! Autodesk Fusion `.f3d` / `.f3z` exchange provider (desktop only).
//!
//! Uses [`cadmpeg-codec-f3d`](https://github.com/cadmpeg/cadmpeg) directly (not the open-formats
//! cadmpeg-registry feature set used by FreeCAD/Rhino).

use super::{ExchangeCapabilities, ExchangeFileType, ExchangeImport, ExchangeProviderId};
use crate::{MAX_F3D_BYTES, Result};

pub const FILE_TYPE: ExchangeFileType = ExchangeFileType {
    id: ExchangeProviderId::FusionF3d,
    dialog_label: "Autodesk Fusion",
    extensions: &["f3d", "F3D", "f3z", "F3Z"],
    max_bytes: MAX_F3D_BYTES,
    capabilities: ExchangeCapabilities { open_new_design: true, insert_bodies: true },
};

pub fn read(bytes: &[u8], path: &str) -> Result<ExchangeImport> {
    crate::f3d_open_feature(bytes, path).map(|f| match f {
        crate::F3dOpenFeature::Solid(sf) => ExchangeImport::Solid(sf),
        crate::F3dOpenFeature::Mesh(mf) => ExchangeImport::Mesh(mf),
    })
}

pub fn blocks_native_design(bytes: &[u8]) -> bool {
    crate::looks_like_fusion_archive(bytes)
}
