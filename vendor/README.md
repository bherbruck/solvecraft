# Vendored third-party crates

Copies of third-party crates that SolveCraft builds with local fixes, wired in with
`[patch.crates-io]` in the workspace `Cargo.toml`. They keep their own licence; changed lines
are marked `SolveCraft:` in the source.

## truck-shapeops 0.4.0 (Apache-2.0, © RICOS Co. Ltd., https://github.com/ricosjp/truck)

Changes:
- `transversal/integrate/mod.rs`: mesh both shells with `robust_triangulation` (faces made by
  earlier booleans have boundaries that lie on their surface only within tolerance).
- `transversal/divide_face/mod.rs`: when projecting a loop onto a face's surface, fall back to
  the nearest parameter when the exact search fails (intersection curves are approximations).
- `lib.rs`: `tr!` reports where an operation gives up when `SHAPEOPS_TRACE` is set; compiler
  warnings no longer fail the build.
