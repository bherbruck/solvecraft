# SolveCraft roadmap

Status as of **2026-10-07**. SolveCraft targets Fusion-style parametric modelling (sketch →
features → parametric timeline) as an open, pure-Rust, agent-drivable application.

## At a glance

| Question | Answer |
|---|---|
| Where are we? | **M0 done; M1/M2 in progress.** Sketch with solver, extrude (taper, two-sided, through all), revolve, fillet/chamfer, holes, patterns, mirror, shell, draft, loft, sweep, split, construction planes, parameters that rebuild the timeline, STEP/STL export, desktop and browser (wasm) app with Fusion-style selection (hover/selected highlights, origin planes, click-to-pick sketch planes, multi-select, window/crossing box selection, animated view cube), CLI, MCP server, oracle harness. |
| Command parity (SOLID + SKETCH toolbar) | **57 / 291 (20%)** — [docs/parity.md](docs/parity.md) |
| Fusion oracle | **29 / 29 parts match** Fusion's measurements — [docs/oracle.md](docs/oracle.md) |
| Tests | 85 (solver, profiles, kernel booleans/blends/measures, expressions, timeline, file formats, camera, engine end-to-end, hostile-input fuzz over every command) |
| Gates | `cargo xtask ci`: fmt, clippy -D warnings, tests, asset attribution, layering, wasm32 build — green |
| Weighted parity estimate | **≈ 5%** of Fusion's Design workspace by importance (sketch + basic solids are the core, but surfaces, assemblies, sheet metal, CAM, drawings are untouched) |
| Time to a useful alpha (M0–M6) | ≈ **150 agent hours** remain |

## Milestones

| # | Milestone | Status | Hours left |
|---|---|---|---|
| M0 | Vertical slice | done | — |
| M1 | Sketch depth, oracle sketch cases (arcs, taper, two-sided, sketch modify tools) | in progress | 16 |
| M2 | Robust modelling: coplanar booleans in the kernel, holes (placed on faces and on sketch points), threads (cosmetic, then modelled), patterns, mirror, shell, draft, split, construction geometry | in progress | 16 |
| M3 | General fillets and chamfers (curved edges, chains, vertex blends) | planned | 30 |
| M4 | Files: STEP import (done: all 29 Fusion STEP files — [docs/step-import.md](docs/step-import.md)), 3MF export and 3MF/STL import as mesh bodies (done), DXF sketches, OBJ import | in progress | 6 |
| M5 | Persistent naming of faces and edges | planned | 20 |
| M6 | UI depth: selection feedback and origin (done), timeline editing (done: edit feature, history marker drag, reorder with dependency checks, suppress, rename, delete with dependents, re-resolved references), live previews, measure tool, section view, drag-solve | in progress | 16 |
| M7 | Sweep, loft, rib, web, emboss, thread, coil, pipe | planned | 24 |
| M8 | Surface workspace | planned | 30 |
| M9 | Components and assemblies (Fusion-style): New Component / Create Components from Bodies, active component, per-component origin/bodies/sketches/timeline entries, occurrences with transforms, Copy/Paste vs Paste New, nested Browser tree with visibility and isolate; Ground, Move/Copy of occurrences, joints (rigid, revolute, slider, cylindrical, pin-slot, planar, ball) by joint origins with limits and Drive Joints, As-built Joint, Rigid Group, contact sets later; interference, per-component physical properties; STEP with assembly structure. Groundwork done: the document is a component tree (features and bodies carry a component, active component, New Component, Create Components from Bodies) | planned | 40 |
| M10 | Sheet metal | planned | 30 |
| M11 | 2D drawings | planned | 30 |
| M12 | Release: installers, signing, docs, performance | planned | 16 |

## Known limitations

- Fillets and chamfers: straight edges (convex or concave) between planar faces with perpendicular
  planar end faces; whole smooth loops of a planar face (pocket floors, plate outlines); every edge
  of a convex planar body (sphere corners). Other corner configurations are not supported yet.
- Coincident planar faces are pushed apart exactly when their neighbours stand perpendicular
  (extruded and box-like parts); other coincident configurations can still fail. Fully internal
  voids are not supported. Spheres are built from six pole-free patches so they
  combine reliably.
- Mass properties come from fine tessellation (curved faces within ~1e-4 relative).
- STEP import reads solids (B-rep with analytic and B-spline geometry), units, product names,
  colours and assemblies; offset surfaces and pcurve-only edges are not read yet, and assembly
  components are flattened into bodies (the tree is kept for M9).
- No persistent naming (fillet edges are re-found by position).
- Shell and draft work on convex bodies with planar faces; loft is ruled (no tangency end conditions).
- Dialogs have no live preview of the result yet.

## Kernel

truck (Apache-2.0) behind our own `kernel` crate, with our own blend operation and boolean
retry/validation; see `plan/adr/0001-geometry-kernel.md` (local) — summary in README.
