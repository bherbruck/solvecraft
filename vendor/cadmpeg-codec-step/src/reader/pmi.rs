// SPDX-License-Identifier: Apache-2.0
//! STEP semantic product-manufacturing information.

use crate::ids::kind;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::num::NonZeroU32;

use super::reference::{first_matching, references};
use super::{named_parameter, record_values, source_numeric_id, RecordExt, ValueExt};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PmiId;
use cadmpeg_ir::pmi::{
    DatumReference, DatumTargetForm, DimensionKind, DimensionTolerance, GeometricToleranceKind,
    LimitsAndFits, PmiDefinition, PmiDimension, PmiQuantity, PmiTarget, PmiValue,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;

use crate::ids;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::decode_text_charged;
use super::geometry::GeometryData;
use super::topology::TopologyData;
use super::StageOutcome;

mod annotations;

use annotations::{AnnotationDraft, AnnotationIndex, Annotations};

struct MeasureContext<'a> {
    length_scale: f64,
    angle_scale: f64,
    graph_limit: usize,
    losses: &'a mut Vec<LossNote>,
}

fn collect_pmi_references(
    values: &[Value],
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<Vec<u64>, CodecError> {
    let mut ids = Vec::new();
    for value in values {
        for id in references(value, ctx) {
            let id = id?;
            ctx.push_vec(&mut ids, id, operation)?;
        }
    }
    Ok(ids)
}

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryData,
    topology: &TopologyData,
    ir: &mut CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<()>, CodecError> {
    if !exchange.has_entity_matching(is_pmi_entity_name) {
        return Ok(StageOutcome {
            value: (),
            claims: HashSet::new(),
            losses: Vec::new(),
            notes: Vec::new(),
        });
    }
    let base_aspects = ctx.collect_btree_set(
        exchange
            .entities_any(&["SHAPE_ASPECT", "DATUM_FEATURE", "DATUM"])
            .map(|(id, _)| id),
        "step_pmi_base_aspects",
    )?;
    let shape_aspects = ctx.collect_btree_set(
        exchange.matching_entity_ids(is_shape_aspect_name),
        "step_pmi_shape_aspects",
    )?;
    let mut typed = HashSet::new();
    let mut losses = Vec::new();
    let mut annotations = Annotations::default();
    let hidden_presentation_annotations = hidden_presentation_annotation_ids(exchange, ctx)?;

    let mut presentation_semantics = BTreeMap::<u64, Vec<u64>>::new();
    let graph_limit = super::record_graph_limit(ctx);
    let characteristic_values =
        characteristic_values(exchange, geometry, &mut losses, graph_limit, ctx)?;
    for (id, record) in exchange.entities("DATUM") {
        let identification = named_parameter(record, "DATUM", 0)
            .map(|value| {
                decode_text_charged(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "datum identification",
                    StepLossCode::MetadataStringInvalid,
                    ctx,
                )
            })
            .transpose()?
            .flatten()
            .unwrap_or_else(|| format!("#{id}"));
        annotations.push(
            ctx,
            ir,
            id,
            AnnotationDraft {
                name: shape_aspect_parameter(record, 0)
                    .map(|value| {
                        decode_text_charged(
                            exchange,
                            value,
                            &mut losses,
                            id,
                            "datum name",
                            StepLossCode::MetadataStringInvalid,
                            ctx,
                        )
                    })
                    .transpose()?
                    .flatten(),
                targets: targets([Ok(id)], ctx)?,
                visible: None,
                definition: PmiDefinition::Datum { identification },
            },
        )?;
        ctx.insert_hash_set(&mut typed, id, "step_pmi_typed_claims")?;
    }

    for id in exchange.matching_entity_ids(is_datum_target_name) {
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let form = shape_aspect_parameter(record, 1)
            .map(|value| {
                decode_text_charged(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "datum target form",
                    StepLossCode::MetadataStringInvalid,
                    ctx,
                )
            })
            .transpose()?
            .flatten()
            .unwrap_or_default();
        let identification = datum_target_identification_parameter(record)
            .map(|value| {
                decode_text_charged(
                    exchange,
                    value,
                    &mut losses,
                    id,
                    "datum target identification",
                    StepLossCode::MetadataStringInvalid,
                    ctx,
                )
            })
            .transpose()?
            .flatten()
            .unwrap_or_else(|| format!("#{id}"));
        annotations.push(
            ctx,
            ir,
            id,
            AnnotationDraft {
                name: shape_aspect_parameter(record, 0)
                    .map(|value| {
                        decode_text_charged(
                            exchange,
                            value,
                            &mut losses,
                            id,
                            "datum target name",
                            StepLossCode::MetadataStringInvalid,
                            ctx,
                        )
                    })
                    .transpose()?
                    .flatten(),
                targets: targets([Ok(id)], ctx)?,
                visible: None,
                definition: PmiDefinition::DatumTarget {
                    form: datum_target_form(&form, ctx)?,
                    identification,
                    basis: Vec::new(),
                },
            },
        )?;
        ctx.insert_hash_set(&mut typed, id, "step_pmi_typed_claims")?;
    }

    for (id, record) in exchange.entities("DATUM_SYSTEM") {
        let constituents = record
            .parameters()
            .iter()
            .rev()
            .find_map(ValueExt::list)
            .unwrap_or_default();
        let mut datum_records = HashSet::new();
        let mut measurements = measure_context(geometry, id, &mut losses, graph_limit);
        let mut datum_references = Vec::new();
        for (index, constituent) in constituents.iter().enumerate() {
            let Some(precedence) = u32::try_from(index + 1).ok().and_then(NonZeroU32::new) else {
                continue;
            };
            for reference in datum_references_for_compartment(
                constituent,
                precedence,
                exchange,
                &annotations,
                &mut datum_records,
                &mut measurements,
                ctx,
            )? {
                ctx.push_vec(
                    &mut datum_references,
                    reference,
                    "step_pmi_datum_system_references",
                )?;
            }
        }
        admit_datum_reference_maps(&datum_references, ctx)?;
        let datum_references = match datum_references.try_into() {
            Ok(references) => references,
            Err(error) => {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::PmiDatumSystemInvalid
                        .note(format!("DATUM_SYSTEM #{id} omitted: {error}")),
                    "step_pmi_losses",
                )?;
                continue;
            }
        };
        annotations.push(
            ctx,
            ir,
            id,
            AnnotationDraft {
                name: shape_aspect_parameter(record, 0)
                    .map(|value| {
                        decode_text_charged(
                            exchange,
                            value,
                            &mut losses,
                            id,
                            "datum system name",
                            StepLossCode::MetadataStringInvalid,
                            ctx,
                        )
                    })
                    .transpose()?
                    .flatten(),
                targets: targets(
                    record
                        .parameters()
                        .iter()
                        .flat_map(|value| references(value, ctx))
                        .filter(|id| match id {
                            Ok(id) => base_aspects.contains(id),
                            Err(_) => true,
                        }),
                    ctx,
                )?,
                visible: None,
                definition: PmiDefinition::DatumSystem {
                    references: datum_references,
                },
            },
        )?;
        ctx.insert_hash_set(&mut typed, id, "step_pmi_typed_claims")?;
        ctx.extend_hash_set(&mut typed, datum_records, "step_pmi_typed_claims")?;
    }

    for id in exchange.matching_entity_ids(is_dimension_name) {
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let Some((dimension_name, mut kind)) = dimension_descriptor(record, ctx)? else {
            continue;
        };
        let mut name = None;
        for value in record
            .partials
            .iter()
            .flat_map(|partial| &partial.parameters)
        {
            if let Some(text) = decode_text_charged(
                exchange,
                value,
                &mut losses,
                id,
                "dimension name",
                StepLossCode::MetadataStringInvalid,
                ctx,
            )? {
                name = Some(text);
                break;
            }
        }
        if matches!(kind, DimensionKind::Size) {
            let category = if dimension_name.starts_with("DIMENSIONAL_SIZE_WITH_DATUM_FEATURE") {
                let mut category = None;
                for value in record
                    .partials
                    .iter()
                    .find(|partial| partial.name == dimension_name)
                    .into_iter()
                    .flat_map(|partial| partial.parameters.iter().rev())
                {
                    if let Some(text) = decode_text_charged(
                        exchange,
                        value,
                        &mut losses,
                        id,
                        "dimension category",
                        StepLossCode::MetadataStringInvalid,
                        ctx,
                    )? {
                        category = Some(text);
                        break;
                    }
                }
                category
            } else {
                None
            };
            let category = category.as_deref().or(name.as_deref());
            kind = if category.is_some_and(|value| value.eq_ignore_ascii_case("diameter")) {
                DimensionKind::Diameter
            } else if category.is_some_and(|value| value.eq_ignore_ascii_case("radius")) {
                DimensionKind::Radius
            } else {
                kind
            };
        }
        let nominal = characteristic_values.get(&id).copied();
        let definition = PmiDimension::new(kind, nominal, None)
            .map_err(|error| CodecError::malformed(format_args!("dimension #{id}: {error}")))?;
        let aspect_ids = record
            .partials
            .iter()
            .flat_map(|partial| &partial.parameters)
            .flat_map(|value| references(value, ctx))
            .filter(|reference| match reference {
                Ok(id) => shape_aspects.contains(id),
                Err(_) => true,
            });
        annotations.push(
            ctx,
            ir,
            id,
            AnnotationDraft {
                name,
                targets: targets(aspect_ids, ctx)?,
                visible: None,
                definition: PmiDefinition::Dimension(definition),
            },
        )?;
        ctx.insert_hash_set(&mut typed, id, "step_pmi_typed_claims")?;
    }

    for (id, record) in exchange.entities("PLUS_MINUS_TOLERANCE") {
        let refs =
            collect_pmi_references(record.parameters(), ctx, "step_pmi_plus_minus_references")?;
        let dimension = refs
            .iter()
            .find_map(|reference| annotations.get(*reference));
        let limits = refs.iter().find_map(|reference| {
            exchange
                .records()
                .get(reference)
                .filter(|candidate| candidate.simple_name() == Some("TOLERANCE_VALUE"))
        });
        let fit = refs
            .iter()
            .find_map(|reference| {
                let record = exchange.records().get(reference)?;
                (record.simple_name() == Some("LIMITS_AND_FITS")).then(
                    || -> Result<_, CodecError> {
                        Ok((
                            *reference,
                            LimitsAndFits {
                                form_variance: record
                                    .parameter(0)
                                    .map(|value| {
                                        decode_text_charged(
                                            exchange,
                                            value,
                                            &mut losses,
                                            *reference,
                                            "limits-and-fits form variance",
                                            StepLossCode::MetadataStringInvalid,
                                            ctx,
                                        )
                                    })
                                    .transpose()?
                                    .flatten()
                                    .unwrap_or_default(),
                                zone_variance: record
                                    .parameter(1)
                                    .map(|value| {
                                        decode_text_charged(
                                            exchange,
                                            value,
                                            &mut losses,
                                            *reference,
                                            "limits-and-fits zone variance",
                                            StepLossCode::MetadataStringInvalid,
                                            ctx,
                                        )
                                    })
                                    .transpose()?
                                    .flatten()
                                    .unwrap_or_default(),
                                grade: record
                                    .parameter(2)
                                    .map(|value| {
                                        decode_text_charged(
                                            exchange,
                                            value,
                                            &mut losses,
                                            *reference,
                                            "limits-and-fits grade",
                                            StepLossCode::MetadataStringInvalid,
                                            ctx,
                                        )
                                    })
                                    .transpose()?
                                    .flatten()
                                    .unwrap_or_default(),
                                source: record
                                    .parameter(3)
                                    .map(|value| {
                                        decode_text_charged(
                                            exchange,
                                            value,
                                            &mut losses,
                                            *reference,
                                            "limits-and-fits source",
                                            StepLossCode::MetadataStringInvalid,
                                            ctx,
                                        )
                                    })
                                    .transpose()?
                                    .flatten()
                                    .unwrap_or_default(),
                            },
                        ))
                    },
                )
            })
            .transpose()?;
        if let (Some(index), Some(limits)) = (dimension, limits) {
            let mut measurements = measure_context(geometry, id, &mut losses, graph_limit);
            let lower = limits
                .parameters()
                .first()
                .map(|value| measure(value, exchange, &mut measurements, ctx))
                .transpose()?
                .flatten();
            let upper = limits
                .parameters()
                .get(1)
                .map(|value| measure(value, exchange, &mut measurements, ctx))
                .transpose()?
                .flatten();
            if let (Some(lower), Some(upper)) = (lower, upper) {
                if set_dimension_tolerance(
                    &mut ir.model.pmi[index.get()].definition,
                    DimensionTolerance::PlusMinus { lower, upper },
                )
                .map_err(|error| {
                    CodecError::malformed(format_args!("PLUS_MINUS_TOLERANCE #{id}: {error}"))
                })? {
                    ctx.insert_hash_set(&mut typed, id, "step_pmi_typed_claims")?;
                    ctx.extend_hash_set(&mut typed, refs, "step_pmi_typed_claims")?;
                } else {
                    ctx.push_vec(
                        &mut losses,
                        StepLossCode::DecodeWarning.note(format!(
                        "PLUS_MINUS_TOLERANCE #{id} is an additional tolerance for one dimension"
                    )),
                        "step_pmi_losses",
                    )?;
                }
            } else {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "PLUS_MINUS_TOLERANCE #{id} does not contain both deviation values"
                    )),
                    "step_pmi_losses",
                )?;
            }
        } else if let (Some(index), Some((fit_id, fit))) = (dimension, fit) {
            if set_dimension_tolerance(
                &mut ir.model.pmi[index.get()].definition,
                DimensionTolerance::Fit { fit },
            )
            .map_err(|error| {
                CodecError::malformed(format_args!("PLUS_MINUS_TOLERANCE #{id}: {error}"))
            })? {
                ctx.extend_hash_set(&mut typed, [id, fit_id], "step_pmi_typed_claims")?;
            } else {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "PLUS_MINUS_TOLERANCE #{id} is an additional tolerance for one dimension"
                    )),
                    "step_pmi_losses",
                )?;
            }
        } else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "PLUS_MINUS_TOLERANCE #{id} has no resolvable dimension and limits"
                )),
                "step_pmi_losses",
            )?;
        }
    }

    for id in exchange.matching_entity_ids(|name| tolerance_kind(Some(name)).is_some()) {
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let Some(tolerance) = record
            .partials
            .iter()
            .find_map(|partial| {
                (partial.name != "GEOMETRIC_TOLERANCE")
                    .then(|| tolerance_kind(Some(&partial.name)))
                    .flatten()
            })
            .or_else(|| {
                record
                    .partials
                    .iter()
                    .find_map(|partial| tolerance_kind(Some(&partial.name)))
            })
        else {
            continue;
        };
        let reference_values = record
            .partials
            .iter()
            .find(|partial| partial.name == "GEOMETRIC_TOLERANCE")
            .map_or(record.parameters(), |partial| partial.parameters.as_slice());
        let refs = collect_pmi_references(
            reference_values,
            ctx,
            "step_pmi_geometric_tolerance_references",
        )?;
        let mut measurements = measure_context(geometry, id, &mut losses, graph_limit);
        let magnitude = first_measure(
            record
                .partials
                .iter()
                .find(|partial| partial.name == "GEOMETRIC_TOLERANCE")
                .into_iter()
                .flat_map(|partial| partial.parameters.iter()),
            exchange,
            &mut measurements,
            ctx,
        )?;
        let magnitude = match magnitude {
            Some(magnitude) => Some(magnitude),
            None => first_measure(
                record
                    .partials
                    .iter()
                    .filter(|partial| partial.name != "GEOMETRIC_TOLERANCE")
                    .flat_map(|partial| partial.parameters.iter()),
                exchange,
                &mut measurements,
                ctx,
            )?,
        };
        let Some(magnitude) = magnitude.and_then(cadmpeg_ir::pmi::PmiMagnitude::new) else {
            let display_name = ctx.join_display_retained(
                record.partials.iter().map(|partial| partial.name.as_str()),
                "+",
                "step_record_display_name",
            )?;
            let message = ctx.format_retained(
                format_args!("{display_name} #{id} has no numeric magnitude"),
                "step_pmi_invalid_tolerance_text",
            )?;
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(message),
                "step_pmi_losses",
            )?;
            continue;
        };
        let defined_unit = record
            .partials
            .iter()
            .find(|partial| partial.name == "GEOMETRIC_TOLERANCE_WITH_DEFINED_UNIT")
            .and_then(|partial| partial.parameters.first())
            .map(|value| measure(value, exchange, &mut measurements, ctx))
            .transpose()?
            .flatten();
        let (defined_area_unit, defined_area_second_unit) = if let Some(partial) = record
            .partials
            .iter()
            .find(|partial| partial.name == "GEOMETRIC_TOLERANCE_WITH_DEFINED_AREA_UNIT")
        {
            let area = partial
                .parameters
                .first()
                .and_then(ValueExt::enumeration)
                .map(|name| {
                    let mut name =
                        ctx.copy_retained_text(name, "step_pmi_defined_area_unit_text")?;
                    name.make_ascii_lowercase();
                    Ok::<_, CodecError>(name)
                })
                .transpose()?;
            let second = partial
                .parameters
                .get(1)
                .map(|value| measure(value, exchange, &mut measurements, ctx))
                .transpose()?
                .flatten();
            (area, second)
        } else {
            (None, None)
        };
        // A complex tolerance keeps its base targets in GEOMETRIC_TOLERANCE,
        // while GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE carries the datum
        // system as a separate aggregate.
        let datum_system = first_matching(
            record
                .partials
                .iter()
                .find(|partial| partial.name == "GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE")
                .into_iter()
                .flat_map(|partial| partial.parameters.iter()),
            ctx,
            |id| {
                annotations.get(id).is_some_and(|index| {
                    matches!(
                        ir.model.pmi[index.get()].definition,
                        PmiDefinition::DatumSystem { .. }
                    )
                })
            },
        )?
        .and_then(|id| {
            annotations
                .get(id)
                .map(|index| &ir.model.pmi[index.get()].id)
        })
        .map(|id| id.try_clone_for_decode(ctx, "step_pmi_datum_system_identity_copy"))
        .transpose()?;
        annotations.push(
            ctx,
            ir,
            id,
            AnnotationDraft {
                name: named_parameter(record, "GEOMETRIC_TOLERANCE", 0)
                    .or_else(|| record.parameter(0))
                    .map(|value| {
                        decode_text_charged(
                            exchange,
                            value,
                            &mut losses,
                            id,
                            "geometric tolerance name",
                            StepLossCode::MetadataStringInvalid,
                            ctx,
                        )
                    })
                    .transpose()?
                    .flatten(),
                targets: targets(
                    refs.iter()
                        .copied()
                        .filter(|id| base_aspects.contains(id))
                        .map(Ok),
                    ctx,
                )?,
                visible: None,
                definition: PmiDefinition::GeometricTolerance {
                    tolerance,
                    magnitude,
                    defined_unit,
                    defined_area_unit,
                    defined_area_second_unit,
                    datum_system,
                    modifiers: tolerance_modifiers(record, ctx)?,
                },
            },
        )?;
        ctx.insert_hash_set(&mut typed, id, "step_pmi_typed_claims")?;
        ctx.extend_hash_set(
            &mut typed,
            refs.iter().copied().filter(|reference| {
                exchange
                    .records()
                    .get(reference)
                    .is_some_and(is_measure_record)
            }),
            "step_pmi_typed_claims",
        )?;
        for value in record
            .partials
            .iter()
            .flat_map(|partial| partial.parameters.iter())
        {
            for reference in references(value, ctx) {
                let reference = reference?;
                if exchange
                    .records()
                    .get(&reference)
                    .is_some_and(is_measure_record)
                {
                    ctx.insert_hash_set(&mut typed, reference, "step_pmi_typed_claims")?;
                }
            }
        }
    }

    for (id, record) in exchange.entities("DRAUGHTING_MODEL_ITEM_ASSOCIATION") {
        let Some(definition) = named_parameter(record, "DRAUGHTING_MODEL_ITEM_ASSOCIATION", 2)
            .and_then(ValueExt::reference)
        else {
            continue;
        };
        if annotations.get(definition).is_some() {
            if let Some(items) = named_parameter(record, "DRAUGHTING_MODEL_ITEM_ASSOCIATION", 4) {
                for item in references(items, ctx) {
                    let item = item?;
                    ctx.push_btree_group(
                        &mut presentation_semantics,
                        item,
                        definition,
                        "step_pmi_presentation_semantic_groups",
                        "step_pmi_presentation_semantic_members",
                    )?;
                }
            }
            ctx.insert_hash_set(&mut typed, id, "step_pmi_typed_claims")?;
        }
    }

    for id in exchange.matching_entity_ids(is_presentation_annotation) {
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let Some(name) = presentation_annotation_name(record) else {
            continue;
        };
        let mut text_records = BTreeSet::new();
        let text = find_annotation_text(
            id,
            exchange,
            &mut BTreeSet::new(),
            &mut text_records,
            &mut losses,
            0,
            ctx,
        )?;
        // Placement identity is the carrier key; the transform value cannot
        // make two source carriers one semantic carrier.
        let mut placement_candidates = BTreeMap::new();
        let mut placement_visited = BTreeMap::new();
        for parameter in record_values(record) {
            for reference in references(parameter, ctx) {
                let reference = reference?;
                collect_placement_candidates(
                    reference,
                    exchange,
                    geometry,
                    &mut placement_visited,
                    &mut placement_candidates,
                    0,
                    ctx,
                )?;
            }
        }
        let placement = match placement_candidates.len() {
            0 => None,
            1 => placement_candidates.values().next().copied(),
            count => {
                ctx.push_vec(&mut losses, StepLossCode::PresentationAnnotationPlacementAmbiguous.note(
                    format!(
                        "presentation annotation #{id} has {count} reachable placement carriers with no unique placement"
                    ),
                ), "step_pmi_losses")?;
                None
            }
        };
        let mut semantics = Vec::new();
        for parameter in record_values(record) {
            for reference in references(parameter, ctx) {
                let reference = reference?;
                if annotations.get(reference).is_some() {
                    ctx.push_vec(
                        &mut semantics,
                        pmi_id(reference),
                        "step_pmi_presentation_semantics",
                    )?;
                }
            }
        }
        for semantic in presentation_semantics.get(&id).into_iter().flatten() {
            ctx.push_vec(
                &mut semantics,
                pmi_id(*semantic),
                "step_pmi_presentation_semantics",
            )?;
        }
        annotations.push(
            ctx,
            ir,
            id,
            AnnotationDraft {
                name: named_parameter(record, name, 0)
                    .or_else(|| named_parameter(record, "REPRESENTATION_ITEM", 0))
                    .or_else(|| record.parameter(0))
                    .map(|value| {
                        decode_text_charged(
                            exchange,
                            value,
                            &mut losses,
                            id,
                            "presentation annotation name",
                            StepLossCode::MetadataStringInvalid,
                            ctx,
                        )
                    })
                    .transpose()?
                    .flatten(),
                targets: Vec::new(),
                visible: hidden_presentation_annotations
                    .contains(&id)
                    .then_some(false),
                definition: PmiDefinition::Presentation {
                    text,
                    placement,
                    semantics,
                },
            },
        )?;
        ctx.insert_hash_set(&mut typed, id, "step_pmi_typed_claims")?;
        ctx.extend_hash_set(&mut typed, text_records, "step_pmi_typed_claims")?;
    }
    for (id, _) in
        exchange.entities_any(&["DRAUGHTING_MODEL", "ANNOTATION_PLANE", "DRAUGHTING_CALLOUT"])
    {
        ctx.insert_hash_set(&mut typed, id, "step_pmi_typed_claims")?;
    }

    resolve_feature_for_datum_target_relationships(exchange, &annotations, ir, &mut typed, ctx)?;
    let points_by_source = point_sources(ir, ctx)?;
    let curves_by_source = curve_sources(ir, ctx)?;
    let geometry_sources = GeometrySources {
        points: &points_by_source,
        curves: &curves_by_source,
    };
    resolve_geometric_item_usages(
        exchange,
        topology,
        geometry_sources,
        (&shape_aspects, &annotations),
        ir,
        &mut typed,
        ctx,
    )?;

    let targeted_aspects = ctx.collect_btree_set(
        ir.model
            .pmi
            .iter()
            .flat_map(|annotation| &annotation.targets)
            .filter_map(|target| match target {
                PmiTarget::ShapeAspect { source_id } => {
                    source_id.as_str().strip_prefix('#')?.parse().ok()
                }
                _ => None,
            }),
        "step_pmi_targeted_aspects",
    )?;
    ctx.extend_hash_set(
        &mut typed,
        shape_aspects.intersection(&targeted_aspects).copied(),
        "step_pmi_typed_claims",
    )?;
    mark_characteristic_representations(exchange, &annotations, &mut typed, ctx)?;
    Ok(StageOutcome {
        value: (),
        claims: typed,
        losses,
        notes: Vec::new(),
    })
}

fn set_dimension_tolerance(
    definition: &mut PmiDefinition,
    value: DimensionTolerance,
) -> Result<bool, String> {
    let PmiDefinition::Dimension(dimension) = definition else {
        return Ok(false);
    };
    dimension.merge_tolerance(value)
}

fn mark_characteristic_representations(
    exchange: &Exchange,
    annotations: &Annotations,
    typed: &mut HashSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (id, record) in exchange.entities("DIMENSIONAL_CHARACTERISTIC_REPRESENTATION") {
        let Some(_) = first_matching(record_values(record), ctx, |reference| {
            annotations.get(reference).is_some()
        })?
        else {
            continue;
        };
        ctx.insert_hash_set(typed, id, "step_pmi_typed_claims")?;
        for parameter in record_values(record) {
            for representation_id in references(parameter, ctx) {
                let representation_id = representation_id?;
                let Some(representation) = exchange.records().get(&representation_id) else {
                    continue;
                };
                if !representation
                    .partials
                    .iter()
                    .any(|partial| partial.name == "SHAPE_DIMENSION_REPRESENTATION")
                {
                    continue;
                }
                ctx.insert_hash_set(typed, representation_id, "step_pmi_typed_claims")?;
                for parameter in record_values(representation) {
                    for reference in references(parameter, ctx) {
                        let reference = reference?;
                        if exchange
                            .records()
                            .get(&reference)
                            .is_some_and(is_measure_record)
                        {
                            ctx.insert_hash_set(typed, reference, "step_pmi_typed_claims")?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn resolve_feature_for_datum_target_relationships(
    exchange: &Exchange,
    annotations: &Annotations,
    ir: &mut CadIr,
    typed: &mut HashSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (id, record) in exchange.entities("FEATURE_FOR_DATUM_TARGET_RELATIONSHIP") {
        let Some((relating, related)) = relationship_endpoints(record, ctx)? else {
            continue;
        };
        let Some(annotation_index) = annotations.get(related) else {
            continue;
        };
        let annotation = &mut ir.model.pmi[annotation_index.get()];
        let PmiDefinition::DatumTarget { basis, .. } = &mut annotation.definition else {
            continue;
        };
        push_target(
            basis,
            PmiTarget::ShapeAspect {
                source_id: super::step_source_id(relating),
            },
            ctx,
            "step_pmi_datum_basis_targets",
        )?;
        ctx.extend_hash_set(typed, [id, relating], "step_pmi_typed_claims")?;
    }
    Ok(())
}

fn resolve_geometric_item_usages(
    exchange: &Exchange,
    topology: &TopologyData,
    geometry_sources: GeometrySources<'_>,
    (shape_aspects, annotations): (&BTreeSet<u64>, &Annotations),
    ir: &mut CadIr,
    typed: &mut HashSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut aspect_annotations = BTreeMap::<u64, BTreeSet<AnnotationIndex>>::new();
    for (&annotation_id, record) in exchange.records() {
        let Some(annotation_index) = annotations.get(annotation_id) else {
            continue;
        };
        if shape_aspects.contains(&annotation_id) {
            ctx.insert_btree_group_set(
                &mut aspect_annotations,
                annotation_id,
                annotation_index,
                "step_pmi_aspect_annotation_groups",
                "step_pmi_aspect_annotation_members",
            )?;
        }
        for parameter in record_values(record) {
            for reference in references(parameter, ctx) {
                let reference = reference?;
                if shape_aspects.contains(&reference) {
                    ctx.insert_btree_group_set(
                        &mut aspect_annotations,
                        reference,
                        annotation_index,
                        "step_pmi_aspect_annotation_groups",
                        "step_pmi_aspect_annotation_members",
                    )?;
                }
            }
        }
    }

    let mut relationship_aspects = BTreeMap::<u64, BTreeSet<u64>>::new();
    for record in exchange.records().values() {
        let Some((relating, related)) = relationship_endpoints(record, ctx)? else {
            continue;
        };
        ctx.insert_btree_group_set(
            &mut relationship_aspects,
            relating,
            related,
            "step_pmi_relationship_aspect_groups",
            "step_pmi_relationship_aspect_members",
        )?;
        ctx.insert_btree_group_set(
            &mut relationship_aspects,
            related,
            relating,
            "step_pmi_relationship_aspect_groups",
            "step_pmi_relationship_aspect_members",
        )?;
    }

    for (&id, record) in exchange.records() {
        let Some(partial) = record
            .partials
            .iter()
            .find(|partial| partial.name == "GEOMETRIC_ITEM_SPECIFIC_USAGE")
        else {
            continue;
        };
        let Some(definition) = first_matching(partial.parameters.get(2), ctx, |_| true)? else {
            continue;
        };
        let Some(identified_item) = first_matching(partial.parameters.get(4), ctx, |_| true)?
        else {
            continue;
        };
        let mut annotation_indices = BTreeSet::new();
        for &index in aspect_annotations.get(&definition).into_iter().flatten() {
            ctx.insert_btree_set(
                &mut annotation_indices,
                index,
                "step_pmi_usage_annotation_indices",
            )?;
        }
        if let Some(aspects) = relationship_aspects.get(&definition) {
            for aspect in aspects {
                for &index in aspect_annotations.get(aspect).into_iter().flatten() {
                    ctx.insert_btree_set(
                        &mut annotation_indices,
                        index,
                        "step_pmi_usage_annotation_indices",
                    )?;
                }
            }
        }
        if annotation_indices.is_empty() {
            continue;
        }
        let targets = topology_targets(identified_item, topology, geometry_sources, ctx)?;
        if targets.is_empty() {
            continue;
        }
        for annotation_index in annotation_indices {
            let annotation = &mut ir.model.pmi[annotation_index.get()];
            for target in &targets {
                push_target(
                    &mut annotation.targets,
                    copy_pmi_target(target, ctx, "step_pmi_geometric_usage_identity")?,
                    ctx,
                    "step_pmi_geometric_usage_targets",
                )?;
            }
        }
        ctx.insert_hash_set(typed, id, "step_pmi_typed_claims")?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct GeometrySources<'a> {
    points: &'a BTreeMap<u64, Vec<cadmpeg_ir::ids::PointId>>,
    curves: &'a BTreeMap<u64, Vec<cadmpeg_ir::ids::CurveId>>,
}

fn topology_targets(
    id: u64,
    topology: &TopologyData,
    geometry_sources: GeometrySources<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<PmiTarget>, CodecError> {
    let mut targets = Vec::new();
    for body in topology.body_by_root.get(&id).into_iter().flatten() {
        let body = body.try_clone_for_decode(ctx, "step_pmi_topology_identity")?;
        push_target(
            &mut targets,
            PmiTarget::Body { body },
            ctx,
            "step_pmi_topology_targets",
        )?;
    }
    for face in topology.faces_by_source.get(&id).into_iter().flatten() {
        let face = face.try_clone_for_decode(ctx, "step_pmi_topology_identity")?;
        push_target(
            &mut targets,
            PmiTarget::Face { face },
            ctx,
            "step_pmi_topology_targets",
        )?;
    }
    for edge in topology.edges_by_source.get(&id).into_iter().flatten() {
        let edge = edge.try_clone_for_decode(ctx, "step_pmi_topology_identity")?;
        push_target(
            &mut targets,
            PmiTarget::Edge { edge },
            ctx,
            "step_pmi_topology_targets",
        )?;
    }
    for vertex in topology.vertices_by_source.get(&id).into_iter().flatten() {
        let vertex = vertex.try_clone_for_decode(ctx, "step_pmi_topology_identity")?;
        push_target(
            &mut targets,
            PmiTarget::Vertex { vertex },
            ctx,
            "step_pmi_topology_targets",
        )?;
    }
    for point in geometry_sources.points.get(&id).into_iter().flatten() {
        let point = point.try_clone_for_decode(ctx, "step_pmi_topology_identity")?;
        push_target(
            &mut targets,
            PmiTarget::Point { point },
            ctx,
            "step_pmi_topology_targets",
        )?;
    }
    for curve in geometry_sources.curves.get(&id).into_iter().flatten() {
        let curve = curve.try_clone_for_decode(ctx, "step_pmi_topology_identity")?;
        push_target(
            &mut targets,
            PmiTarget::Curve { curve },
            ctx,
            "step_pmi_topology_targets",
        )?;
    }
    Ok(targets)
}

fn push_target(
    targets: &mut Vec<PmiTarget>,
    target: PmiTarget,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    if !targets.contains(&target) {
        ctx.push_vec(targets, target, operation)?;
    }
    Ok(())
}

fn relationship_endpoints(
    record: &RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(u64, u64)>, CodecError> {
    let Some(parameters) = record.partials.iter().find_map(|partial| {
        matches!(
            partial.name.as_str(),
            "SHAPE_ASPECT_RELATIONSHIP" | "FEATURE_FOR_DATUM_TARGET_RELATIONSHIP"
        )
        .then_some(partial.parameters.as_slice())
    }) else {
        return Ok(None);
    };
    let Some(relating) = first_matching(parameters.get(2), ctx, |_| true)? else {
        return Ok(None);
    };
    let Some(related) = first_matching(parameters.get(3), ctx, |_| true)? else {
        return Ok(None);
    };
    Ok(Some((relating, related)))
}

fn point_sources(
    ir: &CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, Vec<cadmpeg_ir::ids::PointId>>, CodecError> {
    let mut points = BTreeMap::new();
    for point in &ir.model.points {
        let Some(source) = source_numeric_id(point.id.as_str(), "point") else {
            continue;
        };
        let id = point
            .id
            .try_clone_for_decode(ctx, "step_pmi_point_source_identity")?;
        ctx.push_btree_group(
            &mut points,
            source,
            id,
            "step_pmi_point_source_groups",
            "step_pmi_point_source_items",
        )?;
    }
    Ok(points)
}

fn curve_sources(
    ir: &CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, Vec<cadmpeg_ir::ids::CurveId>>, CodecError> {
    let mut curves = BTreeMap::new();
    for curve in &ir.model.curves {
        let Some(source) = source_numeric_id(curve.id.as_str(), "curve") else {
            continue;
        };
        let id = curve
            .id
            .try_clone_for_decode(ctx, "step_pmi_curve_source_identity")?;
        ctx.push_btree_group(
            &mut curves,
            source,
            id,
            "step_pmi_curve_source_groups",
            "step_pmi_curve_source_items",
        )?;
    }
    Ok(curves)
}

fn datum_references_for_compartment(
    value: &Value,
    precedence: NonZeroU32,
    exchange: &Exchange,
    annotations: &Annotations,
    typed: &mut HashSet<u64>,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<DatumReference>, CodecError> {
    let Some(compartment_id) = value.reference() else {
        return Ok(Vec::new());
    };
    let Some(compartment) = exchange.records().get(&compartment_id) else {
        return Ok(Vec::new());
    };
    if compartment.partial("DATUM_REFERENCE_COMPARTMENT").is_none()
        && compartment.partial("DATUM_REFERENCE_ELEMENT").is_none()
    {
        return Ok(Vec::new());
    }
    ctx.insert_hash_set(typed, compartment_id, "step_pmi_typed_claims")?;
    let mut compartment_modifiers = Vec::new();
    for modifier in datum_modifiers(compartment)
        .and_then(ValueExt::list)
        .into_iter()
        .flatten()
    {
        if let Some(text) = modifier_text(modifier, exchange, typed, measurements, ctx)? {
            ctx.push_vec(
                &mut compartment_modifiers,
                text,
                "step_pmi_datum_modifier_items",
            )?;
        }
    }
    let base = datum_base(compartment);
    let mut output = Vec::new();
    if is_common_datum_list(base) {
        let Some(Value::Typed(_, members)) = base else {
            return Ok(output);
        };
        let members = members.list().unwrap_or_default();
        let common_group = (members.iter().filter_map(ValueExt::reference).count() >= 2)
            .then_some(precedence.get());
        for element_id in members.iter().filter_map(ValueExt::reference) {
            let Some(element) = exchange.records().get(&element_id) else {
                continue;
            };
            if element.partial("DATUM_REFERENCE_ELEMENT").is_none() {
                continue;
            }
            let Some(datum) = datum_base(element).and_then(ValueExt::reference) else {
                continue;
            };
            if annotations.get(datum).is_none() {
                continue;
            }
            let mut modifiers = ctx.try_collect_vec(
                compartment_modifiers
                    .iter()
                    .map(|value| ctx.copy_retained_text(value, "step_pmi_datum_modifier_copy")),
                "step_pmi_datum_modifier_items",
            )?;
            for modifier in datum_modifiers(element)
                .and_then(ValueExt::list)
                .into_iter()
                .flatten()
            {
                if let Some(text) = modifier_text(modifier, exchange, typed, measurements, ctx)? {
                    ctx.push_vec(&mut modifiers, text, "step_pmi_datum_modifier_items")?;
                }
            }
            ctx.extend_hash_set(typed, [element_id, datum], "step_pmi_typed_claims")?;
            ctx.push_vec(
                &mut output,
                DatumReference {
                    datum: pmi_id(datum),
                    precedence,
                    common_group,
                    modifiers,
                },
                "step_pmi_datum_reference_items",
            )?;
        }
        return Ok(output);
    }
    if let Some(base) = base {
        visit_datum_ids(base, ctx, &mut |datum| {
            if annotations.get(datum).is_none() {
                return Ok(());
            }
            ctx.insert_hash_set(typed, datum, "step_pmi_typed_claims")?;
            ctx.push_vec(
                &mut output,
                DatumReference {
                    datum: pmi_id(datum),
                    precedence,
                    common_group: None,
                    modifiers: ctx.try_collect_vec(
                        compartment_modifiers.iter().map(|value| {
                            ctx.copy_retained_text(value, "step_pmi_datum_modifier_copy")
                        }),
                        "step_pmi_datum_modifier_items",
                    )?,
                },
                "step_pmi_datum_reference_items",
            )
        })?;
    }
    Ok(output)
}

fn admit_datum_reference_maps(
    references: &[DatumReference],
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (index, reference) in references.iter().enumerate() {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(index),
            "step_pmi_datum_map_scan",
        )?;
        let prior = &references[..index];
        if !prior
            .iter()
            .any(|other| other.precedence == reference.precedence)
        {
            ctx.charge_collection_items(1, "step_pmi_datum_compartments")?;
        }
        if let Some(group) = reference.common_group {
            if !prior.iter().any(|other| other.common_group == Some(group)) {
                ctx.charge_collection_items(1, "step_pmi_datum_common_groups")?;
            }
        }
    }
    Ok(())
}

fn datum_base(record: &RawRecord) -> Option<&Value> {
    record
        .partials
        .iter()
        .find(|partial| partial.name == "GENERAL_DATUM_REFERENCE")
        .and_then(|partial| partial.parameters.first())
        .or_else(|| record.parameter(4))
}

fn datum_modifiers(record: &RawRecord) -> Option<&Value> {
    record
        .partials
        .iter()
        .find(|partial| partial.name == "GENERAL_DATUM_REFERENCE")
        .and_then(|partial| partial.parameters.get(1))
        .or_else(|| record.parameter(5))
}

fn is_common_datum_list(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Typed(kind, _)) if kind == "COMMON_DATUM_LIST")
}

fn visit_datum_ids(
    value: &Value,
    ctx: &DecodeContext<'_>,
    visitor: &mut impl FnMut(u64) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let _nested = ctx.enter_nested("step_pmi_datum_id_walk")?;
    match value {
        Value::Reference(id) => visitor(*id)?,
        Value::List(values) => {
            for value in values {
                visit_datum_ids(value, ctx, visitor)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn modifier_text(
    value: &Value,
    exchange: &Exchange,
    typed: &mut HashSet<u64>,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let _nested = ctx.enter_nested("step_pmi_datum_modifier_walk")?;
    match value {
        Value::Enumeration(value) => {
            let mut text = ctx.copy_retained_text(value, "step_pmi_datum_modifier_text")?;
            text.make_ascii_lowercase();
            Ok(Some(text))
        }
        Value::Typed(_, value) => modifier_text(value, exchange, typed, measurements, ctx),
        Value::Reference(id) => {
            let Some(record) = exchange.records().get(id) else {
                return Ok(None);
            };
            let Some(parameters) = record
                .partials
                .iter()
                .find(|partial| partial.name == "DATUM_REFERENCE_MODIFIER_WITH_VALUE")
            else {
                return Ok(None);
            };
            let parameters = parameters.parameters.as_slice();
            ctx.insert_hash_set(typed, *id, "step_pmi_typed_claims")?;
            let Some(kind) = parameters.first().and_then(ValueExt::enumeration) else {
                return Ok(None);
            };
            let Some(measure_id) = parameters.get(1).and_then(ValueExt::reference) else {
                return Ok(None);
            };
            let Some(value) = measure(&Value::Reference(measure_id), exchange, measurements, ctx)?
            else {
                return Ok(None);
            };
            let value = value.value.get();
            ctx.insert_hash_set(typed, measure_id, "step_pmi_typed_claims")?;
            let mut text = ctx.format_retained(
                format_args!("{kind}:{value}"),
                "step_pmi_datum_modifier_value_text",
            )?;
            text.make_ascii_lowercase();
            Ok(Some(text))
        }
        _ => Ok(None),
    }
}

pub(super) fn is_presentation_annotation(name: &str) -> bool {
    name.starts_with("ANNOTATION_")
        && (name.ends_with("_OCCURRENCE") || name.ends_with("_OCCURRENCE_WITH_LEADER_LINE"))
        || matches!(
            name,
            "TESSELLATED_ANNOTATION_OCCURRENCE"
                | "LEADER_CURVE"
                | "LEADER_DIRECTED_CALLOUT"
                | "LEADER_DIRECTED_DIMENSION"
        )
}

fn presentation_annotation_name(record: &RawRecord) -> Option<&str> {
    record.partials.iter().find_map(|partial| {
        is_presentation_annotation(&partial.name).then_some(partial.name.as_str())
    })
}

pub(super) fn is_supported_invisibility_target(record: &RawRecord) -> bool {
    presentation_annotation_name(record).is_some()
}

fn hidden_presentation_annotation_ids(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u64>, CodecError> {
    let mut hidden = BTreeSet::new();
    for record in exchange.records().values() {
        let Some(items) = record
            .partials
            .iter()
            .find(|partial| partial.name == "INVISIBILITY")
            .and_then(|partial| partial.parameters.first())
        else {
            continue;
        };
        for target in references(items, ctx) {
            let target = target?;
            if exchange
                .records()
                .get(&target)
                .is_some_and(is_supported_invisibility_target)
            {
                ctx.insert_btree_set(&mut hidden, target, "step_pmi_hidden_annotation_ids")?;
            }
        }
    }
    Ok(hidden)
}

fn collect_typed_placement_candidates(
    record: &RawRecord,
    geometry: &GeometryData,
    candidates: &mut BTreeMap<u64, Transform>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let has_annotation_text = record.partials.iter().any(|partial| {
        partial.name == "ANNOTATION_TEXT"
            || partial.name == "ANNOTATION_TEXT_CHARACTER"
            || partial.name.starts_with("ANNOTATION_TEXT_WITH_")
    });
    for partial in &record.partials {
        let is_carrier = match partial.name.as_str() {
            "DEFINED_CHARACTER_GLYPH"
            | "SYMBOL_TARGET"
            | "TEXT_LITERAL"
            | "DRAUGHTING_TEXT_LITERAL_WITH_DELINEATION" => true,
            name if name.starts_with("TEXT_LITERAL_WITH_") => true,
            "MAPPED_ITEM" | "ANNOTATION_TEXT" | "ANNOTATION_TEXT_CHARACTER" => has_annotation_text,
            name if has_annotation_text && name.starts_with("ANNOTATION_TEXT_WITH_") => true,
            _ => false,
        };
        if !is_carrier {
            continue;
        }
        for reference in partial
            .parameters
            .iter()
            .flat_map(|value| references(value, ctx))
        {
            let reference = reference?;
            if let Some(&(origin, z_axis, x_axis)) = geometry.placements.get(&reference) {
                if let Some(transform) =
                    super::geometry::placement_transform((origin, z_axis, x_axis))
                {
                    ctx.insert_btree_map(
                        candidates,
                        reference,
                        transform,
                        "step_pmi_placement_candidates",
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn find_annotation_text(
    id: u64,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
    used: &mut BTreeSet<u64>,
    losses: &mut Vec<LossNote>,
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let mut candidates = BTreeMap::new();
    collect_annotation_text(id, exchange, visited, &mut candidates, losses, depth, ctx)?;
    let Some((text_id, text)) = candidates.pop_first() else {
        return Ok(None);
    };
    if candidates.is_empty() {
        ctx.insert_btree_set(used, text_id, "step_pmi_annotation_text_used")?;
        Ok(Some(text))
    } else {
        let count = candidates.len() + 1;
        ctx.push_vec(losses, StepLossCode::PresentationAnnotationTextUnordered.note(format!(
                    "presentation annotation #{id} has {count} reachable text carriers with no ordered composition"
                )), "step_pmi_losses")?;
        Ok(None)
    }
}

fn collect_annotation_text(
    id: u64,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
    candidates: &mut BTreeMap<u64, String>,
    losses: &mut Vec<LossNote>,
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if depth >= 256 || visited.contains(&id) {
        return Ok(());
    }
    let _depth_guard = ctx.enter_nested("step_pmi_annotation_text_walk")?;
    ctx.insert_btree_set(visited, id, "step_pmi_annotation_text_visited")?;
    let Some(record) = exchange.records().get(&id) else {
        return Ok(());
    };
    if let Some(value) = named_parameter(record, "TEXT_LITERAL", 0)
        .or_else(|| named_parameter(record, "TEXT_LITERAL_WITH_ASSOCIATED_CURVES", 0))
    {
        if let Some(text) = decode_text_charged(
            exchange,
            value,
            losses,
            id,
            "PMI annotation text",
            StepLossCode::MetadataStringInvalid,
            ctx,
        )? {
            ctx.insert_btree_map(candidates, id, text, "step_pmi_annotation_text_candidates")?;
        }
    }
    for reference in record_values(record).flat_map(|value| references(value, ctx)) {
        let reference = reference?;
        collect_annotation_text(
            reference,
            exchange,
            visited,
            candidates,
            losses,
            depth + 1,
            ctx,
        )?;
    }
    Ok(())
}

fn collect_placement_candidates(
    id: u64,
    exchange: &Exchange,
    geometry: &GeometryData,
    visited: &mut BTreeMap<u64, usize>,
    candidates: &mut BTreeMap<u64, Transform>,
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    // Retain the shortest depth so the bounded traversal is independent of
    // aggregate member order when a graph has alternate paths.
    if depth >= 256
        || visited
            .get(&id)
            .is_some_and(|visited_depth| *visited_depth <= depth)
    {
        return Ok(());
    }
    let _nested = ctx.enter_nested("step_pmi_placement_walk")?;
    ctx.insert_btree_map(visited, id, depth, "step_pmi_placement_visited")?;
    if let Some(record) = exchange.records().get(&id) {
        collect_typed_placement_candidates(record, geometry, candidates, ctx)?;
    }
    let Some(record) = exchange.records().get(&id) else {
        return Ok(());
    };
    for reference in record_values(record).flat_map(|value| references(value, ctx)) {
        let reference = reference?;
        collect_placement_candidates(
            reference,
            exchange,
            geometry,
            visited,
            candidates,
            depth + 1,
            ctx,
        )?;
    }
    Ok(())
}

fn targets(
    ids: impl IntoIterator<Item = Result<u64, CodecError>>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<PmiTarget>, CodecError> {
    let mut seen = BTreeSet::new();
    let mut targets = Vec::new();
    for id in ids {
        let id = id?;
        if seen.contains(&id) {
            continue;
        }
        ctx.insert_btree_set(&mut seen, id, "step_pmi_target_ids")?;

        ctx.reserve_vec(&mut targets, 1, "step_pmi_target_items")?;
        targets.push(PmiTarget::ShapeAspect {
            source_id: super::step_source_id(id),
        });
    }
    Ok(targets)
}

fn pmi_id(id: u64) -> PmiId {
    PmiId::from(ids::presentation(kind!("pmi"), id))
}

fn copy_pmi_target(
    target: &PmiTarget,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<PmiTarget, CodecError> {
    Ok(match target {
        PmiTarget::Body { body } => PmiTarget::Body {
            body: body.try_clone_for_decode(ctx, operation)?,
        },
        PmiTarget::Face { face } => PmiTarget::Face {
            face: face.try_clone_for_decode(ctx, operation)?,
        },
        PmiTarget::Edge { edge } => PmiTarget::Edge {
            edge: edge.try_clone_for_decode(ctx, operation)?,
        },
        PmiTarget::Vertex { vertex } => PmiTarget::Vertex {
            vertex: vertex.try_clone_for_decode(ctx, operation)?,
        },
        PmiTarget::Point { point } => PmiTarget::Point {
            point: point.try_clone_for_decode(ctx, operation)?,
        },
        PmiTarget::Curve { curve } => PmiTarget::Curve {
            curve: curve.try_clone_for_decode(ctx, operation)?,
        },
        PmiTarget::Product { product } => PmiTarget::Product {
            product: product.try_clone_for_decode(ctx, operation)?,
        },
        PmiTarget::Occurrence { occurrence } => PmiTarget::Occurrence {
            occurrence: occurrence.try_clone_for_decode(ctx, operation)?,
        },
        PmiTarget::ShapeAspect { source_id } => {
            let copy = ctx.copy_retained_text(source_id.as_str(), operation)?;
            let source_id = cadmpeg_core::text::NonBlankString::new(copy)
                .ok_or_else(|| CodecError::malformed("STEP PMI target has a blank source ID"))?;
            PmiTarget::ShapeAspect { source_id }
        }
    })
}

fn datum_target_form(value: &str, ctx: &DecodeContext<'_>) -> Result<DatumTargetForm, CodecError> {
    let form = value.trim();
    if form.eq_ignore_ascii_case("point") {
        Ok(DatumTargetForm::Point)
    } else if form.eq_ignore_ascii_case("line") {
        Ok(DatumTargetForm::Line)
    } else if form.eq_ignore_ascii_case("rectangle") {
        Ok(DatumTargetForm::Rectangle)
    } else if form.eq_ignore_ascii_case("circle") {
        Ok(DatumTargetForm::Circle)
    } else if form.eq_ignore_ascii_case("circular curve") {
        Ok(DatumTargetForm::CircularCurve)
    } else {
        Ok(DatumTargetForm::Other(ctx.copy_retained_text(
            value,
            "step_pmi_datum_target_form_copy",
        )?))
    }
}

fn datum_target_identification_parameter(record: &RawRecord) -> Option<&Value> {
    record
        .partials
        .iter()
        .find(|partial| partial.name == "DATUM_TARGET")
        .and_then(|partial| partial.parameters.last())
        .or_else(|| record.parameter(4))
}

fn is_datum_target_name(name: &str) -> bool {
    matches!(name, "DATUM_TARGET" | "PLACED_DATUM_TARGET_FEATURE")
}

fn is_pmi_entity_name(name: &str) -> bool {
    matches!(
        name,
        "SHAPE_ASPECT"
            | "DATUM_FEATURE"
            | "DATUM"
            | "DATUM_SYSTEM"
            | "PLUS_MINUS_TOLERANCE"
            | "DRAUGHTING_MODEL_ITEM_ASSOCIATION"
            | "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION"
            | "DRAUGHTING_MODEL"
            | "ANNOTATION_PLANE"
            | "DRAUGHTING_CALLOUT"
            | "FEATURE_FOR_DATUM_TARGET_RELATIONSHIP"
            | "GEOMETRIC_ITEM_SPECIFIC_USAGE"
    ) || is_dimension_name(name)
        || tolerance_kind(Some(name)).is_some()
        || is_datum_target_name(name)
        || is_presentation_annotation(name)
}

// Simple STEP instances contain only their most-derived entity name. The
// parser does not load an EXPRESS inheritance graph, so keep the supported
// shape_aspect lineage here for leaf names that do not identify that lineage
// by the `_SHAPE_ASPECT` suffix.
const SHAPE_ASPECT_SUBTYPE_NAMES: &[&str] = &[
    "APEX",
    "APPLIED_AREA",
    "ASSEMBLY_BOND_DEFINITION",
    "ASSEMBLY_JOINT",
    "ASSEMBLY_SHAPE_CONSTRAINT",
    "ASSEMBLY_SHAPE_JOINT",
    "BASIC_ROUND_HOLE_OCCURRENCE",
    "BASIC_ROUND_HOLE_OCCURRENCE_IN_ASSEMBLY",
    "BEAD_END",
    "BOSS_TOP",
    "CENTRE_OF_SYMMETRY",
    "CHAMFER",
    "CHAMFER_OFFSET",
    "CIRCULAR_CLOSED_PROFILE",
    "CLOSED_PATH_PROFILE",
    "COMMON_DATUM",
    "COMPONENT_FEATURE",
    "COMPONENT_FEATURE_JOINT",
    "COMPONENT_MATING_CONSTRAINT_CONDITION",
    "COMPONENT_TERMINAL",
    "CONNECTION_ZONE_BASED_ASSEMBLY_JOINT",
    "CONNECTION_ZONE_INTERFACE_PLANE_RELATIONSHIP",
    "CONNECTIVITY_DEFINITION",
    "CONTACTING_FEATURE",
    "CONTACT_FEATURE",
    "COUNTERBORE_HOLE_OCCURRENCE",
    "COUNTERBORE_HOLE_OCCURRENCE_IN_ASSEMBLY",
    "COUNTERDRILL_HOLE_OCCURRENCE",
    "COUNTERDRILL_HOLE_OCCURRENCE_IN_ASSEMBLY",
    "COUNTERSINK_HOLE_OCCURRENCE",
    "COUNTERSINK_HOLE_OCCURRENCE_IN_ASSEMBLY",
    "CROSS_SECTIONAL_ALTERNATIVE_SHAPE_ELEMENT",
    "CROSS_SECTIONAL_GROUP_SHAPE_ELEMENT",
    "CROSS_SECTIONAL_GROUP_SHAPE_ELEMENT_WITH_LACING",
    "CROSS_SECTIONAL_GROUP_SHAPE_ELEMENT_WITH_TUBULAR_COVER",
    "CROSS_SECTIONAL_OCCURRENCE_SHAPE_ELEMENT",
    "CROSS_SECTIONAL_PART_SHAPE_ELEMENT",
    "DATUM",
    "DATUM_FEATURE",
    "DATUM_REFERENCE_COMPARTMENT",
    "DATUM_REFERENCE_ELEMENT",
    "DATUM_SYSTEM",
    "DATUM_SYSTEM_FOR_COMPOSITE_GROUP_ELEMENT",
    "DATUM_TARGET",
    "DEFAULT_MODEL_GEOMETRIC_VIEW",
    "DIMENSIONAL_LOCATION_WITH_DATUM_FEATURE",
    "DIMENSIONAL_SIZE_WITH_DATUM_FEATURE",
    "DIRECTED_ANGLE",
    "DIRECTED_TOLERANCE_ZONE",
    "DIRECTION_FEATURE_TOLERANCE_ZONE",
    "EDGE_ROUND",
    "EXTENSION",
    "FILLET",
    "GENERAL_DATUM_REFERENCE",
    "GEOMETRIC_ALIGNMENT",
    "GEOMETRIC_CONTACT",
    "GEOMETRIC_INTERSECTION",
    "HARNESS_NODE",
    "HARNESS_SEGMENT",
    "HOLE_BOTTOM",
    "INSTANCED_FEATURE",
    "JOGGLE_TERMINATION",
    "LINEAR_PROFILE",
    "MATED_PART_RELATIONSHIP",
    "MODIFIED_PATTERN",
    "NGON_CLOSED_PROFILE",
    "OPEN_PATH_PROFILE",
    "ORIENTED_TOLERANCE_ZONE",
    "PARALLEL_OFFSET",
    "PARTIAL_CIRCULAR_PROFILE",
    "PATH_FEATURE_COMPONENT",
    "PERPENDICULAR_TO",
    "PHYSICAL_COMPONENT_FEATURE",
    "PHYSICAL_COMPONENT_INTERFACE_TERMINAL",
    "PHYSICAL_COMPONENT_TERMINAL",
    "PLACED_DATUM_TARGET_FEATURE",
    "PLACED_FEATURE",
    "POCKET_BOTTOM",
    "PROFILE_FLOOR",
    "RECTANGULAR_CLOSED_PROFILE",
    "RIB_TOP_FLOOR",
    "ROUNDED_U_PROFILE",
    "SHAPE_ASPECT_OCCURRENCE",
    "SLOT_END",
    "SPOTFACE_OCCURRENCE",
    "SPOTFACE_OCCURRENCE_IN_ASSEMBLY",
    "SQUARE_U_PROFILE",
    "TANGENT",
    "TAPER",
    "TEE_PROFILE",
    "TERMINAL_FEATURE",
    "TERMINAL_LOCATION_GROUP",
    "THREAD_RUNOUT",
    "TOLERANCE_ZONE",
    "TOLERANCE_ZONE_WITH_DATUM",
    "TRANSITION_FEATURE",
    "TRANSPORT_FEATURE",
    "TWISTED_CROSS_SECTIONAL_GROUP_SHAPE_ELEMENT",
    "VEE_PROFILE",
];

fn is_shape_aspect_name(name: &str) -> bool {
    name == "SHAPE_ASPECT"
        || name.ends_with("_SHAPE_ASPECT")
        || SHAPE_ASPECT_SUBTYPE_NAMES.contains(&name)
}

fn shape_aspect_parameter(record: &RawRecord, index: usize) -> Option<&Value> {
    if let Some(partial) = record
        .partials
        .iter()
        .find(|partial| partial.name == "SHAPE_ASPECT")
    {
        partial.parameters.get(index)
    } else {
        record.parameter(index)
    }
}

fn is_measure_record(record: &RawRecord) -> bool {
    record.partials.iter().any(|partial| {
        partial.name == "MEASURE_REPRESENTATION_ITEM"
            || partial.name == "MEASURE_WITH_UNIT"
            || partial.name.ends_with("_MEASURE_WITH_UNIT")
    })
}

fn is_dimension_name(name: &str) -> bool {
    name == "DIMENSIONAL_SIZE"
        || name.starts_with("DIMENSIONAL_SIZE_")
        || name == "DIMENSIONAL_LOCATION"
        || name.starts_with("DIMENSIONAL_LOCATION_")
        || name == "ANGULAR_SIZE"
        || name.starts_with("ANGULAR_SIZE_")
        || name == "ANGULAR_LOCATION"
        || name.starts_with("ANGULAR_LOCATION_")
        || matches!(name, "DIAMETER_SIZE" | "RADIUS_SIZE")
        || name.ends_with("_SIZE")
        || name.ends_with("_LOCATION")
}

fn dimension_kind(
    name: &str,
    ctx: &DecodeContext<'_>,
) -> Result<Option<DimensionKind>, CodecError> {
    Ok(match name {
        name if name == "DIMENSIONAL_SIZE" || name.starts_with("DIMENSIONAL_SIZE_") => {
            Some(DimensionKind::Size)
        }
        name if name == "DIMENSIONAL_LOCATION" || name.starts_with("DIMENSIONAL_LOCATION_") => {
            Some(DimensionKind::Location)
        }
        name if name == "ANGULAR_SIZE"
            || name.starts_with("ANGULAR_SIZE_")
            || name == "ANGULAR_LOCATION"
            || name.starts_with("ANGULAR_LOCATION_") =>
        {
            Some(DimensionKind::Angular)
        }
        "DIAMETER_SIZE" => Some(DimensionKind::Diameter),
        "RADIUS_SIZE" => Some(DimensionKind::Radius),
        name if name.ends_with("_SIZE") || name.ends_with("_LOCATION") => {
            let mut name = ctx.copy_retained_text(name, "step_pmi_other_dimension_name")?;
            name.make_ascii_lowercase();
            Some(DimensionKind::Other(name))
        }
        _ => None,
    })
}

fn dimension_descriptor<'a>(
    record: &'a RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(&'a str, DimensionKind)>, CodecError> {
    for partial in &record.partials {
        if let Some(kind) = dimension_kind(partial.name.as_str(), ctx)? {
            return Ok(Some((partial.name.as_str(), kind)));
        }
    }
    Ok(None)
}

fn tolerance_kind(name: Option<&str>) -> Option<GeometricToleranceKind> {
    use GeometricToleranceKind as Kind;
    Some(match name? {
        "STRAIGHTNESS_TOLERANCE" => Kind::Straightness,
        "FLATNESS_TOLERANCE" => Kind::Flatness,
        "ROUNDNESS_TOLERANCE" => Kind::Roundness,
        "CYLINDRICITY_TOLERANCE" => Kind::Cylindricity,
        "COAXIALITY_TOLERANCE" => Kind::Coaxiality,
        "LINE_PROFILE_TOLERANCE" => Kind::LineProfile,
        "SURFACE_PROFILE_TOLERANCE" => Kind::SurfaceProfile,
        "ANGULARITY_TOLERANCE" => Kind::Angularity,
        "PERPENDICULARITY_TOLERANCE" => Kind::Perpendicularity,
        "PARALLELISM_TOLERANCE" => Kind::Parallelism,
        "POSITION_TOLERANCE" => Kind::Position,
        "CONCENTRICITY_TOLERANCE" => Kind::Concentricity,
        "SYMMETRY_TOLERANCE" => Kind::Symmetry,
        "CIRCULAR_RUNOUT_TOLERANCE" => Kind::CircularRunout,
        "TOTAL_RUNOUT_TOLERANCE" => Kind::TotalRunout,
        _ => return None,
    })
}

fn tolerance_modifiers(
    record: &RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<String>, CodecError> {
    let mut modifiers = Vec::new();
    if let Some(partial) = record
        .partials
        .iter()
        .find(|partial| partial.name == "GEOMETRIC_TOLERANCE_WITH_MODIFIERS")
    {
        for value in &partial.parameters {
            modifier_values(value, &mut modifiers, ctx)?;
        }
    }
    Ok(modifiers)
}

fn modifier_values(
    value: &Value,
    output: &mut Vec<String>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let _nested = ctx.enter_nested("step_pmi_modifier_walk")?;
    match value {
        Value::Enumeration(value) => {
            let mut text = ctx.copy_retained_text(value, "step_pmi_modifier_text")?;
            text.make_ascii_lowercase();
            ctx.push_vec(output, text, "step_pmi_modifier_items")?;
        }
        Value::List(values) => {
            for value in values {
                modifier_values(value, output, ctx)?;
            }
        }
        Value::Typed(_, value) => modifier_values(value, output, ctx)?,
        _ => {}
    }
    Ok(())
}

fn characteristic_values(
    exchange: &Exchange,
    geometry: &GeometryData,
    losses: &mut Vec<LossNote>,
    graph_limit: usize,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<u64, PmiValue>, CodecError> {
    let mut result = BTreeMap::<u64, PmiValue>::new();
    for (id, record) in exchange.entities("DIMENSIONAL_CHARACTERISTIC_REPRESENTATION") {
        let mut measurements = measure_context(geometry, id, losses, graph_limit);
        let Some(characteristic) = first_matching(record_values(record), ctx, |id| {
            exchange.records().get(&id).is_some_and(|record| {
                record
                    .partials
                    .iter()
                    .any(|partial| is_dimension_name(&partial.name))
            })
        })?
        else {
            continue;
        };
        let representation = first_matching(record_values(record), ctx, |id| {
            exchange.records().get(&id).is_some_and(|record| {
                record
                    .partials
                    .iter()
                    .any(|partial| partial.name == "SHAPE_DIMENSION_REPRESENTATION")
            })
        })?;
        let representation_items = representation
            .and_then(|id| exchange.records().get(&id))
            .and_then(|record| {
                record
                    .partials
                    .iter()
                    .find(|partial| partial.name == "SHAPE_DIMENSION_REPRESENTATION")
                    .and_then(|partial| partial.parameters.get(1))
                    .and_then(ValueExt::list)
            });
        let parameters = representation_items
            .map_or(MeasureParameters::Record(record), MeasureParameters::Items);
        let values = characteristic_measure_values(&parameters, exchange, &mut measurements, ctx)?;
        let mut named_count = 0usize;
        let mut named_first = None;
        for (name, value) in &values {
            if name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case("nominal value"))
            {
                named_count += 1;
                if named_count == 1 {
                    named_first = Some(*value);
                }
            }
        }
        let selected = if named_count == 1 {
            named_first
        } else if named_count > 1 {
            ctx.push_vec(losses, StepLossCode::DimensionalNominalAmbiguous.note(format!(
                "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION #{id} has {named_count} nominal value measures; the nominal is ambiguous"
                )), "step_pmi_losses")?;
            None
        } else if values.len() == 1 {
            values.first().map(|(_, value)| *value)
        } else {
            if values.len() > 1 {
                ctx.push_vec(losses, StepLossCode::DimensionalUnnamedMeasureAmbiguous.note(format!(
                        "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION #{id} has {} unnamed measure values; the nominal is ambiguous",
                        values.len()
                    )), "step_pmi_losses")?;
            }
            None
        };
        if let Some(selected) = selected {
            ctx.insert_btree_map(
                &mut result,
                characteristic,
                selected,
                "step_pmi_characteristic_values",
            )?;
        }
    }
    Ok(result)
}

enum MeasureParameters<'a> {
    Items(&'a [Value]),
    Record(&'a RawRecord),
}

impl MeasureParameters<'_> {
    fn visit(
        &self,
        mut visitor: impl FnMut(&Value) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        match self {
            Self::Items(items) => {
                for value in *items {
                    visitor(value)?;
                }
            }
            Self::Record(record) => {
                for value in record_values(record) {
                    visitor(value)?;
                }
            }
        }
        Ok(())
    }
}

fn characteristic_measure_values(
    parameters: &MeasureParameters<'_>,
    exchange: &Exchange,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<(Option<String>, PmiValue)>, CodecError> {
    let mut measure_ids = BTreeSet::new();
    parameters.visit(|parameter| {
        collect_measure_ids(
            parameter,
            exchange,
            &mut BTreeSet::new(),
            0,
            measurements.graph_limit,
            &mut measure_ids,
            ctx,
        )
    })?;
    let mut values = Vec::new();
    for id in measure_ids {
        if let Some(value) = measure(&Value::Reference(id), exchange, measurements, ctx)? {
            let name = exchange
                .records()
                .get(&id)
                .map(|record| measure_item_name(id, record, exchange, measurements.losses, ctx))
                .transpose()?
                .flatten();
            ctx.reserve_vec(&mut values, 1, "step_pmi_measure_values")?;
            values.push((name, value));
        }
    }
    if values.is_empty() {
        parameters.visit(|parameter| {
            if let Some(value) = measure(parameter, exchange, measurements, ctx)? {
                ctx.reserve_vec(&mut values, 1, "step_pmi_measure_values")?;
                values.push((None, value));
            }
            Ok(())
        })?;
    }
    Ok(values)
}

fn collect_measure_ids(
    value: &Value,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    depth: usize,
    graph_limit: usize,
    measure_ids: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if depth >= graph_limit {
        return Ok(());
    }
    let _nested = ctx.enter_nested("step_pmi_measure_id_walk")?;
    match value {
        Value::Reference(id) => {
            if active.contains(id) {
                return Ok(());
            }
            ctx.insert_btree_set(active, *id, "step_pmi_measure_active_ids")?;
            if let Some(record) = exchange.records().get(id) {
                if is_measure_record(record) {
                    ctx.insert_btree_set(measure_ids, *id, "step_pmi_measure_ids")?;
                } else {
                    for partial in &record.partials {
                        for parameter in &partial.parameters {
                            collect_measure_ids(
                                parameter,
                                exchange,
                                active,
                                depth + 1,
                                graph_limit,
                                measure_ids,
                                ctx,
                            )?;
                        }
                    }
                }
            }
            active.remove(id);
        }
        Value::List(values) => {
            for value in values {
                collect_measure_ids(
                    value,
                    exchange,
                    active,
                    depth + 1,
                    graph_limit,
                    measure_ids,
                    ctx,
                )?;
            }
        }
        Value::Typed(_, value) => {
            collect_measure_ids(
                value,
                exchange,
                active,
                depth + 1,
                graph_limit,
                measure_ids,
                ctx,
            )?;
        }
        _ => {}
    }
    Ok(())
}

fn measure_item_name(
    id: u64,
    record: &RawRecord,
    exchange: &Exchange,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    Ok(record
        .partials
        .iter()
        .find(|partial| partial.name == "REPRESENTATION_ITEM")
        .and_then(|partial| partial.parameters.first())
        .or_else(|| {
            record
                .partials
                .iter()
                .find(|partial| partial.name == "MEASURE_REPRESENTATION_ITEM")
                .and_then(|partial| partial.parameters.first())
        })
        .map(|value| {
            decode_text_charged(
                exchange,
                value,
                losses,
                id,
                "measure item name",
                StepLossCode::MetadataStringInvalid,
                ctx,
            )
        })
        .transpose()?
        .flatten()
        .filter(|name| !name.is_empty()))
}

fn measure_context<'a>(
    geometry: &GeometryData,
    id: u64,
    losses: &'a mut Vec<LossNote>,
    graph_limit: usize,
) -> MeasureContext<'a> {
    MeasureContext {
        length_scale: geometry.units.length([id]).get(),
        angle_scale: geometry.units.angle([id]).get(),
        graph_limit,
        losses,
    }
}

fn first_measure<'a>(
    values: impl IntoIterator<Item = &'a Value>,
    exchange: &Exchange,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PmiValue>, CodecError> {
    for value in values {
        if let Some(measured) = measure(value, exchange, measurements, ctx)? {
            return Ok(Some(measured));
        }
    }
    Ok(None)
}

fn measure(
    value: &Value,
    exchange: &Exchange,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PmiValue>, CodecError> {
    measure_inner(value, exchange, &mut BTreeSet::new(), 0, measurements, ctx)
}

fn measure_inner(
    value: &Value,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    depth: usize,
    measurements: &mut MeasureContext<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PmiValue>, CodecError> {
    if depth >= measurements.graph_limit {
        return Ok(None);
    }
    let _nested = ctx.enter_nested("step_pmi_measure_eval_walk")?;
    Ok(match value {
        Value::Integer(value) => cadmpeg_core::convert::f64_from_i64(*value)
            .and_then(|value| PmiValue::new(value, PmiQuantity::Ratio)),
        Value::Real(value) => PmiValue::new(value.get(), PmiQuantity::Ratio),
        Value::Typed(name, value) => value.number().and_then(|number| {
            PmiValue::new(
                if name.contains("LENGTH") {
                    number * measurements.length_scale
                } else if name.contains("ANGLE") {
                    number * measurements.angle_scale
                } else {
                    number
                },
                if name.contains("LENGTH") {
                    PmiQuantity::Length
                } else if name.contains("ANGLE") {
                    PmiQuantity::Angle
                } else {
                    PmiQuantity::Ratio
                },
            )
        }),
        Value::Reference(id) => {
            if active.contains(id) {
                return Ok(None);
            }
            ctx.insert_btree_set(active, *id, "step_pmi_measure_eval_active")?;
            let Some(record) = exchange.records().get(id) else {
                active.remove(id);
                return Ok(None);
            };
            let mut quantity = None;
            for parameter in record
                .partials
                .iter()
                .flat_map(|partial| &partial.parameters)
            {
                quantity = measure_quantity(parameter, ctx)?;
                if quantity.is_some() {
                    break;
                }
            }
            let quantity = quantity.unwrap_or_else(|| {
                if record
                    .partials
                    .iter()
                    .any(|partial| partial.name.contains("LENGTH"))
                {
                    PmiQuantity::Length
                } else if record
                    .partials
                    .iter()
                    .any(|partial| partial.name.contains("ANGLE"))
                {
                    PmiQuantity::Angle
                } else {
                    PmiQuantity::Ratio
                }
            });
            let unit = record
                .partials
                .iter()
                .flat_map(|partial| &partial.parameters)
                .filter_map(Value::reference)
                .find(|unit| {
                    exchange.records().get(unit).is_some_and(|record| {
                        record.partials.iter().any(|partial| {
                            matches!(partial.name.as_str(), "LENGTH_UNIT" | "PLANE_ANGLE_UNIT")
                        })
                    })
                });
            let scale = match quantity {
                PmiQuantity::Length => {
                    let resolved = match unit {
                        Some(unit) => super::geometry::unit_scale_mm(
                            unit,
                            exchange,
                            &mut BTreeSet::new(),
                            ctx,
                        )?,
                        None => None,
                    };
                    if let Some(scale) = resolved {
                        scale.get()
                    } else {
                        ctx.push_vec(measurements.losses, StepLossCode::PmiLengthUnitUnresolved.note(format!(
                                "PMI length measure #{id} unit scale did not resolve; the document length scale was used"
                            )), "step_pmi_losses")?;
                        measurements.length_scale
                    }
                }
                PmiQuantity::Angle => {
                    let resolved = match unit {
                        Some(unit) => super::geometry::unit_scale_radians(
                            unit,
                            exchange,
                            &mut BTreeSet::new(),
                            ctx,
                        )?,
                        None => None,
                    };
                    if let Some(scale) = resolved {
                        scale.get()
                    } else {
                        ctx.push_vec(measurements.losses, StepLossCode::PmiAngleUnitUnresolved.note(format!(
                                "PMI angle measure #{id} unit scale did not resolve; the document plane-angle scale was used"
                            )), "step_pmi_losses")?;
                        measurements.angle_scale
                    }
                }
                PmiQuantity::Ratio => 1.0,
            };
            let mut result = None;
            for parameter in record
                .partials
                .iter()
                .flat_map(|partial| &partial.parameters)
            {
                result = ValueExt::typed_number(parameter)
                    .and_then(|number| PmiValue::new(number * scale, quantity));
                if result.is_none() {
                    result =
                        measure_inner(parameter, exchange, active, depth + 1, measurements, ctx)?;
                }
                if result.is_some() {
                    break;
                }
            }
            active.remove(id);
            result
        }
        Value::List(values) => {
            let mut result = None;
            for value in values {
                result = measure_inner(value, exchange, active, depth + 1, measurements, ctx)?;
                if result.is_some() {
                    break;
                }
            }
            result
        }
        _ => None,
    })
}

fn measure_quantity(
    value: &Value,
    ctx: &DecodeContext<'_>,
) -> Result<Option<PmiQuantity>, CodecError> {
    let _depth = ctx.enter_nested("step_pmi_measure_quantity_walk")?;
    Ok(match value {
        Value::Typed(name, value) => {
            if name.contains("LENGTH") {
                Some(PmiQuantity::Length)
            } else if name.contains("ANGLE") {
                Some(PmiQuantity::Angle)
            } else if name.contains("RATIO") {
                Some(PmiQuantity::Ratio)
            } else {
                measure_quantity(value, ctx)?
            }
        }
        Value::List(values) => {
            let mut quantity = None;
            for value in values {
                quantity = measure_quantity(value, ctx)?;
                if quantity.is_some() {
                    break;
                }
            }
            quantity
        }
        _ => None,
    })
}

#[cfg(test)]
pub(crate) mod tests;
