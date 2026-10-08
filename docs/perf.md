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
