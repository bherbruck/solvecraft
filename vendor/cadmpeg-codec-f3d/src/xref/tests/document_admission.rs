// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::collections::HashSet;
use std::io::Cursor;

use crate::test_support::assembly_test::{f3d_without_brep, XREF_ROLE};

use crate::F3dCodec;

use super::{occurrence_record, repeated_target_occurrence_record};
#[test]
fn typed_placement_admission_rejects_shape_collision() {
    let bytes = occurrence_record("role", 10, &[2, 3], None);
    let records =
        super::super::indexed_records(&cadmpeg_test_support::service_decode_context(), &bytes)
            .unwrap();
    let no_registered_placements = HashSet::new();
    assert!(super::super::occurrence_placements_filtered(
        &bytes,
        &records,
        None,
        Some(&no_registered_placements),
    )
    .is_empty());

    let registered_placement = HashSet::from([0]);
    assert_eq!(
        super::super::occurrence_placements_filtered(
            &bytes,
            &records,
            None,
            Some(&registered_placement),
        )
        .len(),
        1
    );
}

#[test]
fn assembly_root_without_brep_is_not_a_blocking_loss() {
    let archive = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert!(
        decoded
            .report()
            .losses
            .iter()
            .all(|loss| loss.severity < cadmpeg_ir::report::Severity::Error),
        "assembly document must not report blocking/error losses: {:?}",
        decoded.report().losses
    );
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("assembly document")));
    assert!(decoded
        .report()
        .notes
        .iter()
        .any(|note| note.contains("comp.f3d") && note.contains(XREF_ROLE)));
    let native =
        crate::native::F3dNative::load(decoded.ir().native.namespace("f3d").unwrap()).unwrap();
    assert_eq!(native.xref_designs.len(), 2);
    assert_eq!(native.xref_references.len(), 1);
    assert_eq!(native.xref_references[0].relative_path.as_str(), "comp.f3d");
    assert_eq!(native.xref_references[0].neutron_role.as_str(), XREF_ROLE);
    let source = decoded.ir().source.as_ref().unwrap();
    assert_eq!(
        source.attributes.get("docstruct_type").map(String::as_str),
        Some("assembly-design")
    );
}

#[test]
fn part_without_brep_keeps_blocking_losses() {
    // A leaf redirections table (no outgoing references) does not make a
    // BREP-less part a valid assembly.
    let archive = f3d_without_brep("part-design", "part.f3d", &[]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.severity == cadmpeg_ir::report::Severity::Blocking));
}

#[test]
fn repeated_target_component_insert_preserves_caller_refusal() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let bytes = repeated_target_occurrence_record(role, 10, 1, None);
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let error = super::super::repeated_target_component_insert(
            ctx,
            &bytes,
            0,
            bytes.len(),
            10,
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        )
        .unwrap_err();
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("placement must preserve the resource refusal");
        };
        assert_eq!(Some(limit), ctx.resource_refusal());
    });
}
