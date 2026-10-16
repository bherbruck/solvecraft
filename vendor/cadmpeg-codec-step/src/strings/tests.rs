// SPDX-License-Identifier: Apache-2.0
//! Part 21 string codec tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::StepLossCode;
use crate::test_support::exchange::decode_inline;
use crate::StepCodec;

#[test]
pub(crate) fn string_codec_decodes_all_part21_escape_forms_and_round_trips_unicode() {
    use crate::parse::implementation_level::ImplementationLevel;
    use crate::strings::encode;

    assert_eq!(
        crate::test_support::with_service_context(b"it''s", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "it's"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"a\\\\b", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "a\\b"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\\X\\E9", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "é"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\\X2\\03A9\\X0\\", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "Ω"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\\X4\\0001F642\\X0\\", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "🙂"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\\S\\D", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "Ä"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\\PA\\\\S\\D", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "Ä"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\\PB\\\\S\\A", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "Á"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\\PC\\\\S\\!", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "Ħ"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\\PD\\\\S\\!", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "Ą"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\\PE\\\\S\\0", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "А"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\\PF\\\\S\\G", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "ا"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\\PG\\\\S\\A", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "Α"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\\PH\\\\S\\`", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "א"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"\\PI\\\\S\\P", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "Ğ"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"line\\N\\text\\F\\tail", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        "linetexttail"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"caf\xC3\xA9", |input, ctx| {
            crate::strings::decode_with_context(input, ImplementationLevel::Edition3Class1, ctx)
        })
        .unwrap(),
        "café"
    );
    assert_eq!(
        crate::test_support::with_service_context(b"caf\xC3\xA9\\X2\\03A9\\X0\\", |input, ctx| {
            crate::strings::decode_with_context(input, ImplementationLevel::Edition3Class1, ctx)
        })
        .unwrap(),
        "caféΩ"
    );
    let crate::strings::StringDecodeFailure::Invalid(error) =
        crate::test_support::with_service_context(b"caf\xE9", |input, ctx| {
            crate::strings::decode_with_context(input, ImplementationLevel::Edition3Class1, ctx)
        })
        .unwrap_err()
    else {
        panic!("invalid string must return its syntax error");
    };
    assert_eq!(error.message, "invalid UTF-8 direct string bytes");

    for text in ["ASCII", "it's \\ quoted", "café Ω 🙂"] {
        assert_eq!(
            crate::test_support::with_service_context(encode(text).as_bytes(), |input, ctx| {
                crate::strings::decode_with_context(
                    input,
                    crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                    ctx,
                )
            })
            .unwrap(),
            text
        );
    }
}

#[test]
fn writer_and_lexer_preserve_apostrophes_and_backslashes_once() {
    use crate::lex::TokenKind;

    let source = "O'Brien \\ fixtures";
    let encoded = crate::writer::string(source);
    let tokens =
        crate::test_support::with_service_context(encoded.as_bytes(), crate::lex::lex_with_context)
            .expect("lex encoded string");
    let TokenKind::String(bytes) = &tokens[0].kind else {
        panic!("encoded text did not lex as a string")
    };
    assert_eq!(
        crate::test_support::with_service_context(bytes, |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .unwrap(),
        source
    );
    assert!(encoded.contains("O''Brien"));
    assert!(encoded.contains("\\\\"));
}

#[test]
fn wide_escape_streams_surrogate_pairs_and_rejects_isolated_surrogates() {
    assert_eq!(
        crate::test_support::with_service_context(b"\\X2\\D83DDE42\\X0\\", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .expect("valid UTF-16 surrogate pair"),
        "🙂"
    );
    let crate::strings::StringDecodeFailure::Invalid(error) =
        crate::test_support::with_service_context(b"\\X2\\D83D\\X0\\", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .expect_err("isolated high surrogate")
    else {
        panic!("invalid string must return its syntax error");
    };
    assert_eq!(error.message, "wide escape contains an isolated surrogate");
    let crate::strings::StringDecodeFailure::Invalid(error) =
        crate::test_support::with_service_context(b"\\X2\\DE42\\X0\\", |input, ctx| {
            crate::strings::decode_with_context(
                input,
                crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
                ctx,
            )
        })
        .expect_err("isolated low surrogate")
    else {
        panic!("invalid string must return its syntax error");
    };
    assert_eq!(error.message, "wide escape contains an isolated surrogate");
}

#[test]
fn decoded_string_text_refuses_retained_limit_before_output_allocation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        crate::strings::decode_with_context(
            b"\xE9",
            crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
            &ctx,
        ),
        Err(crate::strings::StringDecodeFailure::Resource(CodecError::ResourceLimit(limit)))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "step_string_text"
    ));
    let malformed = crate::strings::decode_with_context(
        b"\\X\\GG",
        crate::parse::implementation_level::ImplementationLevel::LegacyEdition1,
        &ctx,
    );
    assert!(matches!(
        malformed,
        Err(crate::strings::StringDecodeFailure::Invalid(error))
            if error.message == "byte escape contains non-hexadecimal digits"
    ));
}

#[test]
fn invalid_step_string_escape_is_reported_as_metadata_loss() {
    let decoded = decode_inline(r"#1=PRODUCT('\X\GG','valid name','',());");

    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::MetadataStringInvalid.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss
                .message
                .contains("STEP record #1 has an invalid product identifier string")
    }));
}

#[test]
fn edition_three_direct_utf8_text_uses_the_file_description_level() {
    let mut source = b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('test'),'4;1');\nFILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n#1=PRODUCT('P\xC3\xA9','N\xC3\xB8','',());\nENDSEC;\nEND-ISO-10303-21;\n".to_vec();
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(&mut source), &DecodeOptions::default())
        .expect("decode edition-three UTF-8 product");
    let product = decoded
        .ir()
        .model
        .product_definitions
        .first()
        .expect("product definition");
    assert_eq!(product.source_name.as_deref(), Some("Nø"));
    assert_eq!(product.part_number.as_deref(), Some("Pé"));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message.contains("invalid product identifier string")
            || loss.message.contains("invalid product name string")
    }));
}

#[test]
fn edition_three_utf8_and_legacy_escape_fixtures_keep_the_same_text() {
    let mut edition_three = Cursor::new(&include_bytes!("tests/data/el01_edition3_utf8.p21")[..]);
    let decoded_edition_three = StepCodec::default()
        .decode(&mut edition_three, &DecodeOptions::default())
        .expect("decode edition-three UTF-8 fixture");
    let product = decoded_edition_three
        .ir()
        .model
        .product_definitions
        .first()
        .expect("edition-three product");
    assert_eq!(product.part_number.as_deref(), Some("Pé"));
    assert_eq!(product.source_name.as_deref(), Some("Nø"));

    let mut legacy = Cursor::new(&include_bytes!("tests/data/el01_legacy_escaped.p21")[..]);
    let decoded_legacy = StepCodec::default()
        .decode(&mut legacy, &DecodeOptions::default())
        .expect("decode legacy escaped fixture");
    let product = decoded_legacy
        .ir()
        .model
        .product_definitions
        .first()
        .expect("legacy product");
    assert_eq!(product.part_number.as_deref(), Some("Pé"));
    assert_eq!(product.source_name.as_deref(), Some("Nø"));
}

#[test]
fn legacy_direct_single_byte_text_uses_cadir_iso_8859_1_salvage() {
    // `0xE9` is outside the legacy direct repertoire; this pins the documented
    // recovery path for malformed historical input.
    let mut source = b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('test'),'3;1');\nFILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n#1=PRODUCT('P\xE9','N','',());\nENDSEC;\nEND-ISO-10303-21;\n".to_vec();
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(&mut source), &DecodeOptions::default())
        .expect("decode legacy ISO-8859-1 product");
    let product = decoded
        .ir()
        .model
        .product_definitions
        .first()
        .expect("product definition");
    assert_eq!(product.part_number.as_deref(), Some("Pé"));
}
