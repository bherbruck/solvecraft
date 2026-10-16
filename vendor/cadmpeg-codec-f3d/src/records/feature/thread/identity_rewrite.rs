// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignThreadConstruction, DesignThreadDiameters, DesignThreadForm, DesignThreadNominalSize,
};

rewrite_native_record!(DesignThreadConstruction, []; {form, designation_offset, designation, nominal_size, profile, diameters, pitch, face_group_record_indices});
rewrite_native_scalar!(DesignThreadDiameters);
rewrite_native_scalar!(DesignThreadForm);
rewrite_native_record!(DesignThreadNominalSize, []; (field0));
