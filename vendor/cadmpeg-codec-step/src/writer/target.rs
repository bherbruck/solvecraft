// SPDX-License-Identifier: Apache-2.0
//! STEP target resolution and export reporting.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::ResolvedWrite, ArenaCoverage, ArenaDisposition, ArenaDispositions, Consumption,
    EncodeInput, ExportBody, WritePath,
};

use crate::export::write_step_outcome;
use crate::loss::StepLossCode;
use crate::options::StepSchema;
use crate::StepCodec;

/// Why this writer cannot reproduce a source schema outside
/// [`StepSchema::TARGETS`].
///
/// STEP has no retained-image path, and every schema this writer emits stamps
/// object-identifier arcs, so an edition-unspecified or unrecognized
/// declaration cannot be written back.
const OFF_CATALOG_SOURCE_REASON: &str =
    "the semantic writer cannot synthesize it, and writing another schema would change what the \
     file declares; name a target to choose one";

/// Resolve the request against the source and synthesize the selected schema.
pub(crate) fn plan(
    codec: &StepCodec,
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    let Some(index) = resolved.index() else {
        return Err(resolved.unavailable(OFF_CATALOG_SOURCE_REASON));
    };
    let schema = StepSchema::from_target_index(index).ok_or_else(|| {
        resolved.unavailable("the resolved target index is outside the STEP schema catalog")
    })?;
    let mut bytes = Vec::new();
    let outcome = write_step_outcome(input.ir, &mut bytes, schema, &codec.options)?;
    let mut losses = outcome.losses;
    if let Some(message) = resolved.displacement_message() {
        losses.push(StepLossCode::SourceDialectDisplaced.note(message));
    }
    Ok(ExportBody {
        bytes,
        census: outcome.census,
        write_path: WritePath::Synthesized {
            // STEP has no retained image: a provided fidelity is never replayed.
            consumption: Consumption::NotConsumed,
        },
        coverage: ArenaCoverage::Declared(coverage(schema)),
        losses,
        notes: outcome.notes,
    })
}

/// What the STEP writer does with each model arena.
///
/// `Builder::note_unrepresented` in `export.rs` charges a specific loss for
/// every arena or record the graph does not carry, so no arena is omitted.
fn coverage(schema: StepSchema) -> ArenaDispositions {
    use ArenaDisposition::{Reported, Written};
    let ap242 = |supported: bool| if supported { Written } else { Reported };
    ArenaDispositions {
        bodies: Written,
        regions: Written,
        shells: Written,
        faces: Written,
        loops: Written,
        coedges: Written,
        edges: Written,
        vertices: Written,
        points: Written,
        surfaces: Written,
        curves: Written,
        subds: Reported,
        pcurves: Written,
        procedural_surfaces: Written,
        procedural_curves: Written,
        assets: Reported,
        features: Reported,
        feature_input_topologies: Reported,
        feature_result_topologies: Reported,
        configurations: Reported,
        parameters: Reported,
        sketches: Reported,
        sketch_entities: Reported,
        sketch_constraints: Reported,
        spatial_sketches: Reported,
        spatial_sketch_entities: Reported,
        spatial_sketch_constraints: Reported,
        spreadsheets: Reported,
        product_definitions: Written,
        occurrences: Written,
        assembly_joints: Reported,
        drawings: Reported,
        semantic_annotations: Reported,
        presentation_documents: Reported,
        view_presentations: Reported,
        tessellations: ap242(schema.supports_tessellation()),
        appearances: Written,
        appearance_bindings: Written,
        attributes: Reported,
        pmi: ap242(schema.supports_semantic_pmi()),
        presentation_layers: Written,
    }
}
