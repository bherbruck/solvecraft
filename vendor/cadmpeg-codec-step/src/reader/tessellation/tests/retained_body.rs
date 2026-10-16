// SPDX-License-Identifier: Apache-2.0
//! Retained mesh body admission through the STEP tessellation reader.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::BodyId;

const BODY_LINKED_TRIANGLE: &str = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)));
#3=TESSELLATED_SOLID('',(#2),#10);
#10=TESSELLATED_ITEM();";

fn decode_with_body(policy: DecodePolicy) -> Result<CadIr, CodecError> {
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;{BODY_LINKED_TRIANGLE}ENDSEC;END-ISO-10303-21;"
    );
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("test exchange parses");
    let mut ir = CadIr::empty();
    let topology_arena = DecodeArena::new();
    let (topology_ctx, _) = DecodeContext::from_root_bytes(
        source.as_bytes(),
        &topology_arena,
        &DecodePolicy::service(),
    )?;
    let geometry = super::super::super::geometry::decode(&exchange, &mut ir, &topology_ctx)
        .expect("resource allocation did not fail")
        .0
        .value;
    let index = super::super::super::index::CarrierIndex::from_ir(&ir, &topology_ctx)?;
    let mut topology =
        super::super::super::topology::decode(&exchange, &mut ir, &index, &topology_ctx)
            .expect("test topology decodes")
            .value;
    topology.body_by_root.insert(
        10,
        vec![BodyId::try_from("step:data:body#10").expect("test body id")],
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)?;
    super::super::decode(&exchange, &geometry, &topology, &mut ir, &ctx)?;
    Ok(ir)
}

#[test]
fn tessellation_mesh_body_copy_is_charged_before_clone() {
    let service = DecodePolicy::service();
    let ir = decode_with_body(service).expect("service admits mesh body");
    assert_eq!(ir.model.tessellations.len(), 1);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_tessellation_mesh_body",
        |limit| {
            let mut limited = service;
            limited.limits.max_retained_bytes = limit;
            decode_with_body(limited)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_tessellation_mesh_body")
    );
}
