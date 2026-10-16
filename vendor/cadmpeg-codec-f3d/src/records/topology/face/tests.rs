// SPDX-License-Identifier: Apache-2.0
//! Face operands, source groups and the face-recipe postlude.

#[test]
fn face_source_group_borrowed_wire_matches_owned_wire_bytes() {
    let prefix = r#"{"id":"face-source","scope_record_index":1,"carrier_reference_ordinal":0,"carrier_record_index":2,"carrier_byte_offset":0,"carrier_class_tag":"302","carrier_frame_length":80,"paired_record_index":3,"paired_byte_offset":80,"paired_class_tag":"303""#;
    let member = r#"{"record_index":100,"byte_offset":1000,"class_tag":"304","persistent_identity":{"local_id":1,"local_id_offset":1021,"asset_id":"AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE","asset_id_offset":1033,"context_id":"11111111-2222-4333-8444-555555555555","context_id_offset":1109,"tail_slot_present":false,"tail_slot_offset":1185,"next_record_index":101,"next_byte_offset":1190}}"#;
    for (members, offsets) in [
        ("[]".to_owned(), "[]"),
        (format!("[{member}]"), "[25]"),
        (format!("[{member},{member}]"), "[25,36]"),
    ] {
        let group: super::DesignFaceSourceGroup = serde_json::from_str(&format!(
            "{prefix},\"source_reference_offsets\":{offsets},\"source_members\":{members}}}"
        ))
        .unwrap();
        let owned = super::DesignFaceSourceGroupWire::from(group.clone());
        assert_eq!(
            serde_json::to_vec(&group).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn face_source_group_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a super::DesignFaceSourceGroup,
    }
    let wire = r#"{"id":"face-source","scope_record_index":1,"carrier_reference_ordinal":0,"carrier_record_index":2,"carrier_byte_offset":0,"carrier_class_tag":"302","carrier_frame_length":80,"paired_record_index":3,"paired_byte_offset":80,"paired_class_tag":"303","source_reference_offsets":[25],"source_members":[{"record_index":100,"byte_offset":1000,"class_tag":"304","persistent_identity":{"local_id":1,"local_id_offset":1021,"asset_id":"AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE","asset_id_offset":1033,"context_id":"11111111-2222-4333-8444-555555555555","context_id_offset":1109,"tail_slot_present":false,"tail_slot_offset":1185,"next_record_index":101,"next_byte_offset":1190}}]}"#;
    let group: super::DesignFaceSourceGroup = serde_json::from_str(wire).unwrap();
    let record = NestedRecord {
        id: "f3d:native:face-source#0",
        value: &group,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || {
            super::FACE_SOURCE_GROUP_CLONE_COUNT.with(|count| count.set(0));
            crate::records::topology::construction::PERSISTENT_IDENTITY_CLONE_COUNT
                .with(|count| count.set(0));
        },
        || {
            super::FACE_SOURCE_GROUP_CLONE_COUNT.with(std::cell::Cell::get)
                + crate::records::topology::construction::PERSISTENT_IDENTITY_CLONE_COUNT
                    .with(std::cell::Cell::get)
        },
    );
}

#[test]
fn face_source_rows_preserve_wire_and_reject_unequal_offsets() {
    let prefix = r#"{"id":"face-source","scope_record_index":1,"carrier_reference_ordinal":0,"carrier_record_index":2,"carrier_byte_offset":0,"carrier_class_tag":"302","carrier_frame_length":80,"paired_record_index":3,"paired_byte_offset":80,"paired_class_tag":"303""#;
    let member = r#"{"record_index":100,"byte_offset":1000,"class_tag":"304","persistent_identity":{"local_id":1,"local_id_offset":1021,"asset_id":"AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE","asset_id_offset":1033,"context_id":"11111111-2222-4333-8444-555555555555","context_id_offset":1109,"tail_slot_present":false,"tail_slot_offset":1185,"next_record_index":101,"next_byte_offset":1190}}"#;
    for (members, offsets) in [
        ("[]".to_owned(), "[]"),
        (format!("[{member}]"), "[25]"),
        (format!("[{member},{member}]"), "[25,36]"),
    ] {
        let wire = format!(
            "{prefix},\"source_reference_offsets\":{offsets},\"source_members\":{members}}}"
        );
        let group: super::DesignFaceSourceGroup =
            serde_json::from_str(&wire).expect("Face source rows");
        assert_eq!(
            serde_json::to_string(&group).expect("Face source wire"),
            wire
        );
        let invalid = wire.replace(
            &format!("\"source_reference_offsets\":{offsets}"),
            "\"source_reference_offsets\":[25,36,47]",
        );
        let error = serde_json::from_str::<super::DesignFaceSourceGroup>(&invalid)
            .expect_err("unequal source arrays")
            .to_string();
        assert!(error.contains("source_members"));
        assert!(error.contains("source_reference_offsets"));
    }
}

#[test]
fn face_source_span_rejects_empty_reversed_and_conflicting_lengths() {
    let wire = r#"{"id":"face-source","scope_record_index":1,"carrier_reference_ordinal":0,"carrier_record_index":2,"carrier_byte_offset":10,"carrier_class_tag":"302","carrier_frame_length":80,"paired_record_index":3,"paired_byte_offset":90,"paired_class_tag":"303","source_reference_offsets":[],"source_members":[]}"#;
    for (field, value) in [
        ("paired_byte_offset", 10),
        ("paired_byte_offset", 9),
        ("carrier_frame_length", 79),
    ] {
        let mut invalid: serde_json::Value = serde_json::from_str(wire).expect("Face source wire");
        invalid[field] = value.into();
        let error = serde_json::from_value::<super::DesignFaceSourceGroup>(invalid)
            .expect_err("invalid carrier span")
            .to_string();
        assert!(error.contains(field));
    }
    let group: super::DesignFaceSourceGroup =
        serde_json::from_str(wire).expect("positive carrier span");
    assert_eq!(
        serde_json::to_string(&group).expect("Face source wire"),
        wire
    );
}

#[test]
fn face_operand_wire_derives_node_offsets() {
    for count in [0_u32, 1, 3] {
        let offsets: Vec<_> = (0..count).map(|index| 100 + index * 16).collect();
        let nodes: Vec<_> = offsets
            .iter()
            .map(|offset| {
                serde_json::json!({
                    "byte_offset": offset, "end_byte_offset": offset + 16,
                    "program": [-1, -1, 2, 7], "recipe_structure": null
                })
            })
            .collect();
        let base = serde_json::json!({
            "id": "face", "scope_record_index": 1, "scope_reference_ordinal": 0,
            "record_index": 2, "byte_offset": 10, "class_tag": "346",
            "paired_byte_offset": 20, "paired_class_tag": "262",
            "recipe_record_index": 5, "recipe_record_byte_offset": 30,
            "recipe_id": "recipe", "recipe_prefix_offset": 41, "recipe_prefix_bytes": "",
            "recipe_references": [], "recipe_kind": "bounded_face",
            "recipe_program_offset": 50, "recipe_program": [0, -1, 1],
            "recipe_node_offsets": offsets, "recipe_nodes": nodes,
            "next_record_index": 4, "next_byte_offset": 200
        });
        for grouped in [false, true] {
            let mut wire = base.clone();
            if grouped {
                wire["group_record_index"] = serde_json::json!(5);
                wire["group_member_ordinal"] = serde_json::json!(0);
            }
            for field in ["group_record_index", "group_member_ordinal"] {
                let mut invalid = wire.clone();
                invalid
                    .as_object_mut()
                    .unwrap()
                    .remove("group_record_index");
                invalid
                    .as_object_mut()
                    .unwrap()
                    .remove("group_member_ordinal");
                invalid[field] = serde_json::json!(5);
                let error = serde_json::from_value::<super::DesignFaceOperand>(invalid)
                    .unwrap_err()
                    .to_string();
                assert!(error.contains("group_record_index"));
                assert!(error.contains("group_member_ordinal"));
            }
            let operand: super::DesignFaceOperand = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(&operand).unwrap(), wire);
            let mut invalid = wire.clone();
            invalid["recipe_node_offsets"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!(999));
            assert!(serde_json::from_value::<super::DesignFaceOperand>(invalid)
                .unwrap_err()
                .to_string()
                .contains("recipe_node_offsets"));
            if count != 0 {
                let mut invalid = wire;
                invalid["recipe_node_offsets"][0] = serde_json::json!(999);
                assert!(serde_json::from_value::<super::DesignFaceOperand>(invalid)
                    .unwrap_err()
                    .to_string()
                    .contains("recipe_node_offsets"));
            }
        }
    }
}

fn face_operand_for_borrowed(grouped: bool) -> super::DesignFaceOperand {
    let mut wire = serde_json::json!({
        "id": "f3d:native:face-operand#0", "scope_record_index": 1,
        "scope_reference_ordinal": 0, "record_index": 2, "byte_offset": 10,
        "class_tag": "346", "paired_byte_offset": 20, "paired_class_tag": "262",
        "recipe_record_index": 5, "recipe_record_byte_offset": 30,
        "recipe_id": "recipe", "recipe_prefix_offset": 41,
        "recipe_prefix_bytes": "", "recipe_references": [],
        "recipe_kind": "bounded_face", "recipe_program_offset": 50,
        "recipe_program": [0, -1, 1], "recipe_node_offsets": [100],
        "recipe_nodes": [{"byte_offset": 100, "end_byte_offset": 116,
            "program": [-1, -1, 2, 7], "recipe_structure": null}],
        "candidate_faces": [], "resolved_face_slots": [],
        "next_record_index": 4, "next_byte_offset": 200
    });
    if grouped {
        wire["group_record_index"] = serde_json::json!(5);
        wire["group_member_ordinal"] = serde_json::json!(0);
    }
    serde_json::from_value(wire).unwrap()
}

#[test]
fn face_operand_borrowed_wire_matches_owned_wire_bytes() {
    for grouped in [false, true] {
        let operand = face_operand_for_borrowed(grouped);
        let owned = super::DesignFaceOperandWire::from(operand.clone());
        assert_eq!(
            serde_json::to_vec(&operand).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn face_operand_native_retained_limit_refuses_before_clone() {
    let operand = face_operand_for_borrowed(true);
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &operand,
        "design_face_operands",
        || super::FACE_OPERAND_CLONE_COUNT.with(|count| count.set(0)),
        || super::FACE_OPERAND_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn face_recipe_postlude_derives_delimiters_and_rejects_other_programs() {
    let side = r#"{"field_count":2,"header_value":0,"scalars":[0],"payload_prefix":[0],"payload_entry_count":0,"entries":[]}"#;
    let prefix = format!(r#"{{"root":0,"prelude":[1,2],"sides":[{side},{side}]"#);
    for value in [i32::MIN, -1, 0, 4, i32::MAX] {
        let wire = format!(r#"{prefix},"postlude":[-1,{value},-1,0,0,-1]}}"#);
        let structure: super::DesignFaceRecipeStructure = serde_json::from_str(&wire).unwrap();
        assert_eq!(structure.postlude_value, Some(value));
        assert_eq!(serde_json::to_string(&structure).unwrap(), wire);
    }
    let omitted = format!("{prefix}}}");
    for wire in [omitted.clone(), format!(r#"{prefix},"postlude":[]}}"#)] {
        let structure: super::DesignFaceRecipeStructure = serde_json::from_str(&wire).unwrap();
        assert_eq!(structure.postlude_value, None);
        assert_eq!(serde_json::to_string(&structure).unwrap(), omitted);
    }
    for postlude in [
        "[-1]",
        "[-1,4,-1,0,0]",
        "[0,4,-1,0,0,-1]",
        "[-1,4,-1,0,1,-1]",
    ] {
        let wire = format!(r#"{prefix},"postlude":{postlude}}}"#);
        assert!(
            serde_json::from_str::<super::DesignFaceRecipeStructure>(&wire)
                .unwrap_err()
                .to_string()
                .contains("postlude")
        );
    }
}
