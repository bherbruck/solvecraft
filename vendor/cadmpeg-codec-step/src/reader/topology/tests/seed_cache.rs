// SPDX-License-Identifier: Apache-2.0
//! Pcurve seed ownership across repeated endpoint searches.

use std::cell::RefCell;
use std::rc::Rc;

use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::{PcurveId, SurfaceId};
use cadmpeg_ir::math::Point2;

#[test]
fn repeated_pcurve_searches_share_one_admitted_seed_vector() {
    let geometry = crate::test_support::with_service_context(b"", |_, ctx| {
        PcurveNurbs::from_lanes(
            ctx,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)],
            None,
            false,
        )
    })
    .expect("fixture resource admission")
    .expect("finite pcurve");
    let geometry = PcurveGeometry::Nurbs { nurbs: geometry };
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    let surface_id = SurfaceId::mint("test:seeds:surface#1").expect("surface ID");
    let pcurve_id = PcurveId::mint("test:seeds:pcurve#1").expect("pcurve ID");
    let ir = cadmpeg_ir::CadIr::empty();
    let index =
        cadmpeg_ir::index::ModelIndex::new_model_only(&ir, cadmpeg_ir::index::StandardIndex);
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 128;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let cache = RefCell::new(super::super::PcurveSeedCache::default());
        let seeds = super::super::cached_pcurve_selection_seeds(
            &index,
            &surface_id,
            &pcurve_id,
            &geometry,
            &surface,
            &cache,
            ctx,
        )
        .expect("first admitted vector");
        assert_eq!(&seeds[..3], [0.0, 0.5, 1.0]);
        for _ in 0..1000 {
            let repeated = super::super::cached_pcurve_selection_seeds(
                &index,
                &surface_id,
                &pcurve_id,
                &geometry,
                &surface,
                &cache,
                ctx,
            )
            .expect("seed ownership is shared");
            assert!(Rc::ptr_eq(&seeds, &repeated));
        }
    });
}
