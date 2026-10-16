// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignLoftConstruction, DesignPipeConstruction, DesignRevolveConstruction,
    DesignSweepConstruction,
};

rewrite_native_record!(DesignLoftConstruction, []; {operation, operation_offset});
rewrite_native_record!(DesignPipeConstruction, []; {operation, operation_offset, section_shape, section_shape_offset, filled, filled_offset, values, record_indexes, value_offsets});
rewrite_native_scalar!(DesignRevolveConstruction);
rewrite_native_record!(DesignSweepConstruction, []; {operation, operation_offset, values, record_indexes, value_offsets});
