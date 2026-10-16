// SPDX-License-Identifier: Apache-2.0
use super::{
    parameter_record, preceding_incident_angular_dimension_definition, HashMap, ParameterId,
    Point2, Point3, SketchCurveGeometry, SketchCurveIdentity, SketchEntity, SketchEntityId,
    SketchGeometry, SketchGeometryDefinition, SketchId, SketchPoint, Vector3,
};
use cadmpeg_core::decode::ResourceDimension;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let stream = "f3d:A";
    let sketch = SketchId::mint("f3d:model:sketch#angular-incidence").unwrap();
    let curve = |record_index, byte_offset, angle: f64| SketchCurveIdentity {
        id: format!("{stream}:sketch-curve#{record_index}"),
        record_index,
        owner_reference: Some(100),
        class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned()).unwrap(),
        byte_offset,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(u64::from(record_index)).unwrap(),
        secondary_id: 0,
        geometry: Some(
            SketchCurveGeometry::line(
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(angle.cos(), angle.sin(), 0.0),
                Vector3::new(angle.cos(), angle.sin(), 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        ),
    };
    let curves = vec![
        curve(10, 10, 0.0),
        curve(11, 20, 3.0 * std::f64::consts::FRAC_PI_4),
        curve(12, 110, std::f64::consts::FRAC_PI_2),
        curve(13, 120, -std::f64::consts::FRAC_PI_4),
    ];
    let point = |record_index, byte_offset, incident_curves| {
        SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
            id: format!("{stream}:sketch-point#{record_index}"),
            record_index,
            owner_reference: Some(100),
            class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned())
                .unwrap(),
            byte_offset,
            coordinate_offset: 0,
            companion: crate::records::sketch_geometry::SketchPointCompanion { incident_curves },
            record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
                u64::from(record_index),
                crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
                None,
                0.0,
            ),
            paired_reference: 0,
            coordinates: Point2::new(0.0, 0.0),
        })
        .unwrap()
    };
    let points = vec![point(20, 30, vec![10, 11]), point(21, 130, vec![12, 13])];
    let entity = |record_index, angle: f64| {
        SketchEntity::new(
            SketchEntityId::mint(format!("f3d:model:sketch-entity#line-{record_index}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(angle.cos(), angle.sin()),
            })
            .unwrap(),
        )
    };
    let entities = [
        entity(10, 0.0),
        entity(11, 3.0 * std::f64::consts::FRAC_PI_4),
        entity(12, std::f64::consts::FRAC_PI_2),
        entity(13, -std::f64::consts::FRAC_PI_4),
    ];
    let projected = HashMap::from([
        ((stream, 10), &entities[0]),
        ((stream, 11), &entities[1]),
        ((stream, 12), &entities[2]),
        ((stream, 13), &entities[3]),
    ]);
    let parameter = crate::design::decode::parameters::parse_design_parameter(
        &cadmpeg_test_support::service_decode_context(),
        &parameter_record(
            Some(1),
            "135 deg",
            "Angular Dimension-2",
            Some("deg"),
            "d1",
            3.0 * std::f64::consts::FRAC_PI_4,
        ),
    )
    .unwrap()
    .expect("angular parameter")
    .into_record("Design/BulkStream.dat", 100)
    .expect("located parameter");
    let parameter_id =
        ParameterId::mint("synthetic:test:parameter#angle").expect("identity grammar");

    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        preceding_incident_angular_dimension_definition(
            ctx,
            stream,
            &points,
            &curves,
            &projected,
            &sketch,
            (&parameter, &parameter_id),
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn preceding_incident_angular_first_id_refuses_retained_limit() {
    fixture(
        "f3d preceding incident angular first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn preceding_incident_angular_second_id_refuses_retained_limit() {
    fixture(
        "f3d preceding incident angular second id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn preceding_incident_angular_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d preceding incident angular parameter id",
        ResourceDimension::RetainedBytes,
    );
}
