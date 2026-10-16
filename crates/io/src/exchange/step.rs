use super::{ExchangeCapabilities, ExchangeFileType, ExchangeImport, ExchangeProviderId};
use crate::{MAX_STEP_BYTES, Result, step_import_feature};

pub const FILE_TYPE: ExchangeFileType = ExchangeFileType {
    id: ExchangeProviderId::StepIges,
    dialog_label: "STEP / IGES",
    extensions: &["step", "stp", "STEP", "STP", "igs", "iges", "IGS", "IGES"],
    max_bytes: MAX_STEP_BYTES,
    capabilities: ExchangeCapabilities { open_new_design: true, insert_bodies: true },
};

pub fn read(bytes: &[u8], path: &str) -> Result<ExchangeImport> {
    Ok(ExchangeImport::Solid(step_import_feature(bytes, path)?))
}
