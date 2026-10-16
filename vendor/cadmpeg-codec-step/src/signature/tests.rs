// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
use super::validate_detached_cms;
use base64::{engine::general_purpose::STANDARD, Engine as _};

#[test]
fn ber_cursor_past_input_refuses_remaining_length() {
    let mut ber = super::Ber::new(b"");
    ber.at = 1;
    assert_eq!(super::require_empty(&ber), Err("BER cursor exceeds input"));
}

#[test]
fn signature_compact_refuses_collection_limit() {
    let input = b"AAAA";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(input, &arena, &policy)
        .expect("root fits selected policy");
    let error = super::decode_payload(input, &(0..input.len()), &ctx)
        .expect_err("four compact bytes exceed three collection items");
    assert!(matches!(
        error,
        crate::parse::ParseError::Resource(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "step_signature_compact_items"
    ));
}

#[test]
fn signature_decoded_bytes_refuse_collection_limit() {
    let input = b"AAAA";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 6;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(input, &arena, &policy)
        .expect("root fits selected policy");
    let error = super::decode_payload(input, &(0..input.len()), &ctx)
        .expect_err("four compact bytes and three decoded bytes exceed six items");
    assert!(matches!(
        error,
        crate::parse::ParseError::Resource(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "step_signature_cms_bytes"
    ));
}

struct SignatureView {
    span: std::ops::Range<usize>,
    payload: std::ops::Range<usize>,
    signed: std::ops::Range<usize>,
    cms: Vec<u8>,
}

impl SignatureView {
    fn signed_alphabet_bytes(&self, input: &[u8]) -> Option<Vec<u8>> {
        Some(
            input
                .get(self.signed.clone())?
                .iter()
                .copied()
                .filter(|byte| !byte.is_ascii_control())
                .collect(),
        )
    }
}

fn sections(exchange: &crate::parse::Exchange, input: impl AsRef<[u8]>) -> Vec<SignatureView> {
    let input = input.as_ref();
    let tokens = crate::test_support::with_service_context(input, crate::lex::lex_with_context)
        .expect("signature tokens");
    let exchange_start = tokens[0].span.start;
    exchange
        .signatures()
        .iter()
        .map(|span| {
            let section_tokens = tokens
                .iter()
                .filter(|token| token.span.start >= span.start && token.span.end <= span.end)
                .collect::<Vec<_>>();
            let payload = section_tokens[1].span.end..section_tokens[2].span.start;
            SignatureView {
                span: span.clone(),
                signed: exchange_start..span.start,
                cms: crate::test_support::with_service_context(input, |_, ctx| {
                    super::decode_payload(input, &payload, ctx)
                })
                .expect("admitted CMS payload"),
                payload,
            }
        })
        .collect()
}

#[test]
fn accepts_a_minimal_detached_signed_data_envelope() {
    let cms = [
        0x30, 0x5a, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02, 0xa0, 0x4d,
        0x30, 0x4b, 0x02, 0x01, 0x01, 0x31, 0x0d, 0x30, 0x0b, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01,
        0x65, 0x03, 0x04, 0x02, 0x01, 0x30, 0x0b, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d,
        0x01, 0x07, 0x01, 0x31, 0x2a, 0x30, 0x28, 0x02, 0x01, 0x01, 0x30, 0x05, 0x30, 0x00, 0x02,
        0x01, 0x01, 0x30, 0x0b, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00,
        0x04, 0x00,
    ];
    assert_eq!(validate_detached_cms(&cms), Ok(()));
}

const BER_CMS_INDEFINITE: &[u8] = &[
    0x30, 0x80, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02, 0xa0, 0x80, 0x30,
    0x80, 0x02, 0x01, 0x01, 0x31, 0x80, 0x30, 0x0b, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03,
    0x04, 0x02, 0x01, 0x00, 0x00, 0x30, 0x80, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01,
    0x07, 0x01, 0x00, 0x00, 0x31, 0x80, 0x30, 0x80, 0x02, 0x01, 0x01, 0x30, 0x80, 0x30, 0x00, 0x02,
    0x01, 0x01, 0x00, 0x00, 0x30, 0x0b, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
    0x01, 0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00,
    0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

#[test]
fn accepts_cms_ber_indefinite_lengths() {
    assert_eq!(validate_detached_cms(BER_CMS_INDEFINITE), Ok(()));
}

#[test]
fn accepts_ber_contextual_subject_key_identifier_and_octet_string() {
    assert_eq!(
        super::validate_signer_identifier(0x80, &[0x01, 0x02, 0x03]),
        Ok(())
    );
    assert_eq!(
        super::validate_octet_string(0x24, &[0x04, 0x01, 0xaa]),
        Ok(())
    );
}

#[test]
fn parser_retains_base64_encoded_ber_cms() {
    let payload = STANDARD.encode(BER_CMS_INDEFINITE);
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('BER CMS'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;{payload}\nENDSEC;"
    );
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("BER CMS witness");

    assert!(diagnostics.is_empty());
    assert_eq!(sections(&exchange, &source)[0].cms, BER_CMS_INDEFINITE);
}

#[test]
fn rejects_embedded_content() {
    let cms = [
        0x30, 0x5c, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02, 0xa0, 0x4f,
        0x30, 0x4d, 0x02, 0x01, 0x01, 0x31, 0x0d, 0x30, 0x0b, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01,
        0x65, 0x03, 0x04, 0x02, 0x01, 0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d,
        0x01, 0x07, 0x01, 0xa0, 0x02, 0x04, 0x00, 0x31, 0x2a, 0x30, 0x28, 0x02, 0x01, 0x01, 0x30,
        0x05, 0x30, 0x00, 0x02, 0x01, 0x01, 0x30, 0x0b, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65,
        0x03, 0x04, 0x02, 0x01, 0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01,
        0x01, 0x01, 0x05, 0x00, 0x04, 0x00,
    ];
    assert!(validate_detached_cms(&cms).is_err());
}

#[test]
fn parser_retains_multiple_signature_sections_after_exchange_terminator() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('signatures'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("multiple signatures");

    assert!(diagnostics.is_empty());
    assert_eq!(exchange.signatures().len(), 2);
    assert!(source[exchange.signatures()[0].clone()]
        .windows(b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=".len())
        .any(|bytes| bytes == b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA="));
    assert!(source[exchange.signatures()[1].clone()]
        .windows(b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=".len())
        .any(|bytes| bytes == b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA="));
    assert_eq!(sections(&exchange, source).len(), 2);
    let first = &sections(&exchange, source)[0];
    let second = &sections(&exchange, source)[1];
    assert_eq!(first.signed.end, first.span.start);
    assert_eq!(second.signed.end, second.span.start);
    let first_section = &source[first.span.clone()];
    let second_signed = &source[second.signed.clone()];
    assert!(second_signed
        .windows(first_section.len())
        .any(|window| window == first_section));
}

#[test]
fn parser_accepts_signature_edge_separators_and_keeps_each_boundary() {
    let payload = "MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=";
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('signatures'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;/* decoy ENDSEC; */\\N\\{payload}\\F\\/* trailing ENDSEC; */EN\nDSEC;SIGNATURE;\\F\\{payload}\\N\\ENDSEC;"
    );
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("edge separators");

    assert!(diagnostics.is_empty());
    assert_eq!(sections(&exchange, &source).len(), 2);
    assert_eq!(exchange.signatures().len(), 2);
    assert!(source.as_bytes()[exchange.signatures()[0].clone()].ends_with(b"DSEC;"));
    assert!(source.as_bytes()[exchange.signatures()[0].clone()]
        .windows(b"EN\nDSEC;".len())
        .any(|bytes| bytes == b"EN\nDSEC;"));
    assert!(source.as_bytes()[exchange.signatures()[1].clone()].ends_with(b"ENDSEC;"));
    assert_eq!(
        sections(&exchange, &source)[0].signed.end,
        exchange.signatures()[0].start
    );
    assert_eq!(
        sections(&exchange, &source)[1].signed.end,
        exchange.signatures()[1].start
    );
    assert_eq!(exchange.signatures()[1].start, exchange.signatures()[0].end);
    assert_eq!(sections(&exchange, &source)[0].cms.len(), 92);
    assert_eq!(sections(&exchange, &source)[1].cms.len(), 92);
}

#[test]
fn parser_does_not_treat_unseparated_endsec_text_as_a_signature_boundary() {
    let payload = "MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=";
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('signature'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;{payload}ENDSEC;AAAA\nENDSEC;"
    );
    let error =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect_err("unseparated ENDSEC text");

    assert!(matches!(
        error,
        crate::parse::ParseError::Lex(crate::lex::LexError { message, .. })
            if message == "invalid SIGNATURE base64 padding"
    ));
}

#[test]
fn parser_exposes_the_detached_signature_contract() {
    let source = b" /* leading trivia */ ISO-10303-21;HEADER;FILE_DESCRIPTION(('signature'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("signature contract");

    assert!(diagnostics.is_empty());
    let section = &sections(&exchange, source)[0];
    let signed = &source[section.signed.clone()];
    assert!(signed.starts_with(b"ISO-10303-21;"));
    assert!(signed.ends_with(b"END-ISO-10303-21;"));
    assert_eq!(
        &source[section.payload.clone()],
        b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\n"
    );
    assert_eq!(section.cms.len(), 92);
    let signed_alphabet = section
        .signed_alphabet_bytes(source)
        .expect("signed source range");
    assert!(!signed_alphabet.contains(&b'\n'));
    assert!(signed_alphabet.ends_with(b"END-ISO-10303-21;"));
    assert_eq!(
        &source[section.span.clone()],
        b"SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;"
    );
}

#[test]
fn parser_does_not_verify_detached_content_during_structural_admission() {
    let source = include_bytes!("tests/data/sg01_signature_method_selection.p21");
    let (original, original_diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("original signature");
    let mut tampered_source = source.to_vec();
    let marker = b"SG-01 CMS method witness";
    let marker_start = tampered_source
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("signature witness marker");
    tampered_source[marker_start] = b'T';
    let (tampered, tampered_diagnostics) =
        crate::test_support::with_service_context(&tampered_source, crate::parse::parse_inner)
            .expect("structural signature admission");

    assert!(original_diagnostics.is_empty());
    assert!(tampered_diagnostics.is_empty());
    assert_eq!(
        sections(&original, source)[0].cms,
        sections(&tampered, &tampered_source)[0].cms
    );
    assert_ne!(
        sections(&original, source)[0]
            .signed_alphabet_bytes(source)
            .expect("original signed content"),
        sections(&tampered, &tampered_source)[0]
            .signed_alphabet_bytes(&tampered_source)
            .expect("tampered signed content")
    );
}

#[test]
fn real_detached_cms_witness_remains_structural_after_source_tampering() {
    let source = include_bytes!("tests/data/sg04_openssl_detached.p21");
    let (original, original_diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("real CMS witness");
    let mut tampered_source = source.to_vec();
    let marker = b"SG-04 OpenSSL detached CMS witness";
    let marker_start = tampered_source
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("real CMS witness marker");
    tampered_source[marker_start] = b'X';
    let (tampered, tampered_diagnostics) =
        crate::test_support::with_service_context(&tampered_source, crate::parse::parse_inner)
            .expect("tampered CMS structure");

    assert!(original_diagnostics.is_empty());
    assert!(tampered_diagnostics.is_empty());
    assert_eq!(sections(&original, source)[0].cms.len(), 1324);
    assert_eq!(
        super::validate_detached_cms(&sections(&original, source)[0].cms),
        Ok(())
    );
    assert_eq!(
        sections(&original, source)[0].cms,
        sections(&tampered, &tampered_source)[0].cms
    );
    assert_ne!(
        sections(&original, source)[0]
            .signed_alphabet_bytes(source)
            .expect("original signed content"),
        sections(&tampered, &tampered_source)[0]
            .signed_alphabet_bytes(&tampered_source)
            .expect("tampered signed content")
    );
}

#[test]
fn signature_method_and_parameters_are_inside_cms_payload() {
    let source = include_bytes!("tests/data/sg01_signature_method_selection.p21");
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("signature method witness");

    assert!(diagnostics.is_empty());
    assert_eq!(sections(&exchange, source).len(), 1);
    let section = &sections(&exchange, source)[0];
    assert_eq!(
        &source[section.span.clone()],
        b"SIGNATURE;\nMFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;"
    );
    assert_eq!(
        &source[section.span.start..section.payload.start],
        b"SIGNATURE;"
    );
    assert_eq!(&source[section.payload.end..section.span.end], b"ENDSEC;");
    assert_eq!(section.cms.len(), 92);
}

#[test]
fn parser_projects_each_signature_to_preceding_alphabet_bytes() {
    let source = include_bytes!("tests/data/sg02_signed_byte_sequence.p21");
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("signed byte witness");

    assert!(diagnostics.is_empty());
    assert_eq!(sections(&exchange, source).len(), 2);
    let first = &sections(&exchange, source)[0];
    let second = &sections(&exchange, source)[1];
    assert_eq!(first.signed.start, 0);
    assert_eq!(first.signed.end, first.span.start);
    assert_eq!(second.signed.start, 0);
    assert_eq!(second.signed.end, second.span.start);
    assert_eq!(
        first.signed_alphabet_bytes(source).expect("first signed bytes"),
        b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('SG-02 signed byte sequence witness'),'4;2');FILE_NAME('sg02_signed_byte_sequence','2026-08-16T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=PRODUCT('P1','Part','',());#2=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#3=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#4=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#2,#3)) REPRESENTATION_CONTEXT('model','3D'));ENDSEC;END-ISO-10303-21;/*FIRST*/\\N\\"
    );
    let mut expected_second = first
        .signed_alphabet_bytes(source)
        .expect("first signed bytes")
        .clone();
    expected_second.extend_from_slice(
        b"SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=ENDSEC;/*SECOND*/\\F\\",
    );
    assert_eq!(
        second
            .signed_alphabet_bytes(source)
            .expect("second signed bytes"),
        expected_second
    );
    assert_eq!(
        &source[first.span.clone()],
        b"SIGNATURE;\nMFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;"
    );
}

#[test]
fn parser_ignores_controls_inside_signature_terminators() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('signature'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nEN\nDSEC;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("split signature terminator");
    assert_eq!(exchange.signatures().len(), 1);
}

#[test]
fn parser_rejects_invalid_signature_base64() {
    for (payload, expected_message) in [
        ("YWJjZA==!", "invalid SIGNATURE base64 padding"),
        ("YWJjZA==AAAA", "invalid SIGNATURE base64 padding"),
        ("YWJjZA=", "SIGNATURE base64 content has incomplete quantum"),
        ("YWJj ZA==", "invalid SIGNATURE base64 character"),
    ] {
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('signature'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;{payload}\nENDSEC;"
        );
        let error =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect_err("invalid signature");
        assert!(matches!(
            error,
            crate::parse::ParseError::Lex(crate::lex::LexError { message, .. })
                if message == expected_message
        ));
    }
}
