# Viewport performance

Measured with the control channel (`ui.inspect` `frame_ms`: CPU time of one UI frame, which
includes picking under the cursor) on the sample plate patterned 20 × 20 (400 bodies, about
600k triangles), 1600 × 1000 window under Xvfb, release build. Hover: 60 cursor moves across
the model; orbit: a right-drag.

| | hover median | hover p90 | orbit median |
|---|---|---|---|
| before | 18.7 ms | 37.5 ms | 21.0 ms |
| after | 1.9 ms | 2.4 ms | 2.6 ms |
| after, Browser hidden | 0.4 ms | — | 0.4 ms |

What changed:
- **Picking culls by bounding box.** Per-body boxes are cached until the model changes. The
  face raycast skips bodies whose box the ray misses or enters behind the nearest hit so far.
  Edge and vertex tests skip bodies whose box, projected on screen, is not under the cursor.
- **Picks are reused** while the cursor, camera and model are unchanged, and **no picking runs
  while the view moves** (orbit, pan, zoom).
- **The model extent is cached** (it sets the view's depth range and was recomputed from every
  mesh several times per frame).

With 49 bodies the frame was already 2.8 ms (hover) before these changes. The remaining frame
time with the Browser shown is mostly the Browser listing every body row. GPU ID-buffer picking
was not needed at this size.

## Browser

Same model and method (400 bodies, Bodies folder open, 60 hover moves over the viewport):

| | median | p90 |
|---|---|---|
| Browser shown, before | 1.95 ms | 2.71 ms |
| Browser shown, after | 0.72 ms | 1.19 ms |
| Browser hidden | 0.30 ms | 0.72 ms |

What changed: rows scrolled out of view only reserve their height. They register no widgets
and paint nothing (no text layout, icons or eye/fold buttons), so the cost follows the rows on
screen rather than the rows in the tree. Scrolling, Find in Browser and drag and drop still see
every row's place.

# Recompute performance

`solvecraft-cli bench-edit <script> <param> "(<old>) * 1.02"` builds a design from its script
(timing the build), then times a parameter edit, setting the parameter back to its old
expression, undo and redo. Each cell is before → after the evaluation cache, with the number of
features recomputed in parentheses. Release build, on a machine shared with other build jobs (load average about 45; the
times vary by ±50 % run to run, so compare the recompute counts and the zeros). "Early" edits a parameter of the
first sketch or feature, "late" one of the last feature.

| Design | Edit | Build | Edit | Set back | Undo | Redo |
|---|---|---|---|---|---|---|
| sample plate, 20 × 20 pattern | early (`width`) | 7587 ms (10) → 1764 ms (10) | 7969 ms (10) → 1691 ms (10) | 7557 ms (10) → 0 ms (0) | 6534 ms (0) → 0 ms (0) | 4570 ms (0) → 0 ms (0) |
| sample plate, 20 × 20 pattern | late (`d9`) | 7870 ms (10) → 1699 ms (10) | 3944 ms (1) → 812 ms (1) | 4039 ms (1) → 0 ms (0) | 1464 ms (0) → 0 ms (0) | 1398 ms (0) → 0 ms (0) |
| oracle 58 phone stand | early (`d1`) | 163 ms (6) → 140 ms (6) | 228 ms (5) → 140 ms (5) | 171 ms (5) → 0 ms (0) | 158 ms (0) → 0 ms (0) | 130 ms (0) → 0 ms (0) |
| oracle 58 phone stand | late (`d4`) | 144 ms (6) → 195 ms (6) | 118 ms (1) → 118 ms (1) | 145 ms (1) → 0 ms (0) | 149 ms (0) → 0 ms (0) | 187 ms (0) → 0 ms (0) |
| oracle 59 enclosure base | late (`d8`) | 11582 ms (11) → 11179 ms (11) | 5772 ms (1) → 5742 ms (1) | 5606 ms (1) → 0 ms (0) | 6023 ms (0) → 0 ms (0) | 6119 ms (0) → 0 ms (0) |
| oracle 60 pipe flange | early (`d1`) | 8137 ms (7) → 8348 ms (7) | 7569 ms (6) → 10143 ms (6) | 8354 ms (6) → 0 ms (0) | 8607 ms (0) → 0 ms (0) | 10034 ms (0) → 0 ms (0) |
| oracle 60 pipe flange | late (`d6`) | 7661 ms (7) → 11990 ms (7) | 67 ms (1) → 361 ms (1) | 69 ms (1) → 0 ms (0) | 84 ms (0) → 0 ms (0) | 66 ms (0) → 0 ms (0) |
| oracle 61 shaft keyway | early (`d1`) | 1723 ms (6) → 3016 ms (6) | 1785 ms (5) → 2745 ms (5) | 1760 ms (5) → 2323 ms (5) | 1667 ms (0) → 0 ms (0) | 1719 ms (0) → 0 ms (0) |
| oracle 61 shaft keyway | late (`d4`) | 1905 ms (6) → 2247 ms (6) | 1734 ms (1) → 2101 ms (1) | 1682 ms (1) → 0 ms (0) | 1719 ms (0) → 0 ms (0) | 1876 ms (0) → 0 ms (0) |
| oracle 62 heat sink | early (`d1`) | 382 ms (6) → 643 ms (6) | 362 ms (5) → 697 ms (5) | 233 ms (5) → 568 ms (5) | 245 ms (0) → 0 ms (0) | 360 ms (0) → 0 ms (0) |
| oracle 62 heat sink | late (`d5`) | 479 ms (6) → 688 ms (6) | 270 ms (1) → 657 ms (1) | 321 ms (1) → 0 ms (0) | 413 ms (0) → 0 ms (0) | 283 ms (0) → 0 ms (0) |
| oracle 63 hinge knuckle | early (`d1`) | 1305 ms (8) → 1671 ms (8) | 1329 ms (7) → 2397 ms (7) | 1971 ms (7) → 0 ms (0) | 1705 ms (0) → 0 ms (0) | 1321 ms (0) → 0 ms (0) |
| oracle 63 hinge knuckle | late (`d4`) | 1430 ms (8) → 2072 ms (8) | 1387 ms (1) → 2073 ms (1) | 1480 ms (1) → 0 ms (0) | 1192 ms (0) → 0 ms (0) | 1035 ms (0) → 0 ms (0) |

What changed: every feature evaluation is cached by a key made of the feature's own inputs
(definition and parameter values), the document context it may read (construction planes,
sheet metal and plastic rules, components) and a digest of the model it starts from, in which
each body is identified by the evaluation that produced it (bodies a feature passes through
unchanged keep their identity). The same feature on the same inputs is never evaluated twice:
setting a value back, undo, redo, a preview followed by the same command, and the repeated
evaluations while a script builds the design all reuse earlier results. The cache is shared by a
model and its copies (previews, scratch sessions) and keeps the last 4096 results.

An edit still recomputes the edited feature and every later feature whose starting model
changed; in these parts every later feature works on the edited body, so early edits recompute
almost everything, as they must. `cached_evaluation_matches_from_scratch` checks that cached and
from-scratch evaluation give the same models (bodies, volumes and areas to 1e-6, topology,
errors) through edits, suppression, roll-back, undo and redo. Setting a sketch dimension back
can still recompute (oracle 61, 62): solving the sketch moves its points by rounding noise, so
its inputs are not exactly the old ones. Oracle 59's first parameter edit fails (a recipe
expression the bench's `* 1.02` can't scale) and is left out.
