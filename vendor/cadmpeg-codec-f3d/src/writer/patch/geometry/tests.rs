// SPDX-License-Identifier: Apache-2.0

use super::{patch_framed_geometry, GeometryEdits};
use cadmpeg_asm::sab::{Record, Token};
use cadmpeg_ir::transform::Transform;
use std::collections::BTreeMap;

#[test]
fn body_transform_reference_refuses_32_bit_wrap() {
    let records = [
        Record {
            index: 0,
            name: "transform".to_owned(),
            tokens: Vec::new().into(),
            offset: 0,
            len: 0,
        },
        Record {
            index: 1,
            name: "body".to_owned(),
            tokens: vec![
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(4_294_967_296),
            ]
            .into(),
            offset: 0,
            len: 0,
        },
    ];
    let transforms = BTreeMap::from([("f3d:brep:entity#1".to_owned(), Transform::identity())]);
    let mut bytes = Vec::new();
    patch_framed_geometry(
        &mut bytes,
        &records,
        &GeometryEdits {
            positions: &BTreeMap::new(),
            lines: &BTreeMap::new(),
            conics: &BTreeMap::new(),
            degenerate_curves: &BTreeMap::new(),
            planes: &BTreeMap::new(),
            spheres: &BTreeMap::new(),
            tori: &BTreeMap::new(),
            cones: &BTreeMap::new(),
            body_transforms: &transforms,
            entity_colors: &BTreeMap::new(),
            edge_ranges: &BTreeMap::new(),
            face_senses: &BTreeMap::new(),
            coedge_senses: &BTreeMap::new(),
            procedural_surface_edits: &BTreeMap::new(),
            nurbs_surfaces: &BTreeMap::new(),
            nurbs_curves: &BTreeMap::new(),
            pcurves: &BTreeMap::new(),
            procedural_curve_edits: &BTreeMap::new(),
            procedural_surface_fits: &BTreeMap::new(),
            creation_timestamps: &BTreeMap::new(),
            edge_continuities: &BTreeMap::new(),
            vertex_ownerships: &BTreeMap::new(),
            face_sidedness: &BTreeMap::new(),
            tolerant_edges: &BTreeMap::new(),
            tolerant_vertices: &BTreeMap::new(),
        },
        1.0,
    )
    .expect("an unaddressable reference names no transform record");
    assert!(bytes.is_empty());
}
