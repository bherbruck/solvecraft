// SPDX-License-Identifier: Apache-2.0
//! Unrepresented-content accounting charges every check it owns.

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::{StepCodec, StepSchema, StepWriteOptions};

/// Charging dropped PMI does not end the accounting: content checked after
/// PMI on a target without semantic PMI is still charged.
#[test]
fn pmi_dropped_by_schema_does_not_skip_later_losses() {
    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../tests/fixtures/ap242_semantic_pmi.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode semantic PMI")
        .into_parts()
        .0;
    ir.native.namespace_mut("f3d").arenas_mut().insert(
        "asm_histories".into(),
        vec![cadmpeg_ir::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new("f3d:test:asm-history#0").expect("valid identity"),
            serde_json::Map::default(),
        )
        .expect("valid native identity")],
    );

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report-mode STEP write");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::PmiAnnotationNotWritten.kind()));
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.message.contains("source-native record(s)")),
        "{:?}",
        report.losses
    );
}

fn product(ir: &mut cadmpeg_ir::CadIr) {
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            id: "test:model:product-definition#empty".try_into().unwrap(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: None,
            label: None,
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::default(),
            bodies: Vec::new(),
            native_ref: None,
        });
}

#[test]
fn bodies_outside_products_have_a_counted_export_loss() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    product(&mut ir);
    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let loss = report
        .losses
        .iter()
        .find(|loss| loss.code == StepLossCode::BodyWithoutProductRepresentation.kind())
        .unwrap();
    assert!(loss.message.starts_with("1 body record(s)"));
    ir.model.product_definitions[0]
        .bodies
        .push(ir.model.bodies[0].id.clone());
    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::BodyWithoutProductRepresentation.kind()));
}

#[test]
fn solid_and_sheet_shell_wire_members_have_counted_export_losses() {
    for kind in [
        cadmpeg_ir::topology::BodyKind::Solid,
        cadmpeg_ir::topology::BodyKind::Sheet,
    ] {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        ir.model.bodies[0].kind = kind;
        let shell = &ir.model.shells[0];
        ir.model.shells[0] = cadmpeg_ir::topology::Shell::new(
            shell.id.clone(),
            shell.region.clone(),
            shell.faces().to_vec(),
            vec![ir.model.edges[0].id.clone()],
            vec![ir.model.vertices[0].id.clone()],
        )
        .unwrap();
        let report = write_step(
            &ir,
            &mut Vec::new(),
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .unwrap();
        let loss = report
            .losses
            .iter()
            .find(|loss| loss.code == StepLossCode::SurfaceShellWireTopologyOmitted.kind())
            .unwrap();
        assert!(loss
            .message
            .starts_with("1 wire edge record(s) and 1 free vertex record(s)"));
    }
}

#[test]
fn missing_local_product_refuses_root_and_nested_occurrences_before_output() {
    use cadmpeg_ir::products::{Occurrence, OccurrenceParent, PrototypeReference};
    for nested in [false, true] {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        product(&mut ir);
        let root: cadmpeg_ir::ids::OccurrenceId = "test:model:occurrence#root".try_into().unwrap();
        let occurrence = Occurrence {
            id: root.clone(),
            prototype: PrototypeReference::Local {
                definition: ir.model.product_definitions[0].id.clone(),
            },
            parent: OccurrenceParent::Root {},
            ordinal: 0,
            transform: cadmpeg_ir::transform::Transform::identity(),
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        };
        if nested {
            ir.model.occurrences.push(occurrence.clone());
        }
        ir.model.occurrences.push(Occurrence {
            id: "test:model:occurrence#missing".try_into().unwrap(),
            prototype: PrototypeReference::Local {
                definition: "test:model:product-definition#missing".try_into().unwrap(),
            },
            parent: if nested {
                OccurrenceParent::Occurrence { occurrence: root }
            } else {
                OccurrenceParent::Root {}
            },
            ..occurrence
        });
        let mut output = vec![0xaa];
        let error = write_step(
            &ir,
            &mut output,
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("1 occurrence record(s) whose local product definition is missing"));
        assert_eq!(output, [0xaa]);
    }
}
