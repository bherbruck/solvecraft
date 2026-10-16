// SPDX-License-Identifier: Apache-2.0
//! Golden snapshot harness for `inspect`, `decode`, and `encode` over the
//! committed fixtures.
//!
//! `tests/fixtures/*.p21` are the frozen inputs.
//! Fixtures stay frozen; `UPDATE_GOLDEN=1` rewrites goldens only.
//! `inspect` pins the container summary; `decode` pins the IR, losses, and
//! source fidelity; `encode` pins the written schema, the `ExportReport`
//! target, and deliberate refusals. Shared harness:
//! [`cadmpeg_test_support::golden`].
//!
//! `inspect` and `decode` run over every fixture. `encode` runs over
//! [`ENCODE_FIXTURES`] only: a written STEP file is far larger than the report
//! that describes it, and four sources already span every target resolution
//! the writer has.

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::golden::{elide_local_digests, snapshot_text, Branch, Harness};

use super::{StepCodec, StepSchema};

/// Extension of the committed fixture inputs.
const FIXTURE_EXTENSION: &str = "p21";

/// Crate-relative regeneration hint used in every failure message.
const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-step golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), FIXTURE_EXTENSION, REGENERATE)
        .with_fixture_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
}

/// The branches every fixture passes through, in golden-directory order.
fn branches() -> [Branch; 2] {
    [
        Branch::named("inspect", inspect_snapshot),
        Branch::named("decode", decode_snapshot),
    ]
}

/// The single encode branch, run over [`ENCODE_FIXTURES`].
fn encode_branch() -> [Branch; 1] {
    [Branch::named("encode", encode_snapshot)]
}

/// Fixture stems the encode branch covers.
///
/// One per way `plan` can resolve a target from a source. `ap203_sheet`
/// declares `CONFIG_CONTROL_DESIGN`, so `Inherit` reproduces a non-default row;
/// `ap214_sheet` declares `AUTOMOTIVE_DESIGN`, so `Inherit` reproduces the
/// catalog default; `ap242_minimal` carries an edition in its schema object
/// identifier; and `ap242_geometry` declares `AP242_..._MIM_LF` with no
/// edition, which names no catalog row and must refuse rather than guess one.
const ENCODE_FIXTURES: [&str; 4] = [
    "ap203_sheet",
    "ap214_sheet",
    "ap242_minimal",
    "ap242_geometry",
];

/// The fixtures in [`ENCODE_FIXTURES`], as harness inputs.
fn encode_inputs() -> Vec<(String, Vec<u8>)> {
    let all = harness().fixture_inputs();
    let selected: Vec<(String, Vec<u8>)> = all
        .into_iter()
        .filter(|(name, _)| ENCODE_FIXTURES.contains(&name.as_str()))
        .collect();
    assert_eq!(
        selected.len(),
        ENCODE_FIXTURES.len(),
        "ENCODE_FIXTURES names a stem with no `.{FIXTURE_EXTENSION}` fixture"
    );
    selected
}

/// Serializes one container summary. An inspect error is frozen too: refusing a
/// container is contract-relevant behavior, so this never panics on codec
/// output.
fn inspect_snapshot(bytes: &[u8]) -> String {
    let value = match StepCodec::default()
        .inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default())
    {
        Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect summary"),
        Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
    };
    snapshot_text(&value)
}

/// Serializes one decoded document: the IR, the decode report, and source
/// fidelity. A decode error is frozen too: refusing a document is
/// contract-relevant behavior, so this never panics on codec output.
fn decode_snapshot(bytes: &[u8]) -> String {
    let value = match StepCodec::default()
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
    {
        Ok(result) => {
            let mut result = cadmpeg_test_support::EditableDecodeResult::from(result);
            if let Some(source) = result.ir_mut().source.as_mut() {
                elide_local_digests(&mut source.attributes);
            }
            serde_json::json!({
                "ir": serde_json::to_value(result.ir()).expect("serialize ir"),
                "report": serde_json::to_value(result.report()).expect("serialize report"),
                "source_fidelity": serde_json::to_value(result.source_fidelity())
                    .expect("serialize source_fidelity"),
            })
        }
        Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
    };
    snapshot_text(&value)
}

/// Decodes `bytes`, then re-encodes it under three target requests.
///
/// `fidelity: None` forces synthesis on every arm, so the output is what the
/// writer builds from neutral IR rather than a replay of the source.
///
/// `inherit` resolves the target from the source declaration and is the arm
/// that varies per fixture. `explicit_default` and `explicit_ap242_e3` name a
/// catalog row outright, so they pin that the written `FILE_SCHEMA` and the
/// reported `ExportReport.target` agree no matter what the source said.
/// Decode and encode refusals freeze as `decode_error` / `encode_error`; a
/// refusal is contract-relevant behavior, so this never panics on codec
/// output.
///
/// The writer reads no clock: `StepWriteOptions::default()` stamps the fixed
/// `1970-01-01T00:00:00` into `FILE_NAME`, so no arm needs elision.
fn encode_snapshot(bytes: &[u8]) -> String {
    let decoded = match StepCodec::default()
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
    {
        Ok(result) => result,
        Err(error) => {
            return snapshot_text(&serde_json::json!({ "decode_error": error.to_string() }))
        }
    };
    let value = serde_json::json!({
        "inherit": encode_arm(&decoded, TargetRequest::Inherit),
        "explicit_default": encode_arm(
            &decoded,
            TargetRequest::Explicit(StepSchema::Ap214.descriptor().id.as_str()),
        ),
        "explicit_ap242_e3": encode_arm(
            &decoded,
            TargetRequest::Explicit(StepSchema::Ap242Edition3.descriptor().id.as_str()),
        ),
    });
    snapshot_text(&value)
}

/// One encode arm: the export report and the bytes the writer produced.
fn encode_arm(
    decoded: &cadmpeg_ir::codec::DecodeResult,
    request: TargetRequest<'_>,
) -> serde_json::Value {
    let outcome = Encoder::plan(
        &StepCodec::default(),
        EncodeInput::new(decoded.ir(), None),
        request,
    )
    .and_then(|plan| {
        let mut produced = Vec::new();
        plan.write_to(&mut produced)
            .map(|report| (report, produced))
    });
    match outcome {
        Ok((report, produced)) => serde_json::json!({
            "report": report,
            "output": String::from_utf8_lossy(&produced),
        }),
        Err(error) => serde_json::json!({ "encode_error": error.to_string() }),
    }
}

/// Every committed golden still matches what the codec produces.
#[test]
fn golden_snapshots_hold() {
    harness().check(&branches());
    harness().check_inputs(&encode_inputs(), &encode_branch());
}

/// Putting the same bytes through a branch twice produces identical text.
#[test]
fn golden_output_is_deterministic() {
    harness().check_determinism(&branches());
    harness().check_determinism_inputs(&encode_inputs(), &encode_branch());
}
