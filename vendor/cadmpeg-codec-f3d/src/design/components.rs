// SPDX-License-Identifier: Apache-2.0
//! Project exact local component operations into neutral product structure.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::ScopedReservation;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};

use crate::records::feature::{
    assembly::DesignAssemblyOperandQualifier, assembly_features::DesignComponentOccurrence,
    scope::DesignParameterScope,
};

/// Project components and occurrences proven by local component history operations.
pub(crate) fn project_local_components(
    ctx: &DecodeContext<'_>,
    scopes: &[DesignParameterScope],
    native_occurrences: &[DesignComponentOccurrence],
) -> Result<(Vec<ProductDefinition>, Vec<Occurrence>), cadmpeg_core::CodecError> {
    let mut lookup_storage = ctx.reserve_scoped(0, "f3d component lookup storage")?;
    let mut components = BTreeMap::new();
    let mut occurrences = BTreeMap::new();
    let mut native_by_guid = BTreeMap::new();
    for occurrence in native_occurrences {
        lookup_storage.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
            let mut key = ctx.copy_retained_text(
                occurrence.occurrence_guid.as_str(),
                "f3d component native occurrence key",
            )?;
            key.make_ascii_lowercase();
            ctx.admit_btree_entry(
                &native_by_guid,
                &key,
                "f3d component native occurrence index",
            )?;
            match native_by_guid.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Some(occurrence));
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    *entry.get_mut() = None;
                }
            }
            Ok(())
        })?;
    }

    for scope in scopes {
        if let Some(qualifiers) = scope.assembly_alignment().and_then(
            super::super::records::feature::assembly::DesignAssemblyAlignment::operand_qualifiers,
        ) {
            for qualifier in &qualifiers {
                let DesignAssemblyOperandQualifier::OccurrencePath { path } = qualifier else {
                    continue;
                };
                let Some(guid) = path.occurrence_guids().first() else {
                    continue;
                };
                let (key, reservation) = temporary_lowercase_component_key(
                    ctx,
                    guid.value.as_str(),
                    "f3d component assembly occurrence lookup",
                )?;
                let root = native_by_guid.get(&key).copied().flatten();
                drop(key);
                drop(reservation);
                let Some(root) = root else {
                    continue;
                };
                project_occurrence(
                    ctx,
                    (&mut lookup_storage, &mut components),
                    &mut occurrences,
                    &native_by_guid,
                    &root.component_guid,
                    &root.occurrence_guid,
                    root.transform().map_or(
                        [
                            [1.0, 0.0, 0.0, 0.0],
                            [0.0, 1.0, 0.0, 0.0],
                            [0.0, 0.0, 1.0, 0.0],
                            [0.0, 0.0, 0.0, 1.0],
                        ],
                        |frame| frame.value.into(),
                    ),
                )?;
            }
        }
        if let Some(operation) = scope.copy_paste_component_operation() {
            project_occurrence(
                ctx,
                (&mut lookup_storage, &mut components),
                &mut occurrences,
                &native_by_guid,
                &operation.component_guid,
                &operation.source_occurrence_guid,
                operation.source_transform,
            )?;
            project_occurrence(
                ctx,
                (&mut lookup_storage, &mut components),
                &mut occurrences,
                &native_by_guid,
                &operation.component_guid,
                &operation.copied_occurrence_guid,
                operation.copied_transform,
            )?;
        }
        if let Some(construction) = scope.derived_instance_construction() {
            project_occurrence(
                ctx,
                (&mut lookup_storage, &mut components),
                &mut occurrences,
                &native_by_guid,
                &construction.component_guid,
                &construction.occurrence_guid,
                construction.transform,
            )?;
        }
        let Some(
            crate::records::feature::patterns::DesignRectangularPatternInstances::Components {
                component_guid,
                seed,
                generated,
            },
        ) = scope
            .rectangular_pattern_construction()
            .and_then(|construction| construction.instances())
        else {
            continue;
        };
        for occurrence in std::iter::once(seed).chain(generated) {
            project_occurrence(
                ctx,
                (&mut lookup_storage, &mut components),
                &mut occurrences,
                &native_by_guid,
                component_guid,
                &occurrence.occurrence_guid,
                occurrence.instance.transform.value,
            )?;
        }
    }

    let mut occurrence_output = Vec::new();
    ctx.reserve_vec(
        &mut occurrence_output,
        occurrences.len(),
        "f3d component occurrence output",
    )?;
    occurrence_output.extend(occurrences.into_values());
    let mut occurrences = occurrence_output;
    for (ordinal, occurrence) in occurrences.iter_mut().enumerate() {
        occurrence.ordinal = u32::try_from(ordinal).map_err(|_| {
            cadmpeg_core::CodecError::malformed("Fusion Design occurrence ordinal exceeds u32")
        })?;
    }

    let mut component_output = Vec::new();
    ctx.reserve_vec(
        &mut component_output,
        components.len(),
        "f3d component output",
    )?;
    component_output.extend(components.into_values());
    Ok((component_output, occurrences))
}

/// Project a proven local occurrence into a `DerivedInstance` feature.
pub(crate) fn project_derived_instance_features(
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
) {
    for scope in scopes {
        let Some(construction) = scope.derived_instance_construction() else {
            continue;
        };
        let Some(feature) = features
            .iter_mut()
            .find(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()))
        else {
            continue;
        };
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ) {
            continue;
        }
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::InsertComponent {
                    occurrence: crate::ids::neutral_component_occurrence_id(
                        &construction.occurrence_guid,
                    ),
                },
            ));
    }
}

/// Project the occurrence side of an external `Component Insert` when its
/// target reference table is not present in this container.
///
/// The operation still has a complete local placement. The prototype remains
/// explicitly unresolved because the source does not provide a target
/// document or a target object that this decoder can identify. Keeping the
/// occurrence in the product graph lets the feature retain its operation and
/// transform while native storage retains the exact external-reference role.
pub(crate) fn project_unresolved_component_insert_occurrences(
    ctx: &DecodeContext<'_>,
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    ordinal_start: usize,
) -> Result<Vec<Occurrence>, cadmpeg_core::CodecError> {
    let mut occurrences = Vec::new();
    for scope in scopes {
        let Some(construction) = scope.component_insert_construction() else {
            continue;
        };
        let Some(feature) = features
            .iter_mut()
            .find(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()))
        else {
            continue;
        };
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ) {
            continue;
        }

        ctx.reserve_vec(&mut occurrences, 1, "f3d unresolved component occurrence")?;
        let occurrence_id =
            crate::design::identity::neutral_component_insert_occurrence_id(ctx, scope)?;
        let feature_occurrence_id = occurrence_id
            .try_clone_for_decode(ctx, "f3d unresolved component feature occurrence id")?;
        let name =
            ctx.copy_retained_text(&construction.neutron_role, "f3d unresolved component name")?;
        let native_ref =
            ctx.copy_retained_text(&scope.id, "f3d unresolved component native reference")?;
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::InsertComponent {
                    occurrence: feature_occurrence_id,
                },
            ));
        occurrences.push(Occurrence {
            id: occurrence_id,
            prototype: PrototypeReference::Unresolved {},
            parent: OccurrenceParent::Root {},
            ordinal: u32::try_from(ordinal_start.checked_add(occurrences.len()).ok_or_else(
                || {
                    cadmpeg_core::CodecError::malformed(
                        "Fusion Design occurrence ordinal exceeds u32",
                    )
                },
            )?)
            .map_err(|_| {
                cadmpeg_core::CodecError::malformed("Fusion Design occurrence ordinal exceeds u32")
            })?,
            transform: neutral_transform(*construction.transform())?,
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: Some(name),
            visible: None,
            link: None,
            native_ref: Some(native_ref),
        });
    }
    Ok(occurrences)
}

fn temporary_lowercase_component_key<'a>(
    ctx: &'a DecodeContext<'_>,
    source: &str,
    operation: &'static str,
) -> Result<(String, ScopedReservation<'a>), cadmpeg_core::CodecError> {
    let mut reservation = ctx.reserve_scoped(0, operation)?;
    let mut key = ctx.copy_scoped_text(source, &mut reservation, operation)?;
    key.make_ascii_lowercase();
    Ok((key, reservation))
}

fn project_occurrence(
    ctx: &DecodeContext<'_>,
    component_storage: (
        &mut ScopedReservation<'_>,
        &mut BTreeMap<String, ProductDefinition>,
    ),
    occurrences: &mut BTreeMap<String, Occurrence>,
    native_by_guid: &BTreeMap<String, Option<&DesignComponentOccurrence>>,
    component_guid: &crate::records::mesh::DesignRelaxedGuidText,
    occurrence_guid: &crate::records::mesh::DesignRelaxedGuidText,
    transform: impl Into<[[f64; 4]; 4]>,
) -> Result<(), cadmpeg_core::CodecError> {
    let (lookup_storage, components) = component_storage;
    let component_id = crate::ids::neutral_component_id(component_guid);
    let transform = neutral_transform(transform)?;
    project_component(ctx, lookup_storage, components, component_guid)?;
    let occurrence_id = crate::ids::neutral_component_occurrence_id(occurrence_guid);
    if !occurrences.contains_key(occurrence_id.as_str()) {
        let key = lookup_storage.with_storage(|| -> Result<String, cadmpeg_core::CodecError> {
            let key =
                ctx.copy_retained_text(occurrence_id.as_str(), "f3d component occurrence map key")?;
            ctx.admit_btree_entry(occurrences, &key, "f3d component occurrence map entry")?;
            Ok(key)
        })?;
        let (native_key, reservation) = temporary_lowercase_component_key(
            ctx,
            occurrence_guid.as_str(),
            "f3d component native occurrence lookup",
        )?;
        let native_ref = native_by_guid
            .get(&native_key)
            .copied()
            .flatten()
            .map(|occurrence| {
                ctx.copy_retained_text(&occurrence.id, "f3d component occurrence native reference")
            })
            .transpose()?;
        drop(native_key);
        drop(reservation);
        occurrences.insert(
            key,
            Occurrence {
                id: occurrence_id,
                prototype: PrototypeReference::Local {
                    definition: component_id,
                },
                parent: OccurrenceParent::Root {},
                ordinal: 0,
                transform,
                linked_prototype: None,
                scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
                name: None,
                visible: None,
                link: None,
                native_ref,
            },
        );
    }
    Ok(())
}

fn project_component(
    ctx: &DecodeContext<'_>,
    lookup_storage: &mut ScopedReservation<'_>,
    components: &mut BTreeMap<String, ProductDefinition>,
    component_guid: &crate::records::mesh::DesignRelaxedGuidText,
) -> Result<(), cadmpeg_core::CodecError> {
    let component_id = crate::ids::neutral_component_id(component_guid);
    if !components.contains_key(component_id.as_str()) {
        let key = lookup_storage.with_storage(|| -> Result<String, cadmpeg_core::CodecError> {
            let key =
                ctx.copy_retained_text(component_id.as_str(), "f3d component definition map key")?;
            ctx.admit_btree_entry(components, &key, "f3d component definition map entry")?;
            Ok(key)
        })?;
        components.insert(
            key,
            ProductDefinition {
                id: component_id,
                kind: ProductDefinitionKind::Part,
                source_name: None,
                label: None,
                description: None,
                part_number: None,
                bom_properties: BTreeMap::new(),
                bodies: Vec::new(),
                native_ref: None,
            },
        );
    }
    Ok(())
}

/// A millimetre placement projected from source centimetres.
pub(crate) fn neutral_transform(
    transform: impl Into<[[f64; 4]; 4]>,
) -> Result<cadmpeg_ir::transform::Transform, cadmpeg_core::CodecError> {
    let source = transform.into();
    if source[3] != [0.0, 0.0, 0.0, 1.0] {
        return Err(cadmpeg_core::CodecError::NotImplemented(
            "F3D placement must project to a finite affine transform".into(),
        ));
    }
    let mut rows = [source[0], source[1], source[2]];
    for row in &mut rows {
        row[3] *= 10.0;
    }
    cadmpeg_ir::transform::Transform::affine(rows).ok_or_else(|| {
        cadmpeg_core::CodecError::NotImplemented(
            "F3D placement must project to a finite affine transform".into(),
        )
    })
}

#[cfg(test)]
mod tests {
    use crate::design::test_support::identity_matrix;
    use crate::records::feature::{
        assembly_features::{
            DesignComponentOccurrence, DesignCopyPasteComponentOperation,
            DesignDerivedInstanceConstruction,
        },
        scope::DesignParameterScope,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    use cadmpeg_ir::products::PrototypeReference;

    #[test]
    fn local_component_placement_scales_only_translation_to_millimetres() {
        let transform = [
            [0.0, -1.0, 0.0, 1.25],
            [1.0, 0.0, 0.0, -2.5],
            [0.0, 0.0, 1.0, 3.75],
            [0.0, 0.0, 0.0, 1.0],
        ];
        assert_eq!(
            super::neutral_transform(transform).unwrap().rows(),
            [
                [0.0, -1.0, 0.0, 12.5],
                [1.0, 0.0, 0.0, -25.0],
                [0.0, 0.0, 1.0, 37.5],
                [0.0, 0.0, 0.0, 1.0],
            ]
        );
    }

    fn one_component_fixture() -> (DesignParameterScope, DesignComponentOccurrence) {
        const COMPONENT: &str = "11111111-2222-4333-8444-555555555555";
        const OCCURRENCE: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
        let mut scope = DesignParameterScope::empty(
            "f3d:synthetic:design-parameter-scope#1",
            crate::records::feature::scope::DesignFeatureKind::DerivedInstance,
            1,
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::DerivedInstance(slot) =
            scope.payload_mut()
        {
            *slot = Some(DesignDerivedInstanceConstruction {
                reference_record_index: 2,
                relation_record_index: 3,
                carrier_record_index: 4,
                component_guid: COMPONENT.to_owned().try_into().unwrap(),
                occurrence_guid: OCCURRENCE.to_owned().try_into().unwrap(),
                transform: identity_matrix().try_into().unwrap(),
                transform_offset: 0,
            });
        }
        let occurrence = DesignComponentOccurrence::try_new(
            crate::records::feature::assembly_features::DesignComponentOccurrenceDraft {
                id: "f3d:synthetic:design-component-occurrence#4".into(),
                class_tag: crate::records::references::DesignClassTag::try_from("380".to_owned())
                    .unwrap(),
                record_index: 4,
                byte_offset: 0,
                component_record_index: 2,
                component_guid: COMPONENT.to_owned().try_into().unwrap(),
                occurrence_guid: OCCURRENCE.to_owned().try_into().unwrap(),
                placement: crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Base,
            },
        )
        .unwrap();
        (scope, occurrence)
    }

    fn one_component_refusal(maximum: u64) -> cadmpeg_core::CodecError {
        let (scope, occurrence) = one_component_fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = maximum;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        super::project_local_components(&ctx, &[scope], &[occurrence])
            .expect_err("one component exceeds the selected collection limit")
    }

    fn one_component_byte_refusal(
        retained_limit: u64,
        materialized_limit: u64,
    ) -> cadmpeg_core::CodecError {
        let (scope, occurrence) = one_component_fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = retained_limit;
        policy.limits.max_materialized_bytes = materialized_limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        super::project_local_components(&ctx, &[scope], &[occurrence])
            .expect_err("one component exceeds the selected byte limit")
    }

    #[test]
    fn component_native_key_refuses_materialized_limit() {
        let error = one_component_byte_refusal(u64::MAX, 35);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "f3d component native occurrence key")
        );
    }

    #[test]
    fn component_native_key_refuses_retained_limit() {
        let (scope, occurrence) = one_component_fixture();
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::MaterializedBytes,
            "f3d component native occurrence key",
            0,
            |ctx| {
                super::project_local_components(
                    ctx,
                    std::slice::from_ref(&scope),
                    std::slice::from_ref(&occurrence),
                )
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
 if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "f3d component native occurrence key")
        );
    }

    #[test]
    fn component_definition_key_refuses_retained_limit() {
        let (scope, occurrence) = one_component_fixture();
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::MaterializedBytes,
            "f3d component definition map key",
            0,
            |ctx| {
                super::project_local_components(
                    ctx,
                    std::slice::from_ref(&scope),
                    std::slice::from_ref(&occurrence),
                )
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
 if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "f3d component definition map key")
        );
    }

    #[test]
    fn component_occurrence_key_refuses_retained_limit() {
        let (scope, occurrence) = one_component_fixture();
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::MaterializedBytes,
            "f3d component occurrence map key",
            0,
            |ctx| {
                super::project_local_components(
                    ctx,
                    std::slice::from_ref(&scope),
                    std::slice::from_ref(&occurrence),
                )
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
 if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "f3d component occurrence map key")
        );
    }

    #[test]
    fn component_native_reference_refuses_retained_limit() {
        let (scope, occurrence) = one_component_fixture();
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::RetainedBytes,
            "f3d component occurrence native reference",
            0,
            |ctx| {
                super::project_local_components(
                    ctx,
                    std::slice::from_ref(&scope),
                    std::slice::from_ref(&occurrence),
                )
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
 if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "f3d component occurrence native reference")
        );
    }

    #[test]
    fn component_occurrence_lookup_refuses_materialized_limit() {
        let (_, occurrence) = one_component_fixture();
        let mut native_by_guid = std::collections::BTreeMap::new();
        native_by_guid.insert(
            occurrence.occurrence_guid.as_str().to_ascii_lowercase(),
            Some(&occurrence),
        );
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::MaterializedBytes,
            "f3d component native occurrence lookup",
            0,
            |ctx| {
                let mut lookup_storage = ctx.reserve_scoped(0, "f3d component lookup storage")?;
                super::project_occurrence(
                    ctx,
                    (&mut lookup_storage, &mut std::collections::BTreeMap::new()),
                    &mut std::collections::BTreeMap::new(),
                    &native_by_guid,
                    &occurrence.component_guid,
                    &occurrence.occurrence_guid,
                    identity_matrix(),
                )
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "f3d component native occurrence lookup")
        );
    }

    #[test]
    fn native_occurrence_index_refuses_collection_limit() {
        let error = one_component_refusal(0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d component native occurrence index")
        );
    }

    #[test]
    fn component_definition_map_refuses_collection_limit() {
        let error = one_component_refusal(1);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d component definition map entry")
        );
    }

    #[test]
    fn component_occurrence_map_refuses_collection_limit() {
        let error = one_component_refusal(2);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d component occurrence map entry")
        );
    }

    #[test]
    fn component_occurrence_output_refuses_collection_limit() {
        let error = one_component_refusal(3);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d component occurrence output")
        );
    }

    #[test]
    fn component_definition_output_refuses_collection_limit() {
        let error = one_component_refusal(4);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d component output")
        );
    }

    fn unresolved_component_fixture() -> (DesignParameterScope, Feature) {
        let mut scope = DesignParameterScope::empty(
            "f3d:synthetic:design-parameter-scope#7",
            crate::records::feature::scope::DesignFeatureKind::ComponentInsert,
            7,
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::ComponentInsert(slot) =
            scope.payload_mut()
        {
            *slot = Some(
                crate::records::feature::assembly_features::DesignComponentInsertConstruction {
                    relation_record_index: 8,
                    carrier_record_index: 9,
                    occurrence_identity: None,
                    neutron_role: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into(),
                    neutron_role_offset: 0,
                    placement: None,
                },
            );
        }
        let feature = Feature {
            id: FeatureId::mint("f3d:model:feature#component-insert").unwrap(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: "ComponentInsert".into(),
                    parameters: std::collections::BTreeMap::new(),
                }),
            ),
            native_ref: Some(scope.id.clone()),
        };
        (scope, feature)
    }

    #[test]
    fn unresolved_component_occurrence_refuses_collection_limit() {
        let (scope, feature) = unresolved_component_fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::project_unresolved_component_insert_occurrences(
            &ctx,
            &mut [feature],
            &[scope],
            0,
        )
        .expect_err("one unresolved occurrence needs one collection item");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d unresolved component occurrence")
        );
    }

    fn unresolved_component_retained_refusal(maximum: u64) -> cadmpeg_core::CodecError {
        let (scope, feature) = unresolved_component_fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = maximum;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        super::project_unresolved_component_insert_occurrences(&ctx, &mut [feature], &[scope], 0)
            .expect_err("unresolved component text exceeds the retained limit")
    }

    #[test]
    fn unresolved_component_feature_id_refuses_retained_limit() {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "f3d unresolved component feature occurrence id",
            |cap| Err::<(), cadmpeg_core::CodecError>(unresolved_component_retained_refusal(cap)),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "f3d unresolved component feature occurrence id")
        );
    }

    #[test]
    fn unresolved_component_name_refuses_retained_limit() {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "f3d unresolved component name",
            |cap| Err::<(), cadmpeg_core::CodecError>(unresolved_component_retained_refusal(cap)),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "f3d unresolved component name")
        );
    }

    #[test]
    fn unresolved_component_native_reference_refuses_retained_limit() {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "f3d unresolved component native reference",
            |cap| Err::<(), cadmpeg_core::CodecError>(unresolved_component_retained_refusal(cap)),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "f3d unresolved component native reference")
        );
    }

    #[test]
    fn equal_component_guids_share_one_definition_across_carrier_references() {
        const COMPONENT: &str = "11111111-2222-4333-8444-555555555555";
        const SOURCE: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
        const COPY: &str = "aaaaaaaa-bbbb-4ccc-8ddd-ffffffffffff";
        let occurrence = |record_index: u32, component_record_index: u64, occurrence_guid: &str| {
            DesignComponentOccurrence::try_new(
                crate::records::feature::assembly_features::DesignComponentOccurrenceDraft {
                    id: format!(
                        "f3d:Design/BulkStream.dat:design-component-occurrence#{record_index}"
                    ),
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "256".to_owned(),
                    )
                    .unwrap(),
                    record_index,
                    byte_offset: u64::from(record_index),
                    component_record_index,
                    component_guid: COMPONENT.to_owned().try_into().expect("GUID"),
                    occurrence_guid: occurrence_guid.to_owned().try_into().expect("GUID"),
                    placement: crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Base,
                },
            )
            .unwrap()
        };
        let native_occurrences = [occurrence(100, 700, SOURCE), occurrence(101, 701, COPY)];
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#10",
            crate::records::feature::scope::DesignFeatureKind::CopyPaste,
            10,
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::CopyPaste(slot) =
            scope.payload_mut()
        {
            *slot = Some(DesignCopyPasteComponentOperation {
                relation_record_index: 20,
                source_occurrence_record_index: 100,
                copied_occurrence_record_index: 101,
                component_guid: COMPONENT.to_owned().try_into().expect("GUID"),
                source_occurrence_guid: SOURCE.to_owned().try_into().expect("GUID"),
                copied_occurrence_guid: COPY.to_owned().try_into().expect("GUID"),
                source_transform: identity_matrix().try_into().unwrap(),
                source_transform_offset: 0,
                copied_transform: identity_matrix().try_into().unwrap(),
                copied_transform_offset: 0,
            });
        }

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
        let (definitions, occurrences) =
            super::project_local_components(&ctx, &[scope], &native_occurrences).unwrap();

        assert_eq!(definitions.len(), 1);
        assert_eq!(occurrences.len(), 2);
        let definition = definitions[0].id.clone();
        assert!(occurrences.iter().all(|occurrence| matches!(
            &occurrence.prototype,
            PrototypeReference::Local { definition: actual } if actual == &definition
        )));
    }

    #[test]
    fn derived_instance_projects_its_joined_local_occurrence() {
        const COMPONENT: &str = "11111111-2222-4333-8444-555555555555";
        const OCCURRENCE: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#385",
            crate::records::feature::scope::DesignFeatureKind::DerivedInstance,
            385,
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::DerivedInstance(slot) =
            scope.payload_mut()
        {
            *slot = Some(DesignDerivedInstanceConstruction {
                reference_record_index: 305,
                relation_record_index: 383,
                carrier_record_index: 382,
                component_guid: COMPONENT.to_owned().try_into().expect("GUID"),
                occurrence_guid: OCCURRENCE.to_owned().try_into().expect("GUID"),
                transform: identity_matrix().try_into().unwrap(),
                transform_offset: 473,
            });
        }
        let native_occurrence = DesignComponentOccurrence::try_new(
            crate::records::feature::assembly_features::DesignComponentOccurrenceDraft {
                id: "f3d:Design/BulkStream.dat:design-component-occurrence#382".into(),
                class_tag: crate::records::references::DesignClassTag::try_from("380".to_owned())
                    .unwrap(),
                record_index: 382,
                byte_offset: 0,
                component_record_index: 305,
                component_guid: COMPONENT.to_owned().try_into().expect("GUID"),
                occurrence_guid: OCCURRENCE.to_owned().try_into().expect("GUID"),
                placement: crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Explicit {
                    ordinal: std::num::NonZeroU32::MIN,
                    transform: identity_matrix().try_into().unwrap(),
                },
            },
        )
        .unwrap();
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
        let (definitions, occurrences) =
            super::project_local_components(&ctx, &[scope.clone()], &[native_occurrence]).unwrap();
        assert_eq!(definitions.len(), 1);
        assert_eq!(occurrences.len(), 1);
        assert_eq!(
            occurrences[0].id,
            crate::ids::neutral_component_occurrence_id(&OCCURRENCE.to_owned().try_into().unwrap())
        );
        assert!(matches!(
            &occurrences[0].prototype,
            PrototypeReference::Local { definition }
                if definition == &crate::ids::neutral_component_id(&COMPONENT.to_owned().try_into().unwrap())
        ));

        let mut feature = Feature {
            id: FeatureId::mint("f3d:model:feature#derived").expect("identity grammar"),
            ordinal: 1,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: "DerivedInstance".into(),
                    parameters: std::collections::BTreeMap::new(),
                }),
            ),
            native_ref: None,
        };
        feature.native_ref = Some(scope.id.clone());
        super::project_derived_instance_features(std::slice::from_mut(&mut feature), &[scope]);
        assert_eq!(
            *feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::InsertComponent {
                occurrence: crate::ids::neutral_component_occurrence_id(
                    &OCCURRENCE.to_owned().try_into().unwrap()
                ),
            })
        );
    }
}
