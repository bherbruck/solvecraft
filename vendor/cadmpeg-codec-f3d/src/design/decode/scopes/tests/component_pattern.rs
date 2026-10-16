// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::component_constructions::bind_component_pattern_occurrences;
use crate::records::feature::assembly_features::{
    DesignComponentOccurrence, DesignComponentOccurrenceDraft, DesignComponentOccurrencePlacement,
};
use crate::records::feature::patterns::{
    DesignPatternInstance, DesignRectangularPatternConstruction,
    DesignRectangularPatternConstructionWire, DesignRectangularPatternInstances,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::identity::Located;
use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use std::num::NonZeroU32;

#[test]
fn component_pattern_generated_instances_refuse_collection_limit() {
    const COMPONENT: &str = "11111111-2222-4333-8444-555555555555";
    const SEED: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    const GENERATED: &str = "aaaaaaaa-bbbb-4ccc-8ddd-ffffffffffff";
    let identity = SketchPlacementMatrix::IDENTITY;
    let occurrence = |record_index: u32,
                      byte_offset: u64,
                      guid: &str,
                      placement: DesignComponentOccurrencePlacement| {
        DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
            id: format!("f3d:Design/BulkStream.dat:component-occurrence#{record_index}"),
            class_tag: "256".to_owned().try_into().unwrap(),
            record_index,
            byte_offset,
            component_record_index: 700,
            component_guid: COMPONENT.to_owned().try_into().unwrap(),
            occurrence_guid: guid.to_owned().try_into().unwrap(),
            placement,
        })
        .unwrap()
    };
    let occurrences = [
        occurrence(100, 500, SEED, DesignComponentOccurrencePlacement::Base),
        occurrence(
            101,
            1000,
            GENERATED,
            DesignComponentOccurrencePlacement::Explicit {
                ordinal: NonZeroU32::new(2).unwrap(),
                transform: identity,
            },
        ),
    ];
    let construction =
        DesignRectangularPatternConstruction::try_from(DesignRectangularPatternConstructionWire {
            u_count: 2,
            v_count: 1,
            u_extent: 1.0,
            v_extent: 0.0,
            owner_record_indices: [1, 2, 3, 4],
            value_offsets: [10, 20, 30, 40],
            instances: Some(DesignRectangularPatternInstances::Bodies(vec![
                DesignPatternInstance {
                    record_index: 100,
                    transform: Located {
                        value: identity,
                        offset: 0,
                    },
                },
                DesignPatternInstance {
                    record_index: 101,
                    transform: Located {
                        value: identity,
                        offset: 1209,
                    },
                },
            ])),
        })
        .unwrap();
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:pattern#200",
        DesignFeatureKind::RectangularPattern,
        200,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 2000;
            draft.reference_count_offset = 2009;
            draft.paired_byte_offset = 2000 + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let DesignScopePayloadMut::RectangularPattern(slot) = scope.payload_mut() {
        *slot = Some(construction);
    }
    for skip in 0..3 {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::RetainedBytes,
            "retain F3D pattern GUID",
            skip,
            |ctx| bind_component_pattern_occurrences(ctx, &mut scope.clone(), &occurrences),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::RetainedBytes
                && failure.operation == "retain F3D pattern GUID")
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = bind_component_pattern_occurrences(&ctx, &mut scope, &occurrences).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d component pattern generated instances")
    );

    let arena = DecodeArena::new();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    bind_component_pattern_occurrences(&ctx, &mut scope, &occurrences).unwrap();
    let Some(DesignRectangularPatternInstances::Components {
        component_guid,
        seed,
        generated,
    }) = scope
        .rectangular_pattern_construction()
        .and_then(|construction| construction.instances())
    else {
        panic!("admitted component pattern instances");
    };
    assert_eq!(component_guid.as_str(), COMPONENT);
    assert_eq!(seed.occurrence_guid.as_str(), SEED);
    assert_eq!(generated.len(), 1);
    assert_eq!(generated[0].occurrence_guid.as_str(), GENERATED);
}
