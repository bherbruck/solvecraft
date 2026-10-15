# Project-opening performance fixture

`HardEdgeBasin.solidcraft` is the native SolveCraft design supplied by the project
owner for this regression. It contains 58 features and produces 14 bodies. It is
kept separately from the historical format corpus: that corpus deliberately
mutates every fixture hundreds of times, which would multiply this benchmark's
expensive geometry rebuilds.

Run `cargo test --profile ci -p solidcraft-cli --test open_performance -- --nocapture`.
The test covers `doc.open`, saved/path state, the complete timeline, world-space
display meshes, and cache reuse on refresh. A child-process deadline bounds a
hang as well as a slowdown. Build time is outside the deadline.

This comparison branch includes endpoint fillet support. It requires zero feature
errors and sets a 60-second cold-open budget; the upstream-only version pins the
eight unsupported endpoint fillets and uses a 20-second budget. Do not suppress
features or ignore additional errors to make the benchmark faster.

For per-feature timings use `solidcraft-cli bench-open` with this fixture.
This is not a Fusion archive; the supplied F3D files remain outside the repository.
