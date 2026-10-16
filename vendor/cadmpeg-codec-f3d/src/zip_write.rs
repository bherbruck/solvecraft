//! Deterministic ZIP write options for Fusion archives.
//!
//! The `zip` crate defaults `version made by` to the host OS (`Unix` on macOS
//! and Linux, `Dos` on Windows). Pin Unix so source-less generation, retained
//! patching, and synthetic fixtures stay byte-identical across CI platforms.

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, System};

/// File options with a fixed Unix host OS for cross-platform archive bytes.
pub(crate) fn file_options(method: CompressionMethod) -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(method)
        .system(System::Unix)
}
