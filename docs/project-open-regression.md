# HardEdgeBasin opening regression, 2026-10-11

The main isolated workspace starts from upstream `4c4a044`. HardEdgeBasin has
58 features and produces 14 bodies. Parsing/repair costs roughly 3 ms.

| Build | Evaluation | Feature errors | Display triangles |
|---|---:|---:|---:|
| Upstream main | 1.8 s | 8 unsupported fillets | 11,180 |
| Installed development binary | 89.5 s | 0 | 11,700 |
| Clean rebuild including fillet commit `c0b1c7f` | 23.0 s | 0 | 11,700 |
| Same fillet source with redundant fitting work removed | 21.5 s | 0 | 11,700 |

These `bench-open` runs were on an M-series Mac under varying build load. They
localize the cost, rather than provide stable microbenchmark statistics. Upstream
rejects endpoint fillets 102–109, so its quick opening is not equivalent output.
Most of the installed binary's slowdown has already been addressed by the latest
fillet changes; the additional optimization is modest on that newer source.

The separate comparison branch reuses the existing sparse spline evaluator for
boundary validation, computes each quadrature grid once, and skips construction
of collocation constraints that transition fits never use. It preserves the
position/tangency checks and control-hull containment certificate. Those changes
are not mixed into the upstream-based F3D branch.

## Regression coverage

```sh
cargo test --profile ci -p solidcraft-cli --test open_performance -- --nocapture
```

The upstream-only test used a 20-second deadline and pinned eight unsupported
feature IDs. With endpoint fillet support, the test runs `doc.open` and display
preparation in a child with a 60-second deadline (excluding compilation). It
checks saved/path state, parameter errors, all 58 evaluated features with zero
errors, and 14 bodies with
nonempty display meshes, zero recomputation on refresh, shared mesh allocations,
and unchanged source bytes. The native fixture is under `tests/performance`,
outside the historical format corpus that mutates every fixture repeatedly.

The upstream test completed in 0.90 s on a later run; that variation is not an
additional claimed optimization. When fillet support lands, change the expected
errors and remeasure the budget, rather than suppressing features or accepting
new failures. The comparison branch requires zero errors with a 60-second budget;
its end-to-end test passed in 33.3 s alongside other builds/geometry tests.

The comparison's nine setback tests passed, and 14/15 endpoint tests passed.
`endpoint_section_cannot_cross_a_hole` fails in its cylinder-cut setup on both
clean `c0b1c7f` and optimized code. This existing geometry failure is independent
of the performance patch. Targeted Clippy checks passed on both workspaces.

## Integrated validation

The performance workspace includes upstream `a094fa4` and the supported fillet
implementation. The isolated opening test passed in 25.34 seconds for open plus
display, with zero feature errors, 14 bodies and 11,700 triangles. This is one
measurement, not a controlled speedup comparison against the earlier runs.

`cargo xtask test` and `ci` run binaries named `*_performance` after all functional
test binaries and side jobs finish, so their timing budgets do not compete with
the same runner's kernel tests or compiler jobs. The CI nextest profile also reserves
all runner CPUs for `open_performance`, isolating its deadline from other tests in
the shard. Geometry acceptance tolerances,
boundary sampling density and the containment certificate are unchanged.

Full CI passed formatting, attribution/licence/layer checks, workspace Clippy,
WASM, and all functional binaries (211 kernel, 548 engine, 261 UI tests). Its
last performance binary could not start because the workspace's build-output
directory had been removed during the run. Rebuilding and rerunning only that
gate passed in 52.22 seconds under concurrent machine load, with zero feature
errors, 14 bodies and 11,726 display triangles. The mesh count varies between
runs; the test checks nonempty meshes and complete feature evaluation rather
than an exact triangulation. These timings must not be read as a controlled
speedup comparison.

The existing endpoint-hole test now uses a planar square-hole tool, preserving
its intended containment assertion while avoiding the unrelated cylinder-cut
setup failure. The focused test passed, followed by the complete kernel suite.

## Corner-flow and display follow-up, 2026-10-13

Closeups exposed two separate problems: the radial position guide could introduce
small repeated waves beside a floor blend, and a chord-only display mesh could
interpolate its surface normals poorly. The fit now gives the interior guide
less weight, integrates curvature within the collapsed chart's knot spans, and
retains the existing boundary position/tangency and control-hull certificates.
Display refinement checks interior normal error and shares finer boundary samples
between adjacent faces. Multi-span cylindrical joins retain their smooth-edge
classification.

The regression tests trace normal flow through perpendicular and sloped corners,
check strips beside the front floor shoulder, and compare interpolated display
normals with the analytic surfaces on small and full-size placed basins. Display
checks exclude the physical meshing floor and the immediate neighborhood of an
original sharp vertex, where the limiting normal depends on approach direction.
They do not exclude those regions from the geometry containment checks.

Profiling the added display work found dense spline searches on cubic transition
faces, including intermediate bodies used for later edge lookup. Those searches
now use the compact-support evaluator, with bounded searches for cubic and
quintic patches. Unchanged triangles reuse their normal-error result across
refinement passes. Profiling the fit then identified repeated indexing in the
banded QR update; matching contiguous slices preserve its arithmetic while
removing that overhead. Boundary acceptance checks remain unchanged.

After integration with upstream `f5ae7205`, a local `bench-open` run measured
49.81 seconds of evaluation plus 3.67 seconds of display preparation, with zero
feature errors, 14 bodies and 98,596 display triangles. The higher triangle count
reflects normal-aware rendering of the corner fills. The 60-second end-to-end
opening guard remains unchanged. This is a local measurement, not a controlled
cross-version benchmark.

On Linux CI after rebasing to `16c49c3e`, the same document measured 105.04 seconds
of evaluation plus 6.95 seconds of display preparation. Independent fan sectors
now fit concurrently through the guarded worker pool. Endpoint dispatch samples
edge curves without tessellating prior fills, and constraint assembly reuses a
sparse accumulator instead of allocating temporary tensor rows and tree nodes.
The coefficient accumulation order, explicit zero entries, QR equations,
quadrature, and acceptance certificates are unchanged. A subsequent Linux run
measured 42.77 seconds of evaluation plus 4.65 seconds of display preparation,
with zero feature errors, 14 bodies and 98,761 triangles. These are separate CI
runs, not a controlled microbenchmark; the opening test still has its 60-second
limit and full document/cache assertions.

Endpoint fills explicitly use the normal-aware reference mesher. The generic
planner can sometimes succeed on their singular charts, but does not perform
the limiting-normal refinement they need. On Linux this routing change reduced
the small sloped-corner test's worst interpolated normal error from 41.47 degrees
to 0.735 degrees, and its display mesh from 79,258 triangles to 19,029. The test's
one-degree error limit and 20,000-triangle budget are unchanged.

The complete opening test subsequently measured 53.40 seconds on an isolated
Linux runner (47.87 seconds in `doc.open`, 5.52 seconds meshing the basin), but
still exceeded 60 seconds on another runner. A same-run `bench-open` measured
52.78 seconds, so it did not expose a different opening path. Profiling identified
repeated scans over zero spline coefficients in projected-map evaluation. The
fit now trims each basis's zero tails before its tensor products, preserving the
coefficient threshold and accumulation order. A regression compares projection
values and constraint coefficients exactly at every supported density, including
endpoint and derivative bases. The opening test reports its `doc.open` phase
separately, retaining its 60-second deadline and all document assertions.

The QR update remained the main fitting cost in the lightly optimized CI kernel.
The CI profile now builds only `solidcraft-kernel` at optimization level 2, as it
already does for third-party numerical dependencies; other workspace crates keep
level 1. With this setting, the complete Linux opening test, including its real
child-process watchdog, passed in 43.20 seconds (38.38 seconds opening, 4.79 seconds
meshing), with zero errors, 14 bodies and 98,761 triangles. Seven focused numerical
and display tests passed in the same run. These are runner-specific measurements,
not a controlled estimate of the compiler setting's speedup.
