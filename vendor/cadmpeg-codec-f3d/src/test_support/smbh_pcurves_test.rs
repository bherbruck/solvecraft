// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_asm::asm_header;
use cadmpeg_test_support::service_decode_context;

use crate::test_support::smbh_blocks_test::{
    generated_curve_block, generated_pcurve_block, generated_planar_pcurve_block,
    generated_planar_rational_pcurve_block, generated_surface_block,
};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::tokens_test::{t_dbl, t_end, t_ident, t_long, t_ref, t_subident};

/// Add a generated inline 2D `nubs` pcurve to the first coedge of the base
/// topology fixture. The new record is appended at `RecordTable` index 19.
pub(crate) fn synthetic_geometry_with_pcurve_smbh() -> Vec<u8> {
    synthetic_geometry_with_pcurve_block_smbh(generated_planar_pcurve_block())
}

pub(crate) fn synthetic_geometry_with_wrapped_ref_pcurve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_pcurve_smbh();
    let opener = bytes
        .windows(b"\x0f\x0d\x0bexp_par_cur".len())
        .position(|window| window == b"\x0f\x0d\x0bexp_par_cur")
        .expect("generated wrapped pcurve subtype");
    let close = bytes[opener..]
        .windows([0x10, 0x0a, 0x0b, 0x0a, 0x0b].len())
        .position(|window| window == [0x10, 0x0a, 0x0b, 0x0a, 0x0b])
        .map(|offset| opener + offset)
        .expect("generated wrapped pcurve subtype close");
    let mut reference = vec![0x0f];
    t_ident(&mut reference, "ref");
    t_long(&mut reference, 0);
    reference.push(0x10);
    bytes.splice(opener..=close, reference);

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut target = Vec::new();
    t_subident(&mut target, "intcurve");
    t_ident(&mut target, "curve");
    t_ref(&mut target, -1);
    t_long(&mut target, -1);
    t_ref(&mut target, -1);
    target.push(0x0f);
    t_ident(&mut target, "int_int_cur");
    target.extend_from_slice(&generated_pcurve_block());
    target.push(0x10);
    t_end(&mut target);
    bytes.splice(delta..delta, target);
    bytes
}

pub(crate) fn synthetic_geometry_with_inline_pcurve_on_nurbs_surface_smbh() -> Vec<u8> {
    replace_generated_face_with_nurbs_surface(synthetic_geometry_with_pcurve_smbh())
}

pub(crate) fn synthetic_inline_pcurve_with_referenced_support_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_inline_pcurve_on_nurbs_surface_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&service_decode_context(), &bytes)
        .expect("history scan")
        .unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let asmheader_end = records[0].offset + records[0].len - 1;

    let mut target = vec![0x0f];
    t_ident(&mut target, "int_int_cur");
    target.extend_from_slice(&generated_pcurve_block());
    target.push(0x10);
    bytes.splice(asmheader_end..asmheader_end, target);

    let opener = bytes
        .windows(b"\x0f\x0d\x0bexp_par_cur".len())
        .position(|window| window == b"\x0f\x0d\x0bexp_par_cur")
        .expect("inline pcurve scope");
    let close = bytes[opener..]
        .windows([0x10, 0x0a, 0x0b, 0x0a, 0x0b].len())
        .position(|window| window == [0x10, 0x0a, 0x0b, 0x0a, 0x0b])
        .map(|offset| opener + offset)
        .expect("inline pcurve scope close");
    let mut reference = vec![0x0f];
    t_ident(&mut reference, "ref");
    t_long(&mut reference, 0);
    reference.push(0x10);
    bytes.splice(close..close, reference);
    bytes
}

fn replace_generated_face_with_nurbs_surface(mut bytes: Vec<u8>) -> Vec<u8> {
    let planar_pcurve = generated_planar_pcurve_block();
    if let Some(offset) = bytes
        .windows(planar_pcurve.len())
        .position(|window| window == planar_pcurve)
    {
        bytes.splice(
            offset..offset + planar_pcurve.len(),
            generated_pcurve_block(),
        );
    }
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&service_decode_context(), &bytes)
        .expect("history scan")
        .unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[6];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.extend_from_slice(&generated_surface_block());
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_geometry_with_ref_pcurve_on_nurbs_surface_smbh() -> Vec<u8> {
    replace_generated_face_with_nurbs_surface(synthetic_geometry_with_ref_pcurve_smbh())
}

pub(crate) fn synthetic_geometry_with_short_pcurve_tail_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_pcurve_smbh();
    let marker = [0x10, 0x0a, 0x0b, 0x0a, 0x0b, 0x06];
    let tail = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("generated inline pcurve tail");
    bytes.remove(tail + 1);
    bytes
}

pub(crate) fn synthetic_geometry_with_out_of_scope_pcurve_cache_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_additional_out_of_scope_pcurve_cache_smbh();
    let subtype = bytes
        .windows(b"exp_par_cur".len())
        .position(|window| window == b"exp_par_cur")
        .expect("generated inline pcurve subtype");
    let cache = bytes[subtype..]
        .windows(b"nubs".len())
        .position(|window| window == b"nubs")
        .map(|offset| subtype + offset)
        .expect("generated inline pcurve cache");
    bytes[cache] = b'x';
    bytes
}

pub(crate) fn synthetic_geometry_with_additional_out_of_scope_pcurve_cache_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_pcurve_smbh();
    let subtype = bytes
        .windows(b"exp_par_cur".len())
        .position(|window| window == b"exp_par_cur")
        .expect("generated inline pcurve subtype");
    let tail = bytes[subtype..]
        .windows([0x10, 0x0a, 0x0b, 0x0a, 0x0b].len())
        .position(|window| window == [0x10, 0x0a, 0x0b, 0x0a, 0x0b])
        .map(|offset| subtype + offset)
        .expect("generated inline pcurve subtype close");
    bytes.splice(tail + 1..tail + 1, generated_pcurve_block());
    bytes
}

pub(crate) fn synthetic_geometry_with_rational_pcurve_smbh() -> Vec<u8> {
    synthetic_geometry_with_pcurve_block_smbh(generated_planar_rational_pcurve_block())
}

fn synthetic_geometry_with_pcurve_block_smbh(block: Vec<u8>) -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&service_decode_context(), &bytes)
        .expect("history scan")
        .unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let coedge = &records[7];
    let record = &mut bytes[coedge.offset..coedge.offset + coedge.len];
    let pcurve_ref_tag = record.iter().rposition(|b| *b == 0x0c).unwrap();
    record[pcurve_ref_tag + 1..pcurve_ref_tag + 9].copy_from_slice(&19i64.to_le_bytes());

    // Move the coedge's edge endpoints onto the pcurve's neutral surface image.
    // The native plane chart stores neutral `(u, v)` as `(u / 10, v / -10)`.
    for (index, position_cm) in [(16usize, [0.025, 0.05, 0.0]), (17, [0.075, 0.15, 0.0])] {
        let point = &records[index];
        let record = &mut bytes[point.offset..point.offset + point.len];
        let tag = record.iter().position(|b| *b == 0x13).unwrap();
        for (slot, value) in position_cm.iter().copied().enumerate() {
            record[tag + 1 + slot * 8..tag + 9 + slot * 8]
                .copy_from_slice(&f64::to_le_bytes(value));
        }
    }

    let delta = bytes[..]
        .windows(b"delta_state".len())
        .position(|w| w == b"delta_state")
        .unwrap()
        - 2;
    let mut pcurve = Vec::new();
    t_ident(&mut pcurve, "pcurve");
    t_ref(&mut pcurve, -1);
    t_long(&mut pcurve, -1);
    t_ref(&mut pcurve, -1);
    t_long(&mut pcurve, 0);
    pcurve.push(0x0b);
    pcurve.push(0x0f);
    t_ident(&mut pcurve, "exp_par_cur");
    pcurve.extend_from_slice(&block);
    t_dbl(&mut pcurve, 0.001);
    pcurve.push(0x10);
    pcurve.extend_from_slice(&[0x0a, 0x0b, 0x0a, 0x0b]);
    t_dbl(&mut pcurve, -1.0);
    t_dbl(&mut pcurve, 2.0);
    t_end(&mut pcurve);
    bytes.splice(delta..delta, pcurve);
    bytes
}

pub(crate) fn synthetic_geometry_with_ref_pcurve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&service_decode_context(), &bytes)
        .expect("history scan")
        .unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let coedge = &records[7];
    let record = &mut bytes[coedge.offset..coedge.offset + coedge.len];
    let pcurve_ref_tag = record.iter().rposition(|byte| *byte == 0x0c).unwrap();
    record[pcurve_ref_tag + 1..pcurve_ref_tag + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut records = Vec::new();
    t_ident(&mut records, "pcurve");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    t_long(&mut records, 2);
    t_ref(&mut records, 20);
    t_dbl(&mut records, -2.0);
    t_dbl(&mut records, 4.0);
    t_end(&mut records);
    t_subident(&mut records, "intcurve");
    t_ident(&mut records, "curve");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    records.extend_from_slice(&generated_curve_block());
    records.extend_from_slice(&generated_planar_pcurve_block());
    t_end(&mut records);
    bytes.splice(delta..delta, records);
    bytes
}

pub(crate) fn with_pcurve_discriminator(mut bytes: Vec<u8>, discriminator: i64) -> Vec<u8> {
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&service_decode_context(), &bytes)
        .expect("history scan")
        .unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let pcurve = records
        .iter()
        .find(|record| record.head() == "pcurve")
        .expect("generated pcurve record");
    let offsets = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        pcurve,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x04,
    )
    .expect("generated pcurve integer offsets");
    bytes[offsets[1] + 1..offsets[1] + 9].copy_from_slice(&discriminator.to_le_bytes());
    bytes
}

pub(crate) fn with_inline_pcurve_non_boolean_wrapper(mut bytes: Vec<u8>) -> Vec<u8> {
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&service_decode_context(), &bytes)
        .expect("history scan")
        .unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let pcurve = records
        .iter()
        .find(|record| record.head() == "pcurve")
        .expect("generated pcurve record");
    let integers = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        pcurve,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x04,
    )
    .expect("generated pcurve integer offsets");
    let wrapper = integers[1] + 9;
    assert_eq!(bytes[wrapper], 0x0b, "generated inline wrapper boolean");
    bytes.splice(wrapper..=wrapper, [0x02, 0x00]);
    bytes
}

pub(crate) fn with_ref_pcurve_companion_name(mut bytes: Vec<u8>, name: &[u8; 8]) -> Vec<u8> {
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&service_decode_context(), &bytes)
        .expect("history scan")
        .unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let pcurve = records
        .iter()
        .find(|record| record.head() == "pcurve")
        .expect("generated pcurve record");
    let companion_index = pcurve.ref_at(4).expect("generated ref-form companion");
    let companion = &records[usize::try_from(companion_index).unwrap()];
    let head = bytes[companion.offset..companion.offset + companion.len]
        .windows(b"intcurve".len())
        .position(|window| window == b"intcurve")
        .map(|offset| companion.offset + offset)
        .expect("generated intcurve companion name");
    bytes[head..head + name.len()].copy_from_slice(name);
    bytes
}

pub(crate) fn with_ref_pcurve_companion_reversed(mut bytes: Vec<u8>) -> Vec<u8> {
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&service_decode_context(), &bytes)
        .expect("history scan")
        .unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let pcurve = records
        .iter()
        .find(|record| record.head() == "pcurve")
        .expect("generated pcurve record");
    let companion_index = pcurve.ref_at(4).expect("generated ref-form companion");
    let companion = &records[usize::try_from(companion_index).unwrap()];
    let offset = bytes[companion.offset..companion.offset + companion.len]
        .windows(b"\x0d\x04nubs".len())
        .position(|window| window == b"\x0d\x04nubs")
        .map(|offset| companion.offset + offset)
        .expect("generated intcurve cache marker");
    bytes.splice(offset..offset, [0x0a]);
    bytes
}
