// SPDX-License-Identifier: Apache-2.0
use super::{pcurve_admission_note, PcurveAdmission, PCURVE_UNPROVED_NOTE_EXEMPLARS};

fn admissions(count: usize) -> Vec<PcurveAdmission> {
    (0..cadmpeg_core::decode::u64_from_index(count))
        .map(|index| PcurveAdmission {
            curve: index,
            surface: 100 + index,
            coedge_use: 200 + index,
        })
        .collect()
}

/// The warning names the first `PCURVE_UNPROVED_NOTE_EXEMPLARS` relations in
/// decode order and gives the number of relations it does not name.
#[test]
fn admission_warning_names_the_bounded_exemplars_and_counts_the_rest() {
    let extra = 4;
    let note = crate::test_support::with_service_context(b"", |_, ctx| {
        pcurve_admission_note(&admissions(PCURVE_UNPROVED_NOTE_EXEMPLARS + extra), ctx)
    })
    .expect("warning fits policy")
    .expect("recorded admissions give a warning");
    let message = note.message.as_str();

    assert!(
        message.contains(&format!(
            "admits {} pcurve relation(s)",
            PCURVE_UNPROVED_NOTE_EXEMPLARS + extra
        )),
        "{message}"
    );
    for index in 0..cadmpeg_core::decode::u64_from_index(PCURVE_UNPROVED_NOTE_EXEMPLARS) {
        assert!(
            message.contains(&format!(
                "curve #{index} on surface #{} at coedge use #{}",
                100 + index,
                200 + index
            )),
            "{message}"
        );
    }
    for index in cadmpeg_core::decode::u64_from_index(PCURVE_UNPROVED_NOTE_EXEMPLARS)
        ..cadmpeg_core::decode::u64_from_index(PCURVE_UNPROVED_NOTE_EXEMPLARS + extra)
    {
        assert!(
            !message.contains(&format!("curve #{index} on surface")),
            "{message}"
        );
    }
    assert!(
        message.ends_with(&format!(", and {extra} more")),
        "{message}"
    );
}

/// A count at the exemplar bound names every relation and counts no remainder.
#[test]
fn admission_warning_at_the_exemplar_bound_names_every_relation() {
    let note = crate::test_support::with_service_context(b"", |_, ctx| {
        pcurve_admission_note(&admissions(PCURVE_UNPROVED_NOTE_EXEMPLARS), ctx)
    })
    .expect("warning fits policy")
    .expect("recorded admissions give a warning");
    let message = note.message.as_str();

    assert!(
        message.contains(&format!(
            "admits {PCURVE_UNPROVED_NOTE_EXEMPLARS} pcurve relation(s)"
        )),
        "{message}"
    );
    assert!(
        message.ends_with(&format!(
            "curve #{} on surface #{} at coedge use #{}",
            PCURVE_UNPROVED_NOTE_EXEMPLARS - 1,
            100 + PCURVE_UNPROVED_NOTE_EXEMPLARS - 1,
            200 + PCURVE_UNPROVED_NOTE_EXEMPLARS - 1
        )),
        "{message}"
    );
}

/// A document with no admitted relation reports no admission warning.
#[test]
fn no_admitted_relation_reports_no_admission_warning() {
    assert!(crate::test_support::with_service_context(b"", |_, ctx| {
        pcurve_admission_note(&[], ctx)
    })
    .expect("empty warning fits")
    .is_none());
}

#[test]
fn pcurve_warning_refuses_retained_text_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        assert!(matches!(pcurve_admission_note(&admissions(12), ctx),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && refusal.operation == "step_pcurve_admission_note"));
    });
}

#[test]
fn pcurve_warning_retains_exact_text_without_intermediate_buffers() {
    crate::test_support::with_service_context(b"", |_, ctx| {
        let note = pcurve_admission_note(&admissions(1), ctx)
            .expect("warning fits")
            .expect("one admission");
        assert_eq!(note.message, "a finite endpoint and locus witness admits 1 pcurve relation(s); global model-space point-set equality and direction are unproved: curve #0 on surface #100 at coedge use #200");
    });
}
