// SPDX-License-Identifier: Apache-2.0
//! Integration contracts over synthesized STEP Part 21 exchanges.

use cadmpeg_test_support::{wire, EditableDecodeResult};

use cadmpeg_ir::codec::write::target::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
use cadmpeg_ir::examples::unit_cube;

use crate::archive::tests::{
    codec_detects_and_inspects_ap242_exchange_structure,
    codec_inspects_edition3_sections_and_external_references,
};
use crate::export::write_step;
use crate::reader::dependencies::tests::decode_reports_data_section_external_dependencies;
use crate::reader::geometry::tests::analytic::procedural_step_geometry_round_trips_as_native_entities;
use crate::reader::geometry::tests::units::{
    decode_conical_apex_and_context_plane_angle_units,
    decode_resolves_conversion_units_and_linear_uncertainty,
    decode_transfers_placed_analytic_geometry_in_millimetres,
};
use crate::reader::pmi::tests::{
    ap242_dimension_kinds_emit_concrete_schema_entities,
    common_datum_compartment_round_trips_as_one_precedence,
    decode_transfers_ap242_presentation_pmi, decode_transfers_ap242_semantic_pmi,
    typed_pmi_measure_uses_its_explicit_conversion_unit,
    unresolved_lower_tolerance_does_not_shift_upper_deviation,
};
use crate::reader::presentation::tests::{
    body_color_becomes_per_face_styled_item_presentation,
    face_appearance_binding_styles_the_advanced_face,
    face_override_wins_over_body_color_and_body_fills_the_rest,
    hidden_body_geometry_and_visibility_round_trip,
    presentation_reader_normalizes_invalid_layer_and_common_datum_inputs,
    step_color_assets_round_trip_names_and_tessellation_targets_strictly,
};
use crate::reader::product::tests::{
    ap203_specified_source_formations_build_occurrence_tree,
    decode_builds_occurrence_placement_from_mapped_item,
    decode_builds_product_occurrences_with_relative_placement,
    repeated_subassembly_instances_each_receive_the_subtree,
};
use crate::reader::tessellation::tests::decode_transfers_ap242_one_based_tessellation_indices;
use crate::reader::tests::{
    decode_accounts_for_every_part21_byte,
    decode_preserves_named_opaque_records_with_exact_byte_spans,
};
use crate::reader::topology::tests::faces::face_outer_bound_is_canonicalized_ahead_of_inner_bounds;
use crate::reader::topology::tests::sheets::{
    decode_and_write_singular_vertex_loops, decode_builds_a_valid_ap203_sheet_brep,
    decode_builds_a_valid_connected_sheet_brep, reader_recovers_a_valid_solid_from_writer_output,
};
use crate::reader::topology::tests::shells::every_region_of_a_body_is_retained_as_a_shape_item;
use crate::strings::tests::string_codec_decodes_all_part21_escape_forms_and_round_trips_unicode;
use crate::writer::tests::round_trips::{
    analytic_conics_round_trip_through_step,
    ap242_writer_round_trips_indexed_tessellation_and_exact_body_link,
    nurbs_surface_grid_orientation_is_u_major,
    standalone_geometry_uses_general_shape_representation,
    writer_round_trips_edge_based_wire_bodies, writer_round_trips_product_body_ownership,
    writer_round_trips_rational_nurbs_pcurves, writer_round_trips_rigid_body_placements,
};
use crate::{StepCodec, StepSchema, StepWriteOptions};

fn assert_valid(result: &EditableDecodeResult) {
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{validation:#?}");
    assert!(result.ir().native.namespace("step").is_some());
}

#[test]
fn part21_pipeline_composes_envelope_editions_strings_anchors_opaque_records_and_dependencies() {
    codec_detects_and_inspects_ap242_exchange_structure();
    codec_inspects_edition3_sections_and_external_references();
    string_codec_decodes_all_part21_escape_forms_and_round_trips_unicode();
    decode_reports_data_section_external_dependencies();
    decode_preserves_named_opaque_records_with_exact_byte_spans();
    decode_accounts_for_every_part21_byte();
}

#[test]
fn geometry_pipeline_composes_analytic_conic_nurbs_procedural_and_unit_conversion_paths() {
    decode_transfers_placed_analytic_geometry_in_millimetres();
    decode_conical_apex_and_context_plane_angle_units();
    decode_resolves_conversion_units_and_linear_uncertainty();
    procedural_step_geometry_round_trips_as_native_entities();
    writer_round_trips_rational_nurbs_pcurves().expect("sheet pcurve lanes pair");
    analytic_conics_round_trip_through_step();
    nurbs_surface_grid_orientation_is_u_major();
}

#[test]
fn topology_pipeline_composes_sheets_solids_wires_singular_loops_bounds_and_placements() {
    decode_and_write_singular_vertex_loops();
    decode_builds_a_valid_connected_sheet_brep();
    decode_builds_a_valid_ap203_sheet_brep();
    reader_recovers_a_valid_solid_from_writer_output();
    writer_round_trips_rigid_body_placements();
    writer_round_trips_edge_based_wire_bodies();
    face_outer_bound_is_canonicalized_ahead_of_inner_bounds();
    every_region_of_a_body_is_retained_as_a_shape_item();
}

#[test]
fn product_pipeline_composes_body_ownership_occurrences_mappings_and_recursive_assemblies() {
    writer_round_trips_product_body_ownership();
    decode_builds_product_occurrences_with_relative_placement();
    decode_builds_occurrence_placement_from_mapped_item();
    repeated_subassembly_instances_each_receive_the_subtree();
    ap203_specified_source_formations_build_occurrence_tree();
    standalone_geometry_uses_general_shape_representation();
}

#[test]
fn presentation_pipeline_composes_tessellation_colors_visibility_layers_and_style_overrides() {
    ap242_writer_round_trips_indexed_tessellation_and_exact_body_link();
    step_color_assets_round_trip_names_and_tessellation_targets_strictly();
    decode_transfers_ap242_one_based_tessellation_indices();
    hidden_body_geometry_and_visibility_round_trip();
    body_color_becomes_per_face_styled_item_presentation();
    face_appearance_binding_styles_the_advanced_face();
    face_override_wins_over_body_color_and_body_fills_the_rest();
    presentation_reader_normalizes_invalid_layer_and_common_datum_inputs();
}

#[test]
fn pmi_pipeline_composes_semantic_presentation_dimension_datum_and_tolerance_entities() {
    decode_transfers_ap242_semantic_pmi();
    decode_transfers_ap242_presentation_pmi();
    ap242_dimension_kinds_emit_concrete_schema_entities();
    common_datum_compartment_round_trips_as_one_precedence();
    typed_pmi_measure_uses_its_explicit_conversion_unit();
    unresolved_lower_tolerance_does_not_shift_upper_deviation();
}

#[test]
fn writer_pipeline_round_trips_the_full_cube_across_schemas_and_refuses_lossy_strict_output() {
    let ir = unit_cube().expect("unit cube fixture is admitted");
    for schema in [
        StepSchema::Ap203Edition1,
        StepSchema::Ap203Edition2,
        StepSchema::Ap214,
        StepSchema::Ap242Edition1,
        StepSchema::Ap242Edition2,
        StepSchema::Ap242Edition3,
    ] {
        let options = StepWriteOptions::default();
        let mut bytes = Vec::new();
        write_step(&ir, &mut bytes, schema, &options).expect("STEP cube write");
        let mut repeated = Vec::new();
        write_step(&ir, &mut repeated, schema, &options).expect("repeat STEP cube write");
        assert_eq!(
            bytes, repeated,
            "STEP output must be deterministic for {schema:?}"
        );
        assert_eq!(
            cadmpeg_test_support::detection::confidence(&StepCodec::default(), &bytes),
            Confidence::High
        );
        let result = EditableDecodeResult::from(
            StepCodec::default()
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .expect("STEP cube decode"),
        );
        assert_eq!(result.ir().model.bodies.len(), 1);
        assert_eq!(result.ir().model.faces.len(), 6);
        assert_valid(&result);

        let mut edited = result.ir().clone();
        edited
            .model
            .points
            .first_mut()
            .expect("unit cube has a point")
            .position()
            .get()
            .x += 1.0;
        let expected_model = edited.model.clone();
        let codec = StepCodec {
            options: options.clone(),
        };
        let plan = codec
            .plan(
                EncodeInput::new(&edited, Some(result.source_fidelity())),
                TargetRequest::Inherit,
            )
            .expect("edited STEP document plan");
        assert!(matches!(
            plan.report().write_path(),
            cadmpeg_ir::report::export::WritePath::Synthesized { .. }
        ));
        assert_eq!(
            &wire::field::<cadmpeg_ir::report::export::FidelityResolution>(
                plan.report().write_path(),
                "fidelity"
            ),
            &cadmpeg_ir::report::export::FidelityResolution::NotConsumed {}
        );
        let mut edited_bytes = Vec::new();
        let export = plan
            .write_to(&mut edited_bytes)
            .expect("edited STEP document write");
        assert!(matches!(
            export.write_path(),
            cadmpeg_ir::report::export::WritePath::Synthesized { .. }
        ));
        let edited_result = EditableDecodeResult::from(
            codec
                .decode(&mut Cursor::new(edited_bytes), &DecodeOptions::default())
                .expect("edited STEP document decode"),
        );
        assert_valid(&edited_result);
        assert_eq!(
            edited_result.ir().model.bodies.len(),
            expected_model.bodies.len()
        );
        assert_eq!(
            edited_result.ir().model.faces.len(),
            expected_model.faces.len()
        );
        let expected_points = expected_model
            .points
            .iter()
            .map(|point| point.position().get())
            .collect::<Vec<_>>();
        let actual_points = edited_result
            .ir()
            .model
            .points
            .iter()
            .map(|point| point.position().get())
            .collect::<Vec<_>>();
        assert_eq!(actual_points.len(), expected_points.len());
        let mut matched = vec![false; actual_points.len()];
        for expected in expected_points {
            let actual = actual_points
                .iter()
                .enumerate()
                .find(|(index, actual)| {
                    !matched[*index] && actual.distance_squared(expected) <= 1.0e-18
                })
                .map(|(index, actual)| {
                    matched[index] = true;
                    *actual
                });
            assert!(
                actual.is_some(),
                "edited point was not preserved: {expected:?}"
            );
        }
    }
}
