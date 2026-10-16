// SPDX-License-Identifier: Apache-2.0
//! Document report for pcurve relations that the finite witness admits.
//!
//! The B-rep walker collects the located relations as facts. This module owns
//! the presentation policy: how many relations the one document warning names,
//! and the text of that warning.

use std::fmt;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::StepLossCode;

/// Number of admitted relations that the document admission warning names.
const PCURVE_UNPROVED_NOTE_EXEMPLARS: usize = 8;

/// One admitted pcurve relation, by the three source records that locate it.
pub(super) struct PcurveAdmission {
    pub(super) curve: u64,
    pub(super) surface: u64,
    pub(super) coedge_use: u64,
}

/// The one document warning, or `None` when no relation is admitted.
///
/// Every admitted relation shares one class of unproved invariant, so the
/// warning gives the count, names the first [`PCURVE_UNPROVED_NOTE_EXEMPLARS`]
/// relations in decode order, and gives the number it does not name.
pub(super) fn pcurve_admission_note(
    admissions: &[PcurveAdmission],
    ctx: &DecodeContext<'_>,
) -> Result<Option<LossNote>, CodecError> {
    if admissions.is_empty() {
        return Ok(None);
    }
    let named = admissions.iter().take(PCURVE_UNPROVED_NOTE_EXEMPLARS).len();
    ctx.charge_work(
        u64_from_index(named).checked_mul(2).ok_or_else(|| {
            ctx.refuse_codec_limit("step_pcurve_admission_note", u64::MAX, u64::MAX)
        })?,
        "step_pcurve_admission_note",
    )?;
    let message = ctx.format_retained(
        format_args!("{}", AdmissionWarning(admissions)),
        "step_pcurve_admission_note",
    )?;
    Ok(Some(
        StepLossCode::PcurveGlobalFidelityUnproved.note(message),
    ))
}

struct AdmissionWarning<'a>(&'a [PcurveAdmission]);

impl fmt::Display for AdmissionWarning<'_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self.0.len();
        write!(output, "a finite endpoint and locus witness admits {count} pcurve relation(s); global model-space point-set equality and direction are unproved: ")?;
        for (index, admission) in self
            .0
            .iter()
            .take(PCURVE_UNPROVED_NOTE_EXEMPLARS)
            .enumerate()
        {
            if index != 0 {
                output.write_str(", ")?;
            }
            write!(
                output,
                "curve #{} on surface #{} at coedge use #{}",
                admission.curve, admission.surface, admission.coedge_use
            )?;
        }
        if let Some(unnamed) = count
            .checked_sub(PCURVE_UNPROVED_NOTE_EXEMPLARS)
            .filter(|count| *count > 0)
        {
            write!(output, ", and {unnamed} more")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
