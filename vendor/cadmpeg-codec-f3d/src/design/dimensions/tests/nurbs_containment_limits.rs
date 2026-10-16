// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::SketchGeometry;
fn fixture(operation: &'static str) {
    let geometry = SketchGeometry::nurbs(
        PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point2::new(1.0, 2.0),
                Point2::new(2.0, 4.0),
                Point2::new(3.0, 2.0),
            ],
            Some(vec![1.0, 0.5, 1.0]),
            false,
        )
        .expect("fixture pcurve construction admission")
        .unwrap(),
    );
    super::assert_dimension_refusal(operation, ResourceDimension::CollectionItems, |ctx| {
        crate::design::dimensions::point_lies_on_sketch_geometry(
            ctx,
            Point2::new(3.0, 2.0),
            &geometry,
        )
        .map(|_| ())
    });
}
#[test]
fn dimension_nurbs_containment_poles_refuse_collection_limit() {
    fixture("f3d nurbs evaluator poles");
}
#[test]
fn dimension_nurbs_containment_weights_refuse_collection_limit() {
    fixture("f3d nurbs evaluator weights");
}
