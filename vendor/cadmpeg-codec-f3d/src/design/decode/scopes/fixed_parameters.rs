// SPDX-License-Identifier: Apache-2.0
//! Exact fixed extrude, fillet and chamfer parameter scopes.

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use super::shared_frames::FixedScalarFrame;
use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids::native_stream;
use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::extrude::DesignExtrudePrologue;
use crate::records::feature::fixed_parameters::DesignFixedChamferDistance;
use crate::records::feature::fixed_parameters::DesignFixedChamferParameters;
use crate::records::feature::fixed_parameters::DesignFixedExtrudeDistance;
use crate::records::feature::fixed_parameters::DesignFixedExtrudeParameters;
use crate::records::feature::fixed_parameters::DesignFixedExtrudeScalar;
use crate::records::feature::fixed_parameters::DesignFixedFilletGroup;
use crate::records::feature::fixed_parameters::DesignFixedFilletParameters;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameter;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::{NonZeroReal, PositiveReal};

pub(super) fn exact_fixed_extrude_parameters(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[crate::records::parameters::DesignParameterOwner],
) -> Option<DesignFixedExtrudeParameters> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Extrude)
        || scope
            .extrude_prologue()
            .and_then(DesignExtrudePrologue::extent)
            != Some(DesignExtrudeExtent::OneSidedDistance)
    {
        return None;
    }
    let mut fixed_lanes = [None; 2];
    let mut fixed_count = 0;
    let mut embedded_distance = None;
    for record_index in scope.reference_members().values() {
        if let Some(scalar) = exact_fixed_scalar(bytes, records, *record_index)
            .filter(|scalar| scalar.owner_record_index == Some(scope.record_index))
        {
            let slot = fixed_lanes.get_mut(fixed_count)?;
            *slot = Some((*record_index, scalar));
            fixed_count += 1;
        }
        if let Some(scalar) =
            exact_embedded_extrude_distance(bytes, records, *record_index, scope.record_index)
        {
            if embedded_distance.replace((*record_index, scalar)).is_some() {
                return None;
            }
        }
    }
    let mut along_distance = embedded_distance.map(|(record_index, lane)| {
        DesignFixedExtrudeDistance::DistanceConstruction(DesignFixedExtrudeScalar {
            value: lane.value,
            record_index,
            value_offset: lane.value_offset,
        })
    });
    let mut taper_angle = None;
    let mut seen_fixed_ordinals = [false; 2];
    for (record_index, lane) in fixed_lanes.into_iter().flatten() {
        let ordinal = usize::from(lane.ordinal);
        if ordinal >= seen_fixed_ordinals.len() || seen_fixed_ordinals[ordinal] {
            return None;
        }
        seen_fixed_ordinals[ordinal] = true;
        let source_kind = parameter_owners
            .iter()
            .find(|owner| {
                native_stream(owner.id()) == native_stream(&scope.id)
                    && owner.scope_record_index() == scope.record_index
                    && owner.record_index() == record_index
            })
            .and_then(|owner| {
                parameters
                    .iter()
                    .find(|parameter| {
                        native_stream(&parameter.id) == native_stream(&scope.id)
                            && parameter.record_index == owner.parameter_record_index()
                    })
                    .map(crate::records::parameters::DesignParameter::source_kind)
            });
        match source_kind {
            Some("AlongDistance") if along_distance.is_none() => {
                let value = NonZeroReal::new(lane.value.get())?;
                along_distance = Some(DesignFixedExtrudeDistance::FixedScalar(
                    DesignFixedExtrudeScalar {
                        value,
                        record_index,
                        value_offset: lane.value_offset,
                    },
                ));
            }
            Some("TaperAngle") if taper_angle.is_none() => {
                taper_angle = Some(DesignFixedExtrudeScalar {
                    value: lane.value,
                    record_index,
                    value_offset: lane.value_offset,
                });
            }
            Some("AlongDistance") if along_distance.is_some() && lane.value.get() == 0.0 => {}
            Some(_) => return None,
            None => match lane.ordinal {
                0 if along_distance.is_none() && lane.value.get() != 0.0 => {
                    let value = NonZeroReal::new(lane.value.get())?;
                    along_distance = Some(DesignFixedExtrudeDistance::FixedScalar(
                        DesignFixedExtrudeScalar {
                            value,
                            record_index,
                            value_offset: lane.value_offset,
                        },
                    ));
                }
                0 if along_distance.is_some() && lane.value.get() == 0.0 => {}
                1 if taper_angle.is_none() => {
                    taper_angle = Some(DesignFixedExtrudeScalar {
                        value: lane.value,
                        record_index,
                        value_offset: lane.value_offset,
                    });
                }
                _ => return None,
            },
        }
    }
    if along_distance.is_none() && taper_angle.is_none() {
        return None;
    }
    Some(DesignFixedExtrudeParameters {
        along_distance,
        taper_angle,
    })
}

fn exact_embedded_extrude_distance(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Option<FixedScalarFrame<PositiveReal>> {
    let mut candidates = records.frames(record_index).filter_map(|(start, end)| {
        (end.checked_sub(start)? == 100).then_some(())?;
        let (_, after_tag) = lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
        let first_auxiliary = record_index.checked_add(1)?;
        let second_auxiliary = record_index.checked_add(2)?;
        if after_tag != start + 7
            || bytes.get(start + 11..start + 21) != Some(&[0; 10])
            || marked_record_reference(bytes, start + 21)? != scope_record_index
            || bytes.get(start + 26..start + 32) != Some(&[0; 6])
            || View::u32_le_at(bytes, start + 32)? != 1
            || marked_record_reference(bytes, start + 36).is_none()
            || bytes.get(start + 41..start + 47) != Some(&[0; 6])
            || View::u32_le_at(bytes, start + 47)? != 210
            || View::u32_le_at(bytes, start + 59)? != 210
            || marked_record_reference(bytes, start + 63)? != second_auxiliary
            || bytes.get(start + 68..start + 74) != Some(&[0; 6])
            || bytes.get(start + 74..start + 77) != Some(&[1, 0, 0])
            || marked_record_reference(bytes, start + 77)? != first_auxiliary
            || bytes.get(start + 82..start + 89) != Some(&[0; 7])
            || marked_record_reference(bytes, start + 89)? != scope_record_index
            || bytes.get(start + 94..start + 100) != Some(&[0; 6])
        {
            return None;
        }
        let value = PositiveReal::new(View::f64_le_at(bytes, start + 51)?)?;
        Some(FixedScalarFrame {
            owner_record_index: Some(scope_record_index),
            ordinal: 0,
            value,
            value_offset: u64::try_from(start + 51).ok()?,
        })
    });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

pub(super) fn exact_fixed_fillet_parameters(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignFixedFilletParameters>, CodecError> {
    use crate::records::feature::fixed_parameters::{
        DesignFixedFilletIntermediate, DesignFixedFilletLaw, DesignFixedFilletScalar,
    };
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Fillet) {
        return Ok(None);
    }
    let mut lanes = Vec::new();
    for record_index in scope.reference_members().values() {
        if let Some(scalar) = exact_fixed_scalar(bytes, records, *record_index)
            .filter(|scalar| scalar.owner_record_index == Some(scope.record_index))
        {
            ctx.reserve_vec(&mut lanes, 1, "f3d fixed Fillet scalar lanes")?;
            lanes.push((*record_index, scalar));
        }
    }
    if lanes.is_empty()
        || lanes
            .iter()
            .enumerate()
            .any(|(ordinal, (_, scalar))| usize::from(scalar.ordinal) != ordinal)
    {
        return Ok(None);
    }

    let scalar = |(record_index, scalar): &(u32, FixedScalarFrame)| DesignFixedFilletScalar {
        value: scalar.value,
        record_index: *record_index,
        value_offset: scalar.value_offset,
    };
    let group = |tangency_lane: Option<&(u32, FixedScalarFrame)>, law: DesignFixedFilletLaw| {
        DesignFixedFilletGroup::try_new(tangency_lane.map(scalar), law).ok()
    };
    let mut groups = Vec::new();
    let group_count = if lanes.len() == 1 || !lanes.len().is_multiple_of(2) {
        1
    } else {
        lanes.len() / 2
    };

    ctx.reserve_vec(&mut groups, group_count, "f3d fixed Fillet groups")?;
    if lanes.len() == 1 {
        let Some(value) = group(None, DesignFixedFilletLaw::Constant(scalar(&lanes[0]))) else {
            return Ok(None);
        };
        groups.push(value);
    } else if lanes.len() % 2 == 0 {
        for pair in lanes.chunks_exact(2) {
            let Some(value) = group(
                Some(&pair[0]),
                DesignFixedFilletLaw::Constant(scalar(&pair[1])),
            ) else {
                return Ok(None);
            };
            groups.push(value);
        }
    } else {
        let intermediate_count = (lanes.len() - 3) / 2;

        let mut intermediate = Vec::new();
        ctx.reserve_vec(
            &mut intermediate,
            intermediate_count,
            "f3d fixed Fillet intermediate rows",
        )?;
        for pair in lanes[3..].chunks_exact(2) {
            intermediate.push(DesignFixedFilletIntermediate {
                radius: scalar(&pair[0]),
                parameter: scalar(&pair[1]),
            });
        }
        let Some(value) = group(
            Some(&lanes[0]),
            DesignFixedFilletLaw::Variable {
                start: scalar(&lanes[1]),
                end: scalar(&lanes[2]),
                intermediate,
            },
        ) else {
            return Ok(None);
        };
        groups.push(value);
    }
    Ok(Some(DesignFixedFilletParameters { groups }))
}

pub(super) fn exact_fixed_chamfer_parameters(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignFixedChamferParameters> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Chamfer) {
        return None;
    }
    let stream = native_stream(&scope.id);
    if parameter_owners.iter().any(|owner| {
        stream.is_some()
            && native_stream(owner.id()) == stream
            && owner.scope_record_index() == scope.record_index
    }) {
        return None;
    }
    let mut lanes = [None; 2];
    let mut lane_count = 0;
    for record_index in scope.reference_members().values() {
        if let Some(scalar) = exact_fixed_scalar(bytes, records, *record_index)
            .filter(|scalar| scalar.owner_record_index == Some(scope.record_index))
        {
            if usize::from(scalar.ordinal) != lane_count {
                return None;
            }
            let slot = lanes.get_mut(lane_count)?;
            *slot = Some((*record_index, scalar));
            lane_count += 1;
        }
    }
    let mut distances = lanes.into_iter().flatten().map(|(record_index, scalar)| {
        Some(DesignFixedChamferDistance {
            value: cadmpeg_ir::scalar::PositiveReal::new(scalar.value.get())?,
            record_index,
            value_offset: scalar.value_offset,
        })
    });
    let first = distances.next()??;
    Some(match distances.next() {
        Some(Some(second)) => DesignFixedChamferParameters::TwoDistances { first, second },
        None => DesignFixedChamferParameters::EqualDistance { distance: first },
        Some(None) => return None,
    })
}

pub(super) fn unique_revolve_angle_owner<'a>(
    scope: &DesignParameterScope,
    parameter_owners: &'a [DesignParameterOwner],
    record_index: Option<u32>,
) -> Option<&'a DesignParameterOwner> {
    let mut candidates = parameter_owners.iter().filter(|owner| {
        native_stream(owner.id()) == native_stream(&scope.id)
            && owner.scope_record_index() == scope.record_index
            && scope
                .reference_members()
                .values()
                .any(|value| value == &owner.record_index())
            && record_index.is_none_or(|index| owner.record_index() == index)
            && owner.local_ordinal() == 0
            && owner.evaluated_value().get() > 0.0
    });
    let angle = candidates.next()?;
    candidates.next().is_none().then_some(angle)
}
