// SPDX-License-Identifier: Apache-2.0
//! Shared STEP Part 21 exchange builders for crate tests.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::{CadIr, DecodeFailure};

use crate::export::write_step;
use crate::{StepCodec, StepSchema, StepWriteOptions};

pub(crate) fn export(ir: &CadIr) -> String {
    let mut buf = Vec::new();
    write_step(
        ir,
        &mut buf,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("write");
    String::from_utf8(buf).expect("utf8")
}

pub(crate) fn decode_inline(records: &str) -> cadmpeg_ir::codec::DecodeResult {
    decode_inline_result(records).expect("decode inline STEP")
}

pub(crate) fn decode_inline_result(
    records: &str,
) -> Result<cadmpeg_ir::codec::DecodeResult, DecodeFailure> {
    let source = format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('test'),'2;1');\nFILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n{records}\nENDSEC;\nEND-ISO-10303-21;\n"
    );
    StepCodec::default().decode(&mut Cursor::new(source), &DecodeOptions::default())
}

pub(crate) fn equivalent_seam_source() -> String {
    String::from_utf8(include_bytes!("../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
            "#57=SEAM_CURVE('',#16,(#56,#69),.PCURVE_S1.);",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=PCURVE('',#28,#70);\n#70=DEFINITIONAL_REPRESENTATION('',(#71),#50);\n#71=LINE('',#51,#53);\nENDSEC;\nEND-ISO-10303-21;",
        )
}
