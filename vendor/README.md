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
- `transversal/integrate/mod.rs`: the result's shells and faces keep the order of the shell
  they come from (truck's connected components followed hash order of face addresses, so a
  boolean's faces came out in a different order on every run).
- `lib.rs`: `tr!` reports where an operation gives up when `SHAPEOPS_TRACE` is set; compiler
  warnings no longer fail the build.

## truck-stepio 0.3.0 (Apache-2.0, © RICOS Co. Ltd., https://github.com/ricosjp/truck)

Only the writer (`out`) is used; SolveCraft reads STEP with its own reader.

Changes:
- `out/geometry.rs`: an `INTERSECTION_CURVE`'s second surface was written at the first
  surface's entity index, so every boolean-made edge produced duplicate entity ids.
- `out/geometry.rs`: surfaces of revolution carry the right `same_sense` for inverted and
  mirrored revolutions (ISO 10303-42 parameterises them by angle then profile, so their normal
  is the opposite of truck's; a mirroring transform reverses it again), and the modeling-surface
  wrapper passes the revolution's sense through instead of always `true`.
- `lib.rs`: compiler warnings no longer fail the build. Example and test targets are dropped
  (their sources are not vendored).

## truck-meshalgo 0.4.0 (Apache-2.0, © RICOS Co. Ltd., https://github.com/ricosjp/truck)

Changes:
- `tessellation/triangulation.rs`: meshed faces and edges are built unchecked: faces whose loop
  runs along a seam edge both ways and closed edges (a full circle on one vertex) are valid,
  but the debug-build checks rejected them (debug builds could not mesh them).
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
