// SPDX-License-Identifier: Apache-2.0
//! Topology cache identity and carrier lookup resource scaling.

use std::fmt::Write;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::document::CadIr;

#[test]
fn cached_void_roots_preserve_the_outer_shell_role() {
    let source = b"ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CLOSED_SHELL('',());#2=CLOSED_SHELL('',());#3=CLOSED_SHELL('',());#10=BREP_WITH_VOIDS('',#1,(#2,#3));#11=BREP_WITH_VOIDS('',#1,(#3,#2));#12=BREP_WITH_VOIDS('',#2,(#1,#3));ENDSEC;END-ISO-10303-21;";
    crate::test_support::with_service_context(source, |source, ctx| {
        let (exchange, _) = crate::parse::parse_inner(source, ctx).expect("authored roots");
        let shells = (1..=3)
            .map(|id| {
                (
                    id,
                    super::super::ShellDef {
                        base: id,
                        forward: true,
                        typed: std::collections::HashSet::new(),
                    },
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let key = |id| {
            super::super::root_key(
                exchange.records().get(&id).expect("authored root"),
                &exchange,
                &shells,
                ctx,
            )
            .expect("bounded key")
            .expect("resolved shells")
        };
        assert_eq!(key(10), key(11));
        assert_ne!(key(10), key(12));
    });
}

#[test]
fn pcurve_index_is_shared_across_shells_and_coedges() {
    let mut shells = String::new();
    let mut records = String::new();
    for id in 1_000..1_100 {
        if !shells.is_empty() {
            shells.push(',');
        }
        write!(shells, "#{id}").expect("write fixture shell");
        writeln!(records, "#{id}=OPEN_SHELL('',(#54));").expect("write fixture shell");
    }
    for id in 10_000..11_000 {
        writeln!(records, "#{id}=CARTESIAN_POINT('',(0.,0.,0.));").expect("write fixture point");
    }
    let source = include_str!("data/tp12_two_admissions.p21")
        .replace(
            "#56=SHELL_BASED_SURFACE_MODEL('',(#55));",
            &format!("#56=SHELL_BASED_SURFACE_MODEL('',({shells}));"),
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            &format!("{records}ENDSEC;\nEND-ISO-10303-21;"),
        );
    let mut ir = CadIr::empty();
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), |bytes, ctx| {
            let (exchange, diagnostics) =
                crate::parse::parse_inner(bytes, ctx).expect("authored exchange parses");
            crate::reader::geometry::decode(&exchange, &mut ir, ctx)
                .expect("authored geometry fits the preparation policy");
            (exchange, diagnostics)
        });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One shared carrier index and all 100 drafts fit; 200 complete model
    // indexes would consume more than this allowance.
    policy.limits.max_collection_items = 100_000;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let carriers =
        crate::reader::index::CarrierIndex::from_ir(&ir, &ctx).expect("carrier lookup fits policy");
    super::super::decode(&exchange, &mut ir, &carriers, &ctx)
        .expect("all shells share the admitted pcurve index");
    assert_eq!(ir.model.bodies.len(), 100);
    assert_eq!(ir.model.faces.len(), 100);
    assert_eq!(ir.model.coedges.len(), 400);
    assert_eq!(
        ir.model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        200
    );
}

#[test]
fn pcurve_index_is_shared_across_independent_roots() {
    let mut records = String::new();
    for id in 1_000..1_100 {
        writeln!(records, "#{id}=OPEN_SHELL('',(#54));").expect("write fixture shell");
        writeln!(
            records,
            "#{}=SHELL_BASED_SURFACE_MODEL('',(#{id}));",
            id + 1_000
        )
        .expect("write fixture root");
    }
    for id in 10_000..11_000 {
        writeln!(records, "#{id}=CARTESIAN_POINT('',(0.,0.,0.));").expect("write fixture point");
    }
    let source = include_str!("data/tp12_two_admissions.p21")
        .replace("#56=SHELL_BASED_SURFACE_MODEL('',(#55));", "")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            &format!("{records}ENDSEC;\nEND-ISO-10303-21;"),
        )
        .replace("(#56)", "(#2000)");
    let mut ir = CadIr::empty();
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), |bytes, ctx| {
            let (exchange, diagnostics) =
                crate::parse::parse_inner(bytes, ctx).expect("authored exchange parses");
            crate::reader::geometry::decode(&exchange, &mut ir, ctx)
                .expect("authored geometry fits the preparation policy");
            (exchange, diagnostics)
        });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The same carrier population and 100 topology drafts fit this bound.
    // A complete identity index per root exceeds it before the roots finish.
    policy.limits.max_collection_items = 100_000;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let carriers =
        crate::reader::index::CarrierIndex::from_ir(&ir, &ctx).expect("carrier lookup fits policy");
    super::super::decode(&exchange, &mut ir, &carriers, &ctx)
        .expect("all independent roots share the admitted pcurve index");
    assert_eq!(ir.model.bodies.len(), 100);
    assert_eq!(ir.model.faces.len(), 100);
    assert_eq!(ir.model.coedges.len(), 400);
    assert_eq!(
        ir.model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        200
    );
}

#[test]
fn source_identity_groups_keep_singletons_inline_and_shared_order() {
    use cadmpeg_core::{decode::ResourceDimension, CodecError};
    use std::collections::BTreeMap;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1_000;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let mut groups = BTreeMap::new();
    for source in 0..1_000 {
        super::super::push_source_identity(
            &mut groups,
            source,
            source,
            "step_topology_source_edge_groups",
            "step_topology_source_edges",
            &ctx,
        )
        .expect("each singleton has one map entry and no member vector");
    }
    assert_eq!(groups.len(), 1_000);
    let error = super::super::push_source_identity(
        &mut groups,
        0,
        2_000,
        "step_topology_source_edge_groups",
        "step_topology_source_edges",
        &ctx,
    )
    .expect_err("an additional identity needs a member slot");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_topology_source_edges"));
    assert_eq!(groups[&0].iter().copied().collect::<Vec<_>>(), [0]);

    let arena = DecodeArena::new();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let mut groups = BTreeMap::new();
    for identity in [11, 22, 33] {
        super::super::push_source_identity(
            &mut groups,
            7,
            identity,
            "step_topology_source_edge_groups",
            "step_topology_source_edges",
            &ctx,
        )
        .expect("one map entry and two additional members");
    }
    assert_eq!(groups[&7].iter().copied().collect::<Vec<_>>(), [11, 22, 33]);
}

#[test]
fn terminal_edges_own_one_definition_map_without_cycle_nodes() {
    let records = (100..1100).fold(String::new(), |mut records, id| {
        write!(records, "#{id}=EDGE('',#1,#2);").expect("fixture string");
        records
    });
    let source = format!("ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DUMMY();#2=DUMMY();{records}ENDSEC;END-ISO-10303-21;");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("bounded edges");
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1000;
    crate::test_support::with_policy_context(source.as_bytes(), &policy, |_, ctx| {
        let edges = super::super::edge_defs(&exchange, ctx).expect("one slot per terminal edge");
        assert_eq!(edges.len(), 1000);
        assert!(edges.values().all(|edge| matches!(
            edge.as_ref(),
            super::super::EdgeDef::Bare { start: 1, end: 2 }
        )));
    });
}

#[test]
fn coedges_need_only_their_element_definition_until_used_as_wire_edges() {
    let records = (1000..2000).fold(String::new(), |mut records, id| {
        write!(records, "#{id}=ORIENTED_EDGE('',*,*,#3,.F.);").expect("fixture string");
        records
    });
    let base = format!("ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DUMMY();#2=DUMMY();#3=EDGE('',#1,#2);{records}");
    for (wire, cap, keys) in [
        ("", 1, vec![3]),
        ("#3000=CONNECTED_EDGE_SET('',(#1000));", 3, vec![3, 1000]),
    ] {
        let source = format!("{base}{wire}ENDSEC;END-ISO-10303-21;");
        let (exchange, _) =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect("bounded coedges");
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        crate::test_support::with_policy_context(source.as_bytes(), &policy, |_, ctx| {
            let edges = super::super::edge_defs(&exchange, ctx)
                .expect("only referenced edge definitions allocate");
            assert_eq!(edges.keys().copied().collect::<Vec<_>>(), keys);
            assert_eq!(edges[&3].vertices(), (1, 2));
            if !wire.is_empty() {
                assert_eq!(edges[&1000].curve_vertices(), (2, 1));
            }
        });
    }
}
