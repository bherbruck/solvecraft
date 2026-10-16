// SPDX-License-Identifier: Apache-2.0
//! Resolution of a write request against the source: the synthesis catalog and
//! the two refusals.

#![allow(clippy::unwrap_used)]

use cadmpeg_test_support::wire;

use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder, ExportPlan};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::examples::unit_cube;

use crate::codec::StepCodec;
use crate::options::{StepSchema, StepWriteOptions};

/// A decoded STEP document whose `FILE_SCHEMA` declares `identifier`.
///
/// The DATA section is the same cube in every case, written at AP242 edition 3
/// and then re-declared, because the reader has one Part 21 grammar and never
/// branches on `FILE_SCHEMA`. That isolates the axis under test: these documents
/// differ in declared identity and in nothing else.
fn source_declaring(identifier: &str) -> cadmpeg_ir::codec::DecodeResult {
    let written = StepSchema::Ap242Edition3;
    let mut bytes = Vec::new();
    crate::export::write_step(
        &unit_cube().expect("unit cube fixture is admitted"),
        &mut bytes,
        written,
        &StepWriteOptions::default(),
    )
    .expect("write the cube at AP242 edition 3");
    let text = String::from_utf8(bytes).expect("the writer emits 7-bit output");
    assert_eq!(
        text.matches(written.file_schema()).count(),
        1,
        "the schema identifier must occur once, so the re-declaration is exact"
    );
    let text = text.replace(written.file_schema(), identifier);
    StepCodec::default()
        .decode(
            &mut Cursor::new(text.into_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode the re-declared exchange")
}

fn inherit(encoder: &StepCodec, ir: &CadIr) -> Result<ExportPlan, cadmpeg_core::CodecError> {
    encoder.plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
}

fn written_text(plan: ExportPlan) -> String {
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write the planned export");
    String::from_utf8(bytes).expect("the writer emits 7-bit output")
}

fn target_of(plan: &ExportPlan) -> Option<String> {
    wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
        plan.report(),
        "identity/target",
    )
    .as_ref()
    .map(ToString::to_string)
}

/// Encoder planning always returns its typed loss rows, even when a direct
/// `write_step` caller configured the legacy strict-write option.
#[test]
fn planning_reports_unrepresentable_content_under_strict_write_options() {
    let mut ir = CadIr::empty();
    ir.native.namespace_mut("f3d").arenas_mut().insert(
        "asm_histories".into(),
        vec![cadmpeg_ir::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new("f3d:test:asm-history#0").expect("valid identity"),
            serde_json::Map::default(),
        )
        .expect("valid native identity")],
    );
    ir.finalize(&cadmpeg_test_support::service_decode_context())
        .expect("fixture ordering is admitted");
    let encoder = StepCodec {
        options: StepWriteOptions {
            ..StepWriteOptions::default()
        },
    };

    let plan = encoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("planning returns the representable subset and its losses");
    assert!(
        plan.report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("source-native record(s)")),
        "{:?}",
        plan.report().losses
    );
}

fn refusal(
    error: &CodecError,
) -> (
    &str,
    Option<String>,
    &'static [cadmpeg_core::target::TargetDescriptor],
) {
    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("expected a target refusal, got {error}");
    };
    (
        refusal.format(),
        {
            let wire = serde_json::to_value(refusal).expect("serialize refusal");
            wire["refusal"]
                .get("requested")
                .or_else(|| wire["refusal"].get("source"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        },
        refusal.available(),
    )
}

/// The flagship case: `convert in.step -o out.step` on a file that is not the
/// catalog default keeps the schema it was handed.
///
/// The catalog default is AP214, exactly what the command line writes when no
/// dialect is named. Taking it under `Inherit` is the defect: this AP203
/// edition 1 file would come back declaring `AUTOMOTIVE_DESIGN`, a different
/// application protocol, with the report claiming success. The resolution reads
/// the source instead, and the emitted `FILE_SCHEMA` and the reported target are
/// checked separately so a report that agreed with neither the request nor the
/// bytes still fails.
#[test]
fn inherit_synthesizes_the_source_schema_not_the_catalog_default() {
    let decoded = source_declaring(StepSchema::Ap203Edition1.file_schema());
    assert_eq!(
        decoded
            .ir()
            .source
            .as_ref()
            .unwrap()
            .dialect()
            .map(cadmpeg_core::dialect::DialectMatch::dialect)
            .map(ToString::to_string),
        Some("step:ap203-e1".to_owned())
    );

    let encoder = StepCodec::default();
    assert_eq!(
        Encoder::targets(&encoder)
            .default()
            .map(|(_, target)| target.id.as_str()),
        Some("step:ap214")
    );
    let plan = inherit(&encoder, decoded.ir()).expect("an AP203 edition 1 source is a catalog row");

    assert_eq!(target_of(&plan), Some("step:ap203-e1".to_owned()));
    let text = written_text(plan);
    assert!(
        text.contains("FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'))"),
        "{text}"
    );
    assert!(!text.contains("AUTOMOTIVE_DESIGN"), "{text}");
}

/// Every catalog row inherits itself, so the flagship is not one lucky schema.
#[test]
fn inherit_round_trips_the_declaration_of_every_catalog_row() {
    for schema in StepSchema::ALL {
        let decoded = source_declaring(schema.file_schema());
        let plan = inherit(&StepCodec::default(), decoded.ir())
            .unwrap_or_else(|error| panic!("{schema:?} is a catalog row, got {error}"));
        assert_eq!(
            target_of(&plan),
            Some(schema.descriptor().id.as_str().to_owned())
        );
        assert!(
            written_text(plan).contains(schema.file_schema()),
            "{schema:?}"
        );
    }
}

/// An AP242 source that declares no edition refuses under `Inherit`, naming both
/// its own dialect and the catalog.
///
/// The object identifier is optional in Part 21, so this file's declaration is
/// complete and says the edition is unspecified. Every schema this writer emits
/// stamps arcs, so synthesizing edition 3 would make the output declare an
/// edition the input never did — a re-decode of it classifies `step:ap242-e3`,
/// a different registry row. That is the silent change of what the file is that
/// the resolution forbids.
#[test]
fn inherit_refuses_an_edition_unspecified_ap242_source() {
    let decoded = source_declaring("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF");
    assert_eq!(
        decoded
            .ir()
            .source
            .as_ref()
            .unwrap()
            .dialect()
            .map(cadmpeg_core::dialect::DialectMatch::dialect)
            .map(ToString::to_string),
        Some("step:ap242".to_owned())
    );

    let error = inherit(&StepCodec::default(), decoded.ir())
        .expect_err("an edition-unspecified AP242 source has no write target");
    let (format, requested, available) = refusal(&error);
    assert_eq!(format, "step");
    assert_eq!(requested.as_deref(), Some("step:ap242"));
    for schema in StepSchema::ALL {
        assert!(
            available
                .iter()
                .any(|target| target.id.as_str() == schema.descriptor().id.as_str()),
            "{available:?}"
        );
    }
}

/// A declaration the registry does not name lands on `step:unknown`, the
/// totality row, which is also read-side only. `Inherit` refuses it for the same
/// reason: there is no schema to preserve.
#[test]
fn inherit_refuses_an_unrecognized_source_declaration() {
    let decoded = source_declaring("NOT_A_DECLARED_SCHEMA");
    assert_eq!(
        decoded
            .ir()
            .source
            .as_ref()
            .unwrap()
            .dialect()
            .map(cadmpeg_core::dialect::DialectMatch::dialect)
            .map(ToString::to_string),
        Some("step:unknown".to_owned())
    );

    let error =
        inherit(&StepCodec::default(), decoded.ir()).expect_err("step:unknown has no write target");
    let (_, requested, available) = refusal(&error);
    assert_eq!(requested.as_deref(), Some("step:unknown"));
    assert!(
        available
            .iter()
            .any(|target| target.id.as_str() == "step:ap214"),
        "{available:?}"
    );
}

/// A STEP source that records no dialect refuses as well. Nothing names an
/// identity to preserve, and picking one would be inventing it.
///
/// The refusal quotes no dialect id, because none exists. It used to pass the
/// bare format id `"step"` in the dialect-id field the command line renders,
/// which reads as a request for a dialect called `step`.
#[test]
fn inherit_refuses_a_step_source_that_records_no_dialect() {
    let mut ir = CadIr::empty();
    ir.source = Some(
        serde_json::from_value(serde_json::json!({
            "identity": {"classification": "unclassified", "format": crate::dialect::FORMAT},
            "attributes": {},
        }))
        .unwrap(),
    );

    let error = inherit(&StepCodec::default(), &ir)
        .expect_err("a STEP source with no dialect has nothing to preserve");
    let (format, requested, available) = refusal(&error);
    assert_eq!(format, "step");
    assert_eq!(requested.as_deref(), None);
    assert!(
        available
            .iter()
            .any(|target| target.id.as_str() == "step:ap214"),
        "{available:?}"
    );
    let message = error.to_string();
    assert!(message.contains("records no dialect"), "{message}");
    assert!(
        !message.contains("cannot write step:"),
        "the refusal must not quote a dialect id it does not have: {message}"
    );
}

/// The same source writes the moment the caller names a target: an explicit
/// request is always the escape from an inherit refusal, and it is how an
/// edition gets chosen for an edition-unspecified file — deliberately, by the
/// caller.
#[test]
fn an_explicit_target_writes_a_source_the_catalog_cannot_inherit() {
    let decoded = source_declaring("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF");
    let plan = StepCodec::default()
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit("step:ap242-e3"),
        )
        .expect("an explicit catalog row writes");

    assert_eq!(target_of(&plan), Some("step:ap242-e3".to_owned()));
    assert!(
        written_text(plan).contains(StepSchema::Ap242Edition3.file_schema()),
        "the explicit edition is declared with its arcs"
    );
}

/// An explicit target beats the source's own schema, so a deliberate schema
/// change is still one flag away.
#[test]
fn an_explicit_target_overrides_the_source_schema() {
    let decoded = source_declaring(StepSchema::Ap203Edition1.file_schema());
    let plan = StepCodec::default()
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit("step:ap242-e2"),
        )
        .expect("an explicit catalog row writes");

    assert_eq!(target_of(&plan), Some("step:ap242-e2".to_owned()));
}

/// Cross-format conversion into STEP writes the catalog default. The command
/// line builds `Explicit(default)` when the formats differ, because there is
/// nothing to inherit; the default is AP214.
#[test]
fn a_cross_format_conversion_writes_the_catalog_default() {
    let encoder = StepCodec::default();
    let (_, default) = Encoder::targets(&encoder)
        .default()
        .expect("the catalog has a default");
    assert_eq!(default.id.as_str(), "step:ap214");

    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.source = Some(SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("rhino:archive-50"),
        )),
        std::collections::BTreeMap::new(),
    ));
    let plan = encoder
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(default.id.as_str()),
        )
        .expect("the catalog default writes");

    assert_eq!(target_of(&plan), Some("step:ap214".to_owned()));
    assert!(written_text(plan).contains("AUTOMOTIVE_DESIGN"));
}

/// With nothing STEP to inherit — no source at all, or one of another format —
/// the catalog default is the target.
///
/// Encoder state deciding a target was the defect: it was a fourth answer to
/// "which dialect", and it was the one that overrode the other three whenever a
/// request had nothing to inherit. `StepWriteOptions` now carries no schema at
/// all, so the fourth answer cannot be spelled; what remains under test is that
/// the answer is the catalog's.
#[test]
fn nothing_to_inherit_falls_to_the_catalog_default() {
    let encoder = StepCodec::default();
    let (_, default) = Encoder::targets(&encoder)
        .default()
        .expect("the catalog has a default");
    assert_eq!(default.id.as_str(), "step:ap214");

    let sourceless = unit_cube().expect("unit cube fixture is admitted");
    let plan = inherit(&encoder, &sourceless).expect("a sourceless document takes the default");
    assert_eq!(target_of(&plan), Some("step:ap214".to_owned()));
    assert!(written_text(plan).contains("AUTOMOTIVE_DESIGN"));

    let mut foreign = unit_cube().expect("unit cube fixture is admitted");
    foreign.source = Some(SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("iges:5.3-fixed-ascii"),
        )),
        std::collections::BTreeMap::new(),
    ));
    let plan = inherit(&encoder, &foreign).expect("a foreign source takes the default");
    assert_eq!(target_of(&plan), Some("step:ap214".to_owned()));
}

/// An explicit target that is not the source schema charges a displacement loss.
#[test]
fn a_dialect_changing_explicit_write_charges_displacement_by_name() {
    let decoded = source_declaring(StepSchema::Ap203Edition1.file_schema());
    let plan = StepCodec::default()
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit("step:ap214"),
        )
        .expect("AP214 is a catalog row");
    assert_eq!(
        wire::field::<cadmpeg_ir::report::export::FidelityResolution>(
            plan.report().write_path(),
            "fidelity"
        ),
        cadmpeg_ir::report::export::FidelityResolution::NotProvided {}
    );
    let loss = plan
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == crate::loss::StepLossCode::SourceDialectDisplaced.kind())
        .expect("schema displacement is charged");
    assert!(loss.message.contains("step:ap203-e1"));
    assert!(loss.message.contains("step:ap214"));
}

/// An explicit target that is the source's own schema changes nothing, so it is
/// not degraded.
#[test]
fn an_explicit_write_at_the_source_dialect_is_not_degraded() {
    let decoded = source_declaring(StepSchema::Ap203Edition1.file_schema());
    let plan = StepCodec::default()
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit("step:ap203-e1"),
        )
        .expect("AP203 edition 1 is a catalog row");
    assert_eq!(
        wire::field::<cadmpeg_ir::report::export::FidelityResolution>(
            plan.report().write_path(),
            "fidelity"
        ),
        cadmpeg_ir::report::export::FidelityResolution::NotProvided {}
    );
}

/// The catalog is exactly the six schemas the writer emits, in both directions,
/// and each is reachable by its alias as well as its id. The read-side identity
/// rows — `step:ap242`, `step:unknown`, and the three alternate encodings — are
/// deliberately absent: no request makes this writer produce them.
#[test]
fn the_catalog_is_the_schemas_the_writer_emits() {
    let encoder = StepCodec::default();
    let targets = Encoder::targets(&encoder);
    assert_eq!(targets.len(), StepSchema::ALL.len());
    for schema in StepSchema::ALL {
        let (_, target) = targets
            .find(schema.descriptor().id.as_str())
            .unwrap_or_else(|| panic!("{schema:?} has no catalog row"));
        assert!(!target.aliases.is_empty(), "{schema:?}");
        for alias in target.aliases {
            assert_eq!(
                targets.find(alias).map(|(_, row)| &row.id),
                Some(&target.id)
            );
        }
    }
    for target in targets {
        assert!(
            StepSchema::ALL
                .into_iter()
                .any(|schema| schema.descriptor().id.as_str() == target.id.as_str()),
            "catalog row {} names no schema",
            target.id
        );
    }
    for id in [
        "step:ap242",
        "step:unknown",
        "step:part28-xml",
        "step:ap242-bo-model-xml",
        "step:part26-hdf5",
    ] {
        assert!(targets.find(id).is_none(), "{id}");
    }
}

/// The §8.3 honesty invariant on the synthesis path: re-decoding the output
/// classifies the host layer into exactly the dialect the report named.
///
/// The assertion is against the bytes, not against the report twice, and not
/// against a substring of `FILE_SCHEMA` text. `target` is a claim about what
/// was written, and the only thing that can check a claim about bytes is
/// reading them back through the classifier the codec uses on any other input.
/// A substring check would still pass for a writer that emitted a declaration
/// the registry classifies as another row: the bare AP242 MIM identifier
/// without its object-identifier arcs is exactly such a case, and it classifies
/// `step:ap242`, not any edition.
#[test]
fn every_synthesized_target_re_decodes_as_the_dialect_the_report_named() {
    let cube = unit_cube().expect("unit cube fixture is admitted");
    for schema in StepSchema::ALL {
        let plan = StepCodec::default()
            .plan(
                EncodeInput::new(&cube, None),
                TargetRequest::Explicit(schema.descriptor().id.as_str()),
            )
            .unwrap_or_else(|error| panic!("{schema:?} is a catalog row, got {error}"));
        let claimed = wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            plan.report(),
            "identity/target",
        )
        .clone()
        .expect("a STEP write always names its schema");
        let mut written = Vec::new();
        plan.write_to(&mut written).expect("the plan writes");

        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{schema:?} output must decode, got {error}"));
        let classified = decoded
            .report()
            .dialects()
            .unwrap_or_else(|| panic!("{schema:?} output must classify a host dialect"))
            .primary()
            .dialect()
            .clone();
        assert_eq!(
            classified, claimed,
            "{schema:?}: the report claims {claimed} but the bytes are {classified}"
        );
    }
}
