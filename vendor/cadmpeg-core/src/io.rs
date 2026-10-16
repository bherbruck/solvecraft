// SPDX-License-Identifier: Apache-2.0
//! I/O traits used by codec entry points.

use std::io::{Read, Seek};

/// Object-safe input bound combining [`Read`] and [`Seek`].
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}
