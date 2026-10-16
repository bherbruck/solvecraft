// SPDX-License-Identifier: Apache-2.0
//! Collection refusals in the STEP topology reader.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn shape_relationship_graph_preserves_bidirectional_edges() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=SHAPE_REPRESENTATION('',(),$);#2=SHAPE_REPRESENTATION('',(),$);#3=SHAPE_REPRESENTATION_RELATIONSHIP('','',#1,#2);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid relationship graph");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    let graph = super::super::shape_representation_relationships(&exchange, &ctx)
        .expect("relationship graph fits policy");
    assert_eq!(graph.get(&1).map(Vec::as_slice), Some(&[2][..]));
    assert_eq!(graph.get(&2).map(Vec::as_slice), Some(&[1][..]));
}

#[test]
fn topology_commit_error_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let error = cadmpeg_ir::draft::DraftError::IdentityCollision("step:data:body#1".into());
    assert!(matches!(
        super::super::topology_commit_error("topology root", &error, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_topology_commit_error_text"
    ));
}

#[test]
fn associated_pcurves_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=SURFACE_CURVE('',#4,(#2),.PCURVE_S1.);#2=PCURVE('',#3,#5);#3=DUMMY();#4=DUMMY();#5=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid surface curve references");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    assert!(matches!(
        super::super::associated_pcurves(1, 3, &exchange, &std::collections::BTreeSet::from([2]), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_associated_pcurves"
    ));
}

fn body_id() -> cadmpeg_ir::ids::BodyId {
    cadmpeg_ir::ids::BodyId::mint("step:data:body#1").expect("valid body identity")
}

#[test]
fn topology_body_id_copy_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        body_id().try_clone_for_decode(&ctx, "step_topology_root_bodies"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_topology_root_bodies"
    ));
}

#[test]
fn topology_root_group_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        super::super::push_topology_body_group(&mut BTreeMap::new(), 1, &body_id(), &ctx, "step_topology_root_groups", "step_topology_root_bodies"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_root_groups"
    ));
}

#[test]
fn topology_root_bodies_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        super::super::push_topology_body_group(&mut BTreeMap::new(), 1, &body_id(), &ctx, "step_topology_root_groups", "step_topology_root_bodies"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_root_bodies"
    ));
}

#[test]
fn topology_shell_group_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        super::super::insert_topology_body_group(&mut BTreeMap::new(), 1, &body_id(), &ctx, "step_topology_shell_groups", "step_topology_shell_bodies"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_shell_groups"
    ));
}

#[test]
fn topology_shell_bodies_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        super::super::insert_topology_body_group(&mut BTreeMap::new(), 1, &body_id(), &ctx, "step_topology_shell_groups", "step_topology_shell_bodies"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_shell_bodies"
    ));
}

#[test]
fn geometric_set_omissions_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#2),#3);#2=GEOMETRIC_SET('',(#4));#3=DUMMY();#4=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid geometric set references");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&cadmpeg_ir::CadIr::empty(), &ctx)
        .expect("empty carrier index fits policy");
    assert!(matches!(
        super::super::geometric_set_omissions(exchange.records().get(&1).expect("representation"), &exchange, &carriers, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_geometric_set_omissions"
    ));
}

#[test]
fn geometric_set_omission_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        super::super::geometric_set_omission_message("GEOMETRIC_SET", 1, &[2, 3], &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_geometric_set_omission_text"
    ));
}

#[test]
fn built_outcome_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let mut outcome = super::super::BuildOutcome::Built(Vec::new());
    let built = super::super::Built {
        typed: std::collections::HashSet::new(),
        draft: cadmpeg_ir::draft::ModelDraft::new(),
        body_id: body_id(),
        shell_sources: std::collections::BTreeSet::new(),
        pcurve_admissions: Vec::new(),
    };
    assert!(matches!(
        outcome.push(built, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_built_outcome"
    ));
}

#[test]
fn connected_wire_typed_claims_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CONNECTED_EDGE_SET('',(#2));#2=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid connected set");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&cadmpeg_ir::CadIr::empty(), &ctx)
        .expect("empty carrier index fits policy");
    assert!(matches!(
        super::super::build_wire_set(
            3, 1, &exchange,
            super::super::WireSources { vdefs: &BTreeMap::new(), edefs: &BTreeMap::new(), point_positions: &carriers },
            false,
            &mut Vec::new(), &ctx,
        ),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_wire_typed"
    ));
}

#[test]
fn shell_wire_typed_claims_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=WIRE_SHELL('',(#2));#2=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid wire shell");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&cadmpeg_ir::CadIr::empty(), &ctx)
        .expect("empty carrier index fits policy");
    assert!(matches!(
        super::super::build_shell_wire_set(
            3, 1, &exchange,
            super::super::WireSources { vdefs: &BTreeMap::new(), edefs: &BTreeMap::new(), point_positions: &carriers },
            super::super::WireScope { scoped: false, root: false },
            &mut Vec::new(), &ctx,
        ),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_wire_typed"
    ));
}

#[test]
fn subset_parent_loss_refuses_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CONNECTED_EDGE_SUB_SET('',(),$);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid subset syntax");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    assert!(matches!(
        super::super::validate_subset_parent(1, exchange.records().get(&1).expect("subset"), "CONNECTED_EDGE_SUB_SET", &exchange, &mut Vec::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_losses"
    ));
}

#[test]
fn curve_less_wire_edge_loss_refuses_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid empty exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    let edge = super::super::EdgeDef::Bare { start: 1, end: 2 };
    assert!(matches!(
        super::super::edge_curve_id_reported(1, &edge, &exchange, &mut Vec::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_topology_losses"
    ));
}

#[test]
fn vertex_definitions_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=VERTEX_POINT('',#2);#2=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid vertex reference");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    assert!(matches!(
        super::super::vertex_defs(&exchange, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_vertex_definitions"
    ));
}

#[test]
fn oriented_edge_lookup_needs_no_collection_storage() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ORIENTED_EDGE('',*,*,#2,.T.);#2=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid oriented edge reference");
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    crate::test_support::with_policy_context(source, &policy, |_, _| {
        for _ in 0..1000 {
            let edge =
                super::super::oriented_def(&exchange.records()[&1]).expect("source orientation");
            assert_eq!(edge.edge, 2);
            assert!(edge.forward);
            assert!(matches!(edge.kind, super::super::OrientedKind::Plain));
        }
    });
}

fn edge_definition_refusal(
    collection_limit: u64,
    retained_limit: u64,
    depth_limit: u64,
) -> CodecError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DUMMY();#2=DUMMY();#3=EDGE('',#1,#2);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid edge reference");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    super::super::edge_defs(&exchange, &ctx)
        .err()
        .expect("edge definitions exceed limit")
}

#[test]
fn edge_definition_active_set_refuses_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DUMMY();#2=DUMMY();#3=SUBEDGE('',#1,#2,#4);#4=EDGE('',#1,#2);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("recursive edge fixture");
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    crate::test_support::with_policy_context(source, &policy, |_, ctx| {
        assert!(matches!(super::super::edge_defs(&exchange, ctx),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "step_edge_definition_active"));
    });
}

#[test]
fn edge_definition_rejections_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=EDGE('',$,$);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("unresolved edge fixture");
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    crate::test_support::with_policy_context(source, &policy, |_, ctx| {
        assert!(matches!(super::super::edge_defs(&exchange, ctx),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "step_edge_definition_rejections"));
    });
}

#[test]
fn edge_definitions_refuse_collection_limit() {
    assert!(matches!(edge_definition_refusal(0, u64::MAX, u64::MAX),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_edge_definitions"));
}

#[test]
fn edge_definition_node_refuses_retained_limit() {
    assert!(
        matches!(cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::RetainedBytes, "step_edge_definition_node", |cap| Err::<(), CodecError>(edge_definition_refusal(u64::MAX, cap, u64::MAX))),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_edge_definition_node")
    );
}

#[test]
fn edge_definition_recursion_refuses_depth_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DUMMY();#2=DUMMY();#3=SUBEDGE('',#1,#2,#4);#4=EDGE('',#1,#2);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid subedge reference");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    assert!(matches!(super::super::edge_defs(&exchange, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_edge_definition_recursion"));
}

fn shell_definition_refusal(collection_limit: u64) -> CodecError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=OPEN_SHELL('',());ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid shell reference");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    super::super::shell_defs(&exchange, &ctx)
        .err()
        .expect("shell definitions exceed limit")
}

#[test]
fn shell_definition_active_set_refuses_collection_limit() {
    assert!(matches!(shell_definition_refusal(0),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_shell_definition_active"));
}

#[test]
fn shell_definition_cache_refuses_collection_limit() {
    assert!(matches!(shell_definition_refusal(1),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_shell_definition_cache"));
}

#[test]
fn shell_definitions_refuse_collection_limit() {
    assert!(matches!(shell_definition_refusal(2),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_shell_definitions"));
}

#[test]
fn shell_definition_typed_copy_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let definition = super::super::ShellDef {
        base: 1,
        forward: true,
        typed: std::collections::HashSet::from([2]),
    };
    assert!(matches!(super::super::copy_shell_def(&definition, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_shell_definition_typed_copy"));
}

#[test]
fn shell_definition_claims_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let definition = super::super::ShellDef {
        base: 1,
        forward: true,
        typed: std::collections::HashSet::from([2]),
    };
    let shells = BTreeMap::from([(1, definition)]);
    assert!(
        matches!(super::super::shell_def_for(1, &shells, &mut std::collections::HashSet::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_shell_definition_claims")
    );
}

#[test]
fn shell_definition_recursion_refuses_depth_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=OPEN_SHELL('',());#2=ORIENTED_OPEN_SHELL('',*,#1,.T.);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid oriented shell reference");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    assert!(
        matches!(super::super::shell_def_cached(2, &exchange, &mut std::collections::BTreeSet::new(), &mut BTreeMap::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_shell_definition_recursion")
    );
}

fn topology_root_refusal(collection_limit: u64, include_distinct: bool) -> CodecError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=OPEN_SHELL('',());#2=SHELL_BASED_SURFACE_MODEL('',(#1));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid shell model reference");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    let root = exchange.records().get(&2).expect("shell model");
    let shells = BTreeMap::from([(
        1,
        super::super::ShellDef {
            base: 1,
            forward: true,
            typed: std::collections::HashSet::new(),
        },
    )]);
    let key = super::super::root_key(root, &exchange, &shells, &ctx)
        .and_then(|key| key.ok_or_else(|| CodecError::malformed("missing root key")));
    if !include_distinct {
        return key.expect_err("root key exceeds limit");
    }
    let mut distinct = std::collections::BTreeSet::new();
    ctx.insert_btree_set(
        &mut distinct,
        key.expect("root key fits limit"),
        "step_distinct_topology_roots",
    )
    .map(|_| ())
    .expect_err("distinct root set exceeds limit")
}

#[test]
fn topology_root_shell_steps_refuse_collection_limit() {
    assert!(matches!(topology_root_refusal(0, false),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_root_shell_steps"));
}

#[test]
fn topology_root_shell_keys_refuse_collection_limit() {
    assert!(matches!(topology_root_refusal(1, false),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_root_shell_keys"));
}

#[test]
fn topology_distinct_roots_refuse_collection_limit() {
    assert!(matches!(topology_root_refusal(2, true),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_distinct_topology_roots"));
}

fn geometric_set_refusal(collection_limit: u64, has_surface: bool) -> CodecError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#2),#3);#2=GEOMETRIC_SET('',(#4));#3=DUMMY();#4=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid geometric set references");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    let mut carriers =
        crate::reader::index::CarrierIndex::from_ir(&cadmpeg_ir::CadIr::empty(), &ctx)
            .expect("empty carrier index fits policy");
    if has_surface {
        carriers
            .surfaces
            .insert(4, crate::reader::index::SurfaceIndex(0));
    }
    super::super::build_geometric_set(
        1,
        exchange.records().get(&1).expect("representation"),
        &exchange,
        &carriers,
        &mut Vec::new(),
        &ctx,
    )
    .err()
    .expect("geometric set exceeds collection limit")
}

#[test]
fn geometric_set_typed_refuses_collection_limit() {
    assert!(matches!(geometric_set_refusal(0, false),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_geometric_set_typed"));
}

#[test]
fn geometric_set_shell_faces_refuse_collection_limit() {
    assert!(matches!(geometric_set_refusal(2, true),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_geometric_set_shell_faces"));
}

#[test]
fn geometric_set_faces_refuse_collection_limit() {
    assert!(matches!(geometric_set_refusal(3, true),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_geometric_set_faces"));
}

fn staged_topology_refusal(
    collection_limit: u64,
    retained_limit: u64,
    surface_count: usize,
) -> super::super::StageError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let body_id = body_id();
    let region_id =
        cadmpeg_ir::ids::RegionId::mint("step:data:region#1").expect("valid region identity");
    let surfaces = (0..surface_count)
        .map(|index| cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint(format!("step:data:surface#{index}"))
                .expect("valid surface identity"),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown { record: None },
            ),
            source_object: None,
        })
        .collect();
    super::super::staged_topology(
        super::super::StagedTopologyParts {
            typed: std::collections::HashSet::new(),
            vertices: Vec::new(),
            edges: Vec::new(),
            coedges: Vec::new(),
            loops: Vec::new(),
            faces: Vec::new(),
            surfaces,
            shells: Vec::new(),
            region: cadmpeg_ir::topology::Region {
                id: region_id.clone(),
                body: body_id.clone(),
                shells: Vec::new(),
            },
            body: cadmpeg_ir::topology::Body {
                id: body_id,
                kind: cadmpeg_ir::topology::BodyKind::Sheet,
                regions: vec![region_id],
                transform: None,
                name: None,
                color: None,
                visible: None,
            },
        },
        &ctx,
    )
    .err()
    .expect("staging exceeds limit")
}

#[test]
fn staged_surface_ids_refuse_collection_limit() {
    assert!(matches!(staged_topology_refusal(0, u64::MAX, 1),
        super::super::StageError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_staged_surface_ids"));
}

#[test]
fn staged_surface_ids_refuse_retained_limit() {
    assert!(matches!(staged_topology_refusal(u64::MAX, 0, 1),
        super::super::StageError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_staged_surface_ids"));
}

#[test]
fn staged_surfaces_refuse_collection_limit() {
    assert!(matches!(staged_topology_refusal(1, u64::MAX, 1),
        super::super::StageError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "draft entity arena"));
}

#[test]
fn staged_regions_refuse_collection_limit() {
    assert!(matches!(staged_topology_refusal(0, u64::MAX, 0),
        super::super::StageError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "draft entity arena"));
}

#[test]
fn staged_bodies_refuse_collection_limit() {
    assert!(matches!(staged_topology_refusal(3, u64::MAX, 0),
        super::super::StageError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "draft entity arena"));
}

fn brep_builder_refusal(collection_limit: u64) -> super::super::BuildError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=OPEN_SHELL('',(#2));#2=FACE('',());#3=SHELL_BASED_SURFACE_MODEL('',(#1));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid shell model references");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&cadmpeg_ir::CadIr::empty(), &ctx)
        .expect("empty carrier index fits policy");
    let shells = BTreeMap::from([(
        1,
        super::super::ShellDef {
            base: 1,
            forward: true,
            typed: std::collections::HashSet::new(),
        },
    )]);
    let region =
        cadmpeg_ir::ids::RegionId::mint("step:data:region#3").expect("valid region identity");
    super::super::build_one(
        3,
        exchange.records().get(&3).expect("model"),
        super::super::BuildSources {
            exchange: &exchange,
            index: None,
            vdefs: &BTreeMap::new(),
            edefs: &BTreeMap::new(),
            shell_definitions: &shells,
            decoded_pcurves: &std::collections::BTreeSet::new(),
            point_positions: &carriers,
            seed_cache: &std::cell::RefCell::new(super::super::PcurveSeedCache::default()),
            ctx: &ctx,
        },
        super::super::BuildRoot {
            shell_steps: &[1],
            bid: body_id(),
            rid: &region,
        },
        super::super::BuildScope {
            faces: false,
            edges: false,
            root: false,
        },
        &mut Vec::new(),
        &mut None,
    )
    .err()
    .expect("builder exceeds limit")
}

#[test]
fn brep_typed_refuses_collection_limit() {
    assert!(matches!(brep_builder_refusal(0),
        super::super::BuildError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_brep_typed"));
}

#[test]
fn brep_body_regions_refuse_collection_limit() {
    assert!(matches!(brep_builder_refusal(1),
        super::super::BuildError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_brep_body_regions"));
}

#[test]
fn brep_used_shells_refuse_collection_limit() {
    assert!(matches!(brep_builder_refusal(2),
        super::super::BuildError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_brep_used_shells"));
}

#[test]
fn brep_used_faces_refuse_collection_limit() {
    assert!(matches!(brep_builder_refusal(3),
        super::super::BuildError::Resource(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_brep_used_faces"));
}

fn face_attribute_refusal(collection_limit: u64, depth_limit: u64, face_id: u64) -> CodecError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=FACE('',(#2));#2=FACE_BOUND('',#3,.T.);#3=EDGE_LOOP('',());#4=ORIENTED_FACE('',*,#1,.T.);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid face references");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    super::super::face_attributes(
        face_id,
        exchange.records().get(&face_id).expect("face record"),
        &exchange,
        &mut std::collections::BTreeSet::new(),
        &ctx,
    )
    .err()
    .expect("face attributes exceed limit")
}

#[test]
fn face_attribute_active_refuses_collection_limit() {
    assert!(matches!(face_attribute_refusal(0, u64::MAX, 1),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_face_attribute_active"));
}

#[test]
fn face_attribute_bounds_refuse_collection_limit() {
    assert!(matches!(face_attribute_refusal(1, u64::MAX, 1),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_face_attribute_bounds"));
}

#[test]
fn face_attribute_typed_refuses_collection_limit() {
    assert!(matches!(face_attribute_refusal(3, u64::MAX, 4),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_face_attribute_typed"));
}

#[test]
fn face_attribute_recursion_refuses_depth_limit() {
    assert!(matches!(face_attribute_refusal(u64::MAX, 1, 4),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_face_attribute_recursion"));
}

fn implicit_face_refusal(collection_limit: u64, plane: bool) -> CodecError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=FACE_BOUND('',#2,.T.);#2=POLY_LOOP('',(#3,#4,#5));#3=DUMMY();#4=DUMMY();#5=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid polygon loop references");
    let mut ir = cadmpeg_ir::CadIr::empty();
    for (id, position) in [
        (3, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)),
        (4, cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0)),
        (5, cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0)),
    ] {
        ir.model.points.push(cadmpeg_ir::topology::Point::new(
            cadmpeg_ir::ids::PointId::from(crate::ids::data(crate::ids::kind!("point"), id)),
            cadmpeg_ir::features::FinitePoint3::new(position).expect("finite point"),
            None,
        ));
    }
    let setup_arena = DecodeArena::new();
    let setup_policy = DecodePolicy::service();
    let (setup_ctx, _) = DecodeContext::from_root_bytes(source, &setup_arena, &setup_policy)
        .expect("source fits setup policy");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup_ctx)
        .expect("point carriers fit setup policy");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    let result = if plane {
        super::super::implicit_face_plane(&[1], &exchange, &BTreeMap::new(), &carriers, &ctx)
            .map(|_| ())
    } else {
        super::super::implicit_face_points(&[1], &exchange, &BTreeMap::new(), &carriers, &ctx)
            .map(|_| ())
    };
    result.expect_err("implicit face exceeds collection limit")
}

#[test]
fn implicit_face_point_steps_refuse_collection_limit() {
    assert!(matches!(implicit_face_refusal(0, false),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_implicit_face_point_steps"));
}

#[test]
fn implicit_face_distinct_points_refuse_collection_limit() {
    assert!(matches!(implicit_face_refusal(3, false),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_implicit_face_distinct_points"));
}

#[test]
fn implicit_face_points_refuse_collection_limit() {
    assert!(matches!(implicit_face_refusal(6, false),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_implicit_face_points"));
}

#[test]
fn implicit_face_loops_refuse_collection_limit() {
    assert!(matches!(implicit_face_refusal(9, false),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_implicit_face_loops"));
}

#[test]
fn implicit_face_plane_points_refuse_collection_limit() {
    assert!(matches!(implicit_face_refusal(10, true),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_implicit_face_plane_points"));
}

#[test]
fn implicit_face_loop_normals_refuse_collection_limit() {
    assert!(matches!(implicit_face_refusal(13, true),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_implicit_face_loop_normals"));
}

fn pcurve_seed_refusal(collection_limit: u64, break_only: bool) -> CodecError {
    let ir = cadmpeg_ir::CadIr::empty();
    let index =
        cadmpeg_ir::index::ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let surface_id =
        cadmpeg_ir::ids::SurfaceId::mint("test:audit:surface#1").expect("valid surface identity");
    let surface = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown { record: None },
    );
    let pcurve = cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs {
        nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 0.5, 1.0, 1.0],
            vec![
                cadmpeg_ir::math::Point2::new(0.0, 0.0),
                cadmpeg_ir::math::Point2::new(0.5, 0.0),
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
            ],
            None,
            false,
        )
        .expect("fixture pcurve construction admission")
        .expect("finite pcurve"),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    if break_only {
        super::super::pcurve_parameter_break_fractions(&pcurve, [0.0, 1.0], &mut Vec::new(), &ctx)
            .expect_err("break fractions exceed limit")
    } else {
        super::super::pcurve_selection_seeds(&index, &surface_id, &pcurve, &surface, &ctx)
            .expect_err("selection seeds exceed limit")
    }
}

#[test]
fn pcurve_break_fractions_refuse_collection_limit() {
    assert!(matches!(pcurve_seed_refusal(0, true),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_pcurve_break_fractions"));
}

#[test]
fn pcurve_selection_seeds_refuse_collection_limit() {
    assert!(matches!(pcurve_seed_refusal(0, false),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_pcurve_selection_seeds"));
}

#[test]
fn pcurve_selection_fractions_refuse_collection_limit() {
    assert!(matches!(pcurve_seed_refusal(69, false),
        CodecError::ResourceLimit(refusal)
            if refusal.operation == "step_pcurve_selection_fractions"));
}

#[test]
fn selected_pcurve_id_refuses_retained_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid empty exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&cadmpeg_ir::CadIr::empty(), &ctx)
        .expect("empty carrier index fits policy");
    let candidate =
        cadmpeg_ir::ids::PcurveId::mint("step:data:pcurve#1").expect("valid pcurve identity");
    let ir = cadmpeg_ir::CadIr::empty();
    let index =
        cadmpeg_ir::index::ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    assert!(matches!(super::super::select_associated_pcurve(
        &index, &exchange, 1,
        &super::super::EdgeDef::Bare { start: 1, end: 2 },
        super::super::PcurveAssociationSources {
            vdefs: &BTreeMap::new(), point_positions: &carriers, candidates: &[candidate],
            seed_cache: &std::cell::RefCell::new(super::super::PcurveSeedCache::default()),
        }, &ctx,
    ), Err(super::super::PcurveSelectionFailure::Resource(CodecError::ResourceLimit(refusal)))
        if refusal.dimension == ResourceDimension::RetainedBytes
            && refusal.operation == "step_selected_pcurve_id"));
}
