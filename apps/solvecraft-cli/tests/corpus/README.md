# Design file corpus

Designs saved by older SolveCraft builds. `tests/design_corpus.rs` opens each one with the
current build and checks that it evaluates as it did when saved (the `.expected.json` next to
it is `solvecraft-cli eval` of that file by the build that saved it: bodies, volumes, areas,
world bounding boxes), that it is upgraded to the current file format, and that damaged and
hostile variants of it never panic.

| File | Saved by | Era |
|---|---|---|
| `m0-box-fillet-hole` | 3714342 | M0: sketch, parameters, extrude, fillet, cut |
| `m0-revolve-primitives` | 3714342 | M0: revolve, primitives, combine, chamfer, timeline edit |
| `components-arm` | 97d03e3 | components and occurrences: move, copy, Paste New, ground |
| `joints-hinge` | c2309eb | joints: revolute hinge driven by a parameter |
| `modern-sheet-metal` | bd98035 | sheet metal contour flange (oracle 72) |
| `modern-path-pattern` | bd98035 | pattern on path (oracle 53) |
| `modern-plastic` | bd98035 | shell, lip, boss, rest, snap fit, plastic rule |

The scripts that made them are in `scripts/`. Never regenerate a file with a newer build: the
point is that it was written by the old one. Add new files for new eras instead.
