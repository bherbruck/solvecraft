// SPDX-License-Identifier: Apache-2.0

use crate::records::decal::DesignRecordHeader;

use crate::records::dimensions::DesignRecipeReference;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::recipes::ConstructionRecipe;
use crate::records::recipes::ConstructionRecipeKind;

use crate::test_support::indexed_header;

use cadmpeg_ir::ids::FaceId;

fn parse_edge_operand(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    scope: &DesignParameterScope,
    ordinal: u32,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
    terminal_group_limit: Option<u64>,
) -> Option<crate::records::topology::edge_identity::DesignEdgeOperand> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    crate::design::decode::operands::parse_edge_operand(
        &ctx,
        bytes,
        records,
        scope,
        (ordinal, header),
        recipes,
        terminal_group_limit,
    )
    .map(|result| result.expect("recipe allocation admitted"))
}

fn parse_face_operand(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    scope: &DesignParameterScope,
    (ordinal, group_ownership): (u32, Option<(u32, u32)>),
    next_byte_offset: Option<u64>,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
) -> Option<crate::records::topology::face::DesignFaceOperand> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    crate::design::decode::operands::parse_face_operand(
        &ctx,
        bytes,
        records,
        crate::design::decode::operands::FaceOperandFrame {
            scope,
            scope_reference_ordinal: ordinal,
            group_ownership,
            next_byte_offset,
            header,
        },
        recipes,
    )
    .map(|result| result.expect("recipe allocation admitted"))
}

fn parse_vertex_recipe(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    stream: &str,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
) -> Option<crate::records::feature::work_geometry::DesignVertexRecipe> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    crate::design::decode::operands::parse_vertex_recipe(
        &ctx, bytes, records, stream, header, recipes,
    )
    .map(|result| result.expect("recipe allocation admitted"))
}

#[test]
fn operand_recipe_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let recipe = ConstructionRecipe {
        id: "f3d:design:recipe#1".into(),
        byte_offset: 0,
        kind: ConstructionRecipeKind::Face,
        design: None,
        recipe_index: 0,
        record_index: None,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::decode::operands::indexed_operand_recipes(
            &ctx, std::slice::from_ref(&recipe),
        ),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d operand recipe index"
    ));
}

#[test]
fn operand_face_candidate_refuses_collection_and_retained_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let face = FaceId::mint("test:model:face#candidate").unwrap();
    for (collection_limit, retained_limit, dimension, operation) in [
        (
            0,
            u64::MAX,
            ResourceDimension::CollectionItems,
            "f3d operand face candidate",
        ),
        (
            1,
            u64::try_from(face.as_str().len() - 1).unwrap(),
            ResourceDimension::RetainedBytes,
            "f3d operand face candidate ID",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = collection_limit;
        policy.limits.max_retained_bytes = retained_limit;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut candidates = Vec::new();
        assert!(matches!(
            crate::design::decode::operands::push_operand_face_candidate(
                &ctx, &mut candidates, &face,
            ),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == dimension && limit.operation == operation
        ));
        assert!(candidates.is_empty());
    }
}

#[test]
fn referenced_operand_faces_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let reference = DesignRecipeReference {
        selector: 1,
        selector_offset: 0,
        token: "3".into(),
        token_offset: 0,
        design_reference: 303,
        design_reference_offset: 0,
        candidate_faces: vec![FaceId::mint("test:model:face#candidate").unwrap()],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::decode::operands::referenced_operand_faces(
            &ctx, std::slice::from_ref(&reference), 303,
        ),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d referenced face candidate"
    ));
}

#[test]
fn surface_patch_long_field_rejected_before_copy() {
    let mut program = vec![0; 7];
    program.extend_from_slice(&[2, 1, 2, 3, -1]);
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::surface_patch_recipe_structure_with_context(
            ctx, &program, 4,
        )
        .expect("recipe structure")
    })
    .is_none());
}

#[test]
fn face_recipe_boundary_accepts_omitted_n_plus_four() {
    let mut ordinary = Vec::new();
    for record_index in 100..=104 {
        indexed_header(&mut ordinary, *b"306", record_index);
    }
    let ordinary_position = ordinary.len() - 11;
    assert_eq!(
        crate::design::decode::operands::face_recipe_next_boundary(
            &ordinary,
            ordinary_position,
            100,
            None,
        ),
        Some((ordinary_position, 104))
    );

    let mut omitted = Vec::new();
    for record_index in 100..=103 {
        indexed_header(&mut omitted, *b"306", record_index);
    }
    let position = omitted.len();
    indexed_header(&mut omitted, *b"124", 0);
    let next = omitted.len();
    indexed_header(&mut omitted, *b"317", 105);
    assert_eq!(
        crate::design::decode::operands::face_recipe_next_boundary(&omitted, position, 100, None),
        Some((next, 105))
    );

    let mut arbitrary = Vec::new();
    for record_index in 100..=103 {
        indexed_header(&mut arbitrary, *b"306", record_index);
    }
    let arbitrary_position = arbitrary.len();
    indexed_header(&mut arbitrary, *b"124", 205);
    assert_eq!(
        crate::design::decode::operands::face_recipe_next_boundary(
            &arbitrary,
            arbitrary_position,
            100,
            None,
        ),
        Some((arbitrary_position, 205))
    );
}

mod nested_records;
