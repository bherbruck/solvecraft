// SPDX-License-Identifier: Apache-2.0
//! Caller-context limits for decoded STEP presentation text.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{find_color, ColorResolution, StyleDomain};

const LAYER_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();#2=PRESENTATION_LAYER_ASSIGNMENT('Layer','details',(#1));ENDSEC;END-ISO-10303-21;";

fn layer_result(retained_limit: u64) -> Result<(), CodecError> {
    let (exchange, _) =
        crate::test_support::with_service_context(LAYER_SOURCE, crate::parse::parse_inner)
            .expect("valid layer exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(LAYER_SOURCE, &arena, &policy)
        .expect("root fits retained policy");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let index = crate::reader::index::CarrierIndex::from_ir(&ir, &ctx)?;
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, &ctx)?;
    super::super::decode(&exchange, &topology.value, &mut ir, &BTreeMap::new(), &ctx)?;
    Ok(())
}

#[test]
fn presentation_layer_name_refuses_retained_limit() {
    assert!(matches!(
        layer_result(1),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text"
    ));
}

#[test]
fn presentation_layer_description_refuses_retained_limit() {
    assert!(matches!(
        layer_result(u64::try_from("Layer".len()).expect("fixed name length fits u64")),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text"
    ));
}

fn color_result(source: &[u8], retained_limit: u64) -> Result<Option<ColorResolution>, CodecError> {
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid colour exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits retained policy");
    let result = find_color(
        1,
        &exchange,
        StyleDomain::Any,
        super::super::ColorSearchState {
            storage: &std::cell::RefCell::new(
                ctx.reserve_scoped(0, "color search fixture")
                    .expect("scope"),
            ),
            active: &mut BTreeSet::new(),
            cache: &mut super::super::ColorCache::default(),
            losses: &mut Vec::new(),
            invalid_surface_sides: &mut BTreeSet::new(),
        },
        0,
        &ctx,
    );
    result
}

#[test]
fn rgb_colour_name_refuses_retained_limit() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=COLOUR_RGB('named colour',1.,0.,0.);ENDSEC;END-ISO-10303-21;";
    assert!(matches!(
        Err::<(), CodecError>(cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::RetainedBytes, "step_string_text", |cap| color_result(SOURCE, cap))),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text"
    ));
}

#[test]
fn predefined_colour_name_refuses_retained_limit() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DRAUGHTING_PRE_DEFINED_COLOUR('red');ENDSEC;END-ISO-10303-21;";
    assert!(matches!(
        Err::<(), CodecError>(cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::RetainedBytes, "step_string_text", |cap| color_result(SOURCE, cap))),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_string_text"
    ));
}
