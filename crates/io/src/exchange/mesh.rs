use super::{ExchangeCapabilities, ExchangeFileType, ExchangeImport, ExchangeProviderId};
use crate::{MAX_3MF_BYTES, Result, mesh_import_feature};

pub const FILE_TYPE: ExchangeFileType = ExchangeFileType {
    id: ExchangeProviderId::MeshStl3mf,
    dialog_label: "Mesh (3MF, STL, OBJ, glTF)",
    extensions: &["3mf", "3MF", "stl", "STL", "obj", "OBJ", "glb", "GLB", "gltf", "GLTF"],
    max_bytes: MAX_3MF_BYTES,
    capabilities: ExchangeCapabilities { open_new_design: true, insert_bodies: true },
};

pub fn read(bytes: &[u8], path: &str) -> Result<ExchangeImport> {
    Ok(ExchangeImport::Mesh(mesh_import_feature(bytes, path)?))
}
