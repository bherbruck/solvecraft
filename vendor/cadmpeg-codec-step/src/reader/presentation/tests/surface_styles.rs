// SPDX-License-Identifier: Apache-2.0

use crate::loss::StepLossCode;
use crate::test_support::exchange::decode_inline;

#[test]
fn invalid_surface_side_is_opaque_and_does_not_transfer_color() {
    let result = decode_inline(
        "#1=COLOUR_RGB('invalid side',1.,0.,0.);
#2=SURFACE_STYLE_RENDERING(#1,$,$,$,$,$);
#3=SURFACE_SIDE_STYLE('',(#2));
#4=SURFACE_STYLE_USAGE(.SIDE_NOT_IN_SCHEMA.,#3);
#5=PRESENTATION_STYLE_ASSIGNMENT((#4));
#6=STYLED_ITEM('',(#5),#7);
#7=(ADVANCED_FACE() FACE_SURFACE());",
    );

    assert!(result.ir().model.appearances.is_empty());
    assert!(result.ir().model.appearance_bindings.is_empty());
    assert!(result
        .ir()
        .model
        .faces
        .iter()
        .all(|face| face.color.is_none()));
    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    for id in [
        "step:data:colour_rgb#1",
        "step:data:surface_style_rendering#2",
        "step:data:surface_side_style#3",
        "step:data:surface_style_usage#4",
        "step:data:presentation_style_assignment#5",
        "step:data:styled_item#6",
    ] {
        assert!(
            unknowns.iter().any(|record| record.id.as_str() == id),
            "missing {id}"
        );
    }
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::SurfaceSideInvalid.kind()
            && loss.message.contains("SURFACE_STYLE_USAGE #4")
            && loss.message.contains(".SIDE_NOT_IN_SCHEMA.")
            && loss.message.contains("style omitted")
    }));
}

#[test]
fn invalid_surface_side_does_not_block_a_valid_sibling_style() {
    let result = decode_inline(
        "#1=COLOUR_RGB('invalid side',1.,0.,0.);
#2=SURFACE_STYLE_RENDERING(#1,$,$,$,$,$);
#3=SURFACE_SIDE_STYLE('',(#2));
#4=SURFACE_STYLE_USAGE(.SIDE_NOT_IN_SCHEMA.,#3);
#5=COLOUR_RGB('valid side',0.,1.,0.);
#6=SURFACE_STYLE_RENDERING(#5,$,$,$,$,$);
#7=SURFACE_SIDE_STYLE('',(#6));
#8=SURFACE_STYLE_USAGE(.POSITIVE.,#7);
#9=PRESENTATION_STYLE_ASSIGNMENT((#4,#8));
#10=STYLED_ITEM('',(#9),#11);
#11=(ADVANCED_FACE() FACE_SURFACE());",
    );

    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].base_color,
        Some(cadmpeg_ir::topology::Color::new(0.0, 1.0, 0.0, 1.0).expect("valid color"))
    );
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    for id in [
        "step:data:colour_rgb#1",
        "step:data:surface_style_rendering#2",
        "step:data:surface_side_style#3",
        "step:data:surface_style_usage#4",
    ] {
        assert!(
            unknowns.iter().any(|record| record.id.as_str() == id),
            "missing {id}"
        );
    }
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::SurfaceSideInvalid.kind()
            && loss.message.contains("SURFACE_STYLE_USAGE #4")
    }));
}
