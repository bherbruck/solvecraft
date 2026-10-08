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

## truck-stepio 0.3.0 (Apache-2.0, © RICOS Co. Ltd., https://github.com/ricosjp/truck)

Only the writer (`out`) is used; SolveCraft reads STEP with its own reader.

Changes:
- `out/geometry.rs`: an `INTERSECTION_CURVE`'s second surface was written at the first
  surface's entity index, so every boolean-made edge produced duplicate entity ids.
- `out/geometry.rs`: surfaces of revolution carry the right `same_sense`: truck's revolution
  has the same parameterisation and normal as ISO 10303-42 (the writer flipped it), and a
  mirroring transform reverses the normal.
- `lib.rs`: compiler warnings no longer fail the build. Example and test targets are dropped
  (their sources are not vendored).

## truck-meshalgo 0.4.0 (Apache-2.0, © RICOS Co. Ltd., https://github.com/ricosjp/truck)

Changes:
- `tessellation/triangulation.rs`: parameter searches that land outside a surface's bounded,
  non-periodic domain are rejected (a hinted B-spline search can run off and extrapolate to
  thousands of parameter units, and meshing that grid never finishes); the last-resort nearest
  search is clamped into the domain.
- `tessellation/triangulation.rs`, bounded work (hostile files must not hang): edge polylines
  and surface grids come from our own bounded divisions (at most 20 000 points per edge, a
  grid budget per face shared out of 10 million points per shell, 24 refinement rounds; a
  point that does not evaluate counts as flat instead of recursing). Surface grids refine only
  the direction that bends, and a ruling left undivided gets cells comparable to the other
  direction (long triangles from a trim loop to the grid's far ends cut through cylinders).
- `lib.rs`: compiler warnings no longer fail the build. Example, test and bench targets are
  dropped (their sources are not vendored).
