# Performance

## Large sketches

Measured with `cargo test -p solvecraft-engine --release sketch_perf -- --ignored --nocapture`
(`crates/engine/src/cmd/sketch_perf_tests.rs`). The sketch holds `n` rectangles (four lines,
horizontal/vertical constraints and two length dimensions each) and `n` circles. All times are
wall-clock milliseconds on the development machine while other builds were running (load
average around 60), so treat them as upper bounds.

| n | curves | points | constraints | solve | profiles | draw lists | snap query | add a line (command) | drag a point (command) |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 250 | 1 250 | 1 251 | 1 500 | 15 | 30 | 12 | 0.1 | 68 | 48 |
| 500 | 2 500 | 2 501 | 3 000 | 35 | 47 | 10 | 0.1 | 123 | 100 |
| 1000 | 5 000 | 5 001 | 6 000 | 65 | 134 | 1 | 0.3 | 232 | 100 |

"Add a line" and "drag a point" are whole commands: they copy the sketch, solve it, store it and
re-evaluate the timeline (a second solve, plus profile finding). "Draw lists" is
`view::sketch_lines`, the polylines the viewport draws.

Before the 2026-10-08 changes the n = 1000 sketch took 1 280 ms to solve, 590 ms to find
profiles and 5.1 s per command:

- The solver built one dense Jacobian over every variable of the sketch, for each connected
  component's iterations and again for the degree-of-freedom analysis (5 000 × 10 000 entries,
  then reduced row echelon form). It now builds each component's Jacobian over that component's
  own variables. The sketch's Jacobian is block diagonal, so its rank is the sum of the
  components' ranks, and components were already solved separately.
- Grouping variables into components searched a list per variable. It now uses a map.
- Profile finding looked at every curve end for each crossing it found, and sorted profiles with
  a linear id lookup in the comparison. It now uses the grid it already builds for crossings,
  and sorts on precomputed keys.

Projection (linked reference geometry) is cached per model state, so edits to a sketch with
projected silhouettes or sections on a ~20 000-triangle model cost about 3 ms each instead of
about 80 ms.
