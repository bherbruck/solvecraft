// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

mod construction;
mod construction_budget;
mod edge_index;
mod face_sources;
mod header_index;
mod recipe_id_limits;
mod recipe_structure_limits;
mod recipes;
mod selection;
mod work_point;

mod body_recipes;

mod construction_paths;
