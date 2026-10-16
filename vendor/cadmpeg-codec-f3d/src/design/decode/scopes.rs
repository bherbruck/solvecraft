// SPDX-License-Identifier: Apache-2.0
//! Parse parameter scopes and exact feature-construction frames.

mod assembly_alignment;
mod assembly_carrier_paths;
mod assembly_operand_frames;
mod assembly_operand_paths;
mod axial_assembly;
mod base_feature;
mod coil;
mod combine;
mod component_constructions;
mod copy_paste_bodies;
mod direct_face;
mod draft;
pub(crate) mod extrude;
mod fixed_parameters;
pub(super) mod hole;
pub(crate) mod legacy_class_397;
pub(crate) mod legacy_class_415;
mod legacy_operand_paths;
pub(crate) mod mirror;
pub(crate) mod parameter_scope;
mod path_feature;
mod pattern;
pub(super) mod point_data;
pub(super) mod shared_frames;
mod sheet_metal;
mod solid_primitive;
mod surfaces;
mod thicken_shell;
mod thread;
mod work_geometry;

#[cfg(test)]
mod tests;
