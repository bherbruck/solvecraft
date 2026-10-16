// SPDX-License-Identifier: Apache-2.0
use crate::design::feature_project::project_boundary_fill;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::{Located, ReferenceRun};
use crate::records::references::DesignClassTag;
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::ResourceDimension;

pub(super) fn group(
    record_index: u32,
    ordinal: u32,
    members: &[u32],
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("f3d:Design/BulkStream.dat:design-construction-operand-group#{record_index}"),
        scope_record_index: 100,
        scope_reference_ordinal: ordinal,
        record_index,
        byte_offset: 0,
        class_tag: DesignClassTag::try_from("000".to_owned()).unwrap(),
        members: members
            .iter()
            .enumerate()
            .map(|(index, value)| Located {
                value: *value,
                offset: u64::try_from(index).unwrap() * 11,
            })
            .collect(),
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 0,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 18,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 22,
                variant: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(role),
        role_offset: 0,
        paired_class_tag: DesignClassTag::try_from("000".to_owned()).unwrap(),
        paired_byte_offset: 0,
    })
    .unwrap()
}

fn fixture() -> (DesignParameterScope, [DesignConstructionOperandGroup; 2]) {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#100",
        DesignFeatureKind::BoundaryFill,
        100,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![100, 200, 201, 300, 301, 400]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (
        scope,
        [
            group(100, 0, &[200, 201], DesignOperandRole::BODIES_A),
            group(300, 3, &[301], DesignOperandRole::ROLE_0X5),
        ],
    )
}

fn assert_refusal(operation: &'static str, dimension: ResourceDimension) {
    let (scope, groups) = fixture();
    let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
        project_boundary_fill(ctx, &scope, &groups)
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.operation == operation && failure.dimension == dimension)
    );
}

#[test]
fn boundary_fill_group_refuses_collection_limit() {
    assert_refusal("f3d BoundaryFill group", ResourceDimension::CollectionItems);
}

#[test]
fn boundary_fill_cell_refuses_collection_limit() {
    assert_refusal("f3d BoundaryFill cell", ResourceDimension::CollectionItems);
}

#[test]
fn boundary_fill_cell_id_refuses_retained_limit() {
    assert_refusal("f3d BoundaryFill cell id", ResourceDimension::RetainedBytes);
}

#[test]
fn boundary_fill_tool_id_refuses_retained_limit() {
    assert_refusal("f3d BoundaryFill tool id", ResourceDimension::RetainedBytes);
}
