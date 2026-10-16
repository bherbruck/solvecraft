// SPDX-License-Identifier: Apache-2.0
//! Linear historical radius replacement and ordered coedge incidence.
use crate::history::historical_topology;
use cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload;
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CacheContract, ProceduralSurface,
    ProceduralSurfaceDefinition,
};
use cadmpeg_ir::ids::{CoedgeId, EdgeId, FaceId, LoopId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::topology::{Coedge, Loop, LoopBoundary, LoopRing, Sense};

#[test]
fn historical_radii_replace_each_carrier_once_and_keep_last_blend() {
    let mut brep = cadmpeg_asm::brep::AsmBrep::default();
    let blend = |slot, radius| {
        let owner = SurfaceId::mint(format!("test:brep:surface#{slot}")).unwrap();
        let payload = BlendSurfacePayload::try_new(
            [None, None],
            None,
            BlendRadiusLaw::constant(radius).unwrap(),
            BlendCrossSection::Circular,
            CacheContract::legacy(),
        )
        .unwrap();
        (
            owner,
            ProceduralSurface::new(
                ProceduralSurfaceId::mint(format!("test:brep:blend#{slot}")).unwrap(),
                ProceduralSurfaceDefinition::Blend(payload),
                None,
            ),
        )
    };
    for slot in 0..1_000 {
        brep.procedural_surfaces.push(blend(slot, -2.0));
    }
    brep.procedural_surfaces.push(blend(0, 3.0));
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    // Admit one 1,000-row sort and linear carrier scans, including the sort's byte cost.
    policy.limits.max_work_units = 2_000_000;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let topology = historical_topology(&ctx, &brep).unwrap().unwrap();
    assert_eq!(topology.surface_radii.len(), 1_000);
    assert_eq!(topology.surface_radii[0].surface, 0);
    assert_eq!(topology.surface_radii[0].radius, 3.0);
    assert!(topology.surface_radii[1..]
        .iter()
        .all(|radius| radius.radius == 2.0));
    ctx.finish_session().unwrap();
}

#[test]
fn historical_coedge_neighbors_follow_ring_order() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut brep = cadmpeg_asm::brep::AsmBrep::default();
    let owner = LoopId::mint("test:brep:loop#1").unwrap();
    let ids: Vec<_> = (10..110)
        .map(|slot| CoedgeId::mint(format!("test:brep:coedge#{slot}")).unwrap())
        .collect();
    brep.loops.push(Loop {
        id: owner.clone(),
        face: FaceId::mint("test:brep:face#2").unwrap(),
        boundary: LoopBoundary::Ring(
            LoopRing::new(&ctx, ids.clone(), Vec::new())
                .unwrap()
                .unwrap(),
        ),
    });
    for id in &ids {
        brep.coedges.push(Coedge {
            id: id.clone(),
            owner_loop: owner.clone(),
            edge: EdgeId::mint("test:brep:edge#3").unwrap(),
            radial_next: id.clone(),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
        });
    }
    let topology = historical_topology(&ctx, &brep).unwrap().unwrap();
    assert_eq!(topology.coedge_topology.len(), 100);
    for (ordinal, coedge) in topology.coedge_topology.iter().enumerate() {
        let ordinal = i64::try_from(ordinal).unwrap();
        assert_eq!(coedge.next, 10 + (ordinal + 1) % 100);
        assert_eq!(coedge.previous, 10 + (ordinal + 99) % 100);
    }
}
