# cadmpeg-codec-step

`cadmpeg-codec-step` reads and writes ISO 10303-21 Part 21 exchange structures for
AP203 editions 1–2, AP214, and AP242 editions 1–3. [`StepCodec::plan`] resolves
the requested [`StepSchema`] from the encoder catalog. [`StepCodec`] implements both
[`Codec`] decode and [`Encoder`] write. The cadmpeg CLI uses the same model.

<!-- generated: capability step -->

Support: L9 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#step-part-21)).

<!-- /generated: capability step -->

## Install

```sh
cargo add cadmpeg-codec-step cadmpeg-ir
```

## Decode a Part 21 file

```rust,no_run
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_codec_step::StepCodec;
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open("part.step")?;
    let result = StepCodec::default().decode(&mut input, &DecodeOptions::default())?;

    for loss in &result.report().losses {
        eprintln!("{:?}: {}", loss.severity, loss.message);
    }
    println!("{} bodies", result.ir().model.bodies.len());
    Ok(())
}
```

Decode transfers millimeter-normalized analytic and NURBS geometry, connected
solid, sheet, and wire topology with pcurves, product identity and occurrences,
AP242 tessellation where present, layers, colors, visibility, and PMI. Named
opaque application records retain identity and byte spans when retained.

## Write a document

Use [`StepCodec`] through [`Encoder::plan`]. The plan resolves and validates a
catalog target before producing bytes and an export report.

The encoder emits the Part 21 envelope for the named schema, product and
representation context, and reachable shape. Coverage includes solid, sheet,
and wire bodies plus standalone geometry; coedge pcurves; rigid body placement;
products and occurrences; AP242 tessellation; visibility; layers; named colors;
and semantic or presentation PMI where the target application protocol carries
them. Supported surface carriers are planes, cylinders, cones, spheres, tori,
and rational or non-rational NURBS surfaces. Supported curve carriers are lines,
circles, ellipses, parabolas, hyperbolas, and rational or non-rational NURBS
curves. The writer preserves shared carriers by reusing STEP instances.

## Units and metadata

Coordinates are written without rescaling and the representation context
declares millimetres. Supply geometry in millimetres before export. The context
uses the IR linear tolerance as its uncertainty value; plane and solid angles
use radians and steradians.

[`StepWriteOptions`] controls `FILE_NAME` metadata; the target [`StepSchema`] is
selected by `Encoder::plan`, not stored in the options. An empty timestamp produces `1970-01-01T00:00:00`, which keeps
default output deterministic. The first body name, when present, supplies the
STEP product name. `product_name` supplies the `FILE_NAME` name instead.

## Losses and errors

The writer exports representable geometry and records reductions in
[`ExportReport::losses`]. Review these notes before accepting the file.

In particular:

- faces on unknown surfaces and edges without typed 3D curves are omitted;
- non-rigid body transforms and non-identity root occurrence placements report
  losses;
- coedge pcurves emit their geometry; native-only pcurve metadata is not
  represented in STEP;
- textures, shaders, source attributes, and retained opaque records report
  losses;
- unsupported procedural definitions emit their solved carrier with a
  machine-readable loss;
- signed sphere radii and nonstandard torus minor radii are normalized where
  required by the emitted STEP entity.

An empty or fully unrepresentable model still produces a syntactically complete
file with an empty geometric representation and a warning. [`StepError`] covers
[`StepError::Io`] from the output sink. Because output is streamed, an I/O
failure can leave a partial file.

[`ExportReport::census`] groups DATA instances by entity keyword, and
[`EntityCensus::total`] gives the complete DATA instance count.
Inspect [`ExportReport::losses`] for each loss and its severity;
lower-severity losses still require caller review.

## References

- [API documentation][docs]
- [Format support][support]
- [Architecture and crate map][architecture]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

[architecture]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/architecture.md
[docs]: https://docs.rs/cadmpeg-codec-step
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
[support]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#step-part-21
[`CadIr`]: https://docs.rs/cadmpeg-ir/latest/cadmpeg_ir/document/struct.CadIr.html
[`Codec`]: https://docs.rs/cadmpeg-ir/latest/cadmpeg_ir/codec/trait.Codec.html
[`Encoder`]: https://docs.rs/cadmpeg-ir/latest/cadmpeg_ir/codec/write/trait.Encoder.html
[`EntityCensus::total`]: https://docs.rs/cadmpeg-ir/latest/cadmpeg_ir/report/struct.EntityCensus.html#method.total
[`ExportReport::census`]: https://docs.rs/cadmpeg-ir/latest/cadmpeg_ir/report/struct.ExportReport.html#structfield.census
[`ExportReport::losses`]: https://docs.rs/cadmpeg-ir/latest/cadmpeg_ir/report/struct.ExportReport.html#structfield.losses
[`StepCodec`]: https://docs.rs/cadmpeg-codec-step/latest/cadmpeg_codec_step/struct.StepCodec.html
[`StepError`]: https://docs.rs/cadmpeg-codec-step/latest/cadmpeg_codec_step/enum.StepError.html
[`StepError::Io`]: https://docs.rs/cadmpeg-codec-step/latest/cadmpeg_codec_step/enum.StepError.html#variant.Io
[`StepSchema`]: https://docs.rs/cadmpeg-codec-step/latest/cadmpeg_codec_step/enum.StepSchema.html
[`StepWriteOptions`]: https://docs.rs/cadmpeg-codec-step/latest/cadmpeg_codec_step/struct.StepWriteOptions.html
[`std::io::Write`]: https://doc.rust-lang.org/std/io/trait.Write.html
