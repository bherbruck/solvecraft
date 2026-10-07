# SolveCraft roadmap

Status as of **2026-10-07**. SolveCraft targets Fusion-style parametric modelling (sketch →
features → parametric timeline) as an open, pure-Rust, agent-drivable application.

## At a glance

| Question | Answer |
|---|---|
| Where are we? | **M0 done; M1/M2 in progress.** Sketch with solver, extrude (taper, two-sided, through all), revolve, fillet/chamfer, holes, patterns, mirror, shell, draft, loft, sweep, split, construction planes, parameters that rebuild the timeline, STEP/STL export, desktop and browser (wasm) app with Fusion-style selection (hover/selected highlights, origin planes, click-to-pick sketch planes, multi-select, window/crossing box selection, animated view cube), CLI, MCP server, oracle harness. |
| Command parity (in scope) | **139 / 541 (26%)** in-scope commands; SOLID + SKETCH **102 / 291 (35%)** — [docs/parity.md](docs/parity.md) |
| Fusion oracle | **38 / 66 parts match** (batch 1: 29/29; batch 2 in progress) — [docs/oracle.md](docs/oracle.md) |
| Tests | 85 (solver, profiles, kernel booleans/blends/measures, expressions, timeline, file formats, camera, engine end-to-end, hostile-input fuzz over every command) |
| Gates | `cargo xtask ci`: fmt, clippy -D warnings, tests, asset attribution, layering, wasm32 build — green |
| Weighted parity estimate | **≈ 5%** of Fusion's Design workspace by importance (sketch + basic solids are the core, but surfaces, assemblies, sheet metal, CAM, drawings are untouched) |
| Time to a useful alpha (M0–M6) | ≈ **150 agent hours** remain |

## Scope (owner decision, 2026-10-07)

In scope, in priority order:
1. SOLID and SKETCH (now).
2. Parameters / functional constraints: user parameters with units and expressions (+-*/, ^,
   parentheses, sin/cos/sqrt/min/max/floor/ceil/round, unit conversion), model parameters
   d1, d2… for every dimension and feature input, references between parameters with a
   dependency graph and cycle detection, re-evaluation on change, comments, favourites,
   CSV/JSON import and export.
3. Components and assemblies with joints (M9).
4. SHEET METAL: flange, contour flange, bend, unfold/refold, flat pattern with DXF export, sheet
   metal rules (thickness, K-factor, bend radius) (M10).
5. A small PLASTIC subset for enclosures, after sheet metal: Boss, Rib, Web, Lip/Groove, Snap Fit.

Deferred until further notice: MESH, FORM (T-splines), SURFACE beyond what solids need, PCB / 3D
PCB / Package, Render, Animation, Simulation, Manufacture/CAM, Drawings. The parity report's
headline counts in-scope tabs only and lists the deferred ones separately.

## Milestones

| # | Milestone | Status | Hours left |
|---|---|---|---|
| M0 | Vertical slice | done | — |
| M1 | Sketch depth: done — projection/intersect/include as linked geometry (auto-project on faces, lost-reference warnings, break link), full SKETCH create/modify set (slots, polygons, tangent arcs/circles, ellipse, fit/control splines, conic, text, trim/extend/break, fillet/chamfer, offset, mirror, patterns, move/scale, blend curve, centerline), constraints incl. curvature and polygon with over-constraint refusal, driven/arc-length/linear-diameter dimensions, AutoConstrain, DXF/SVG insert. Left: 3D sketch curves (Project To Surface, Intersection Curve, Isoparametric Curve, Spun Profile), exact (non-faceted) free-form profiles | in progress | 8 |
| M2 | Robust modelling: coplanar booleans in the kernel, holes (placed on faces and on sketch points), threads (cosmetic, then modelled), patterns, mirror, shell, draft, split, construction geometry | in progress | 16 |
| M3 | General fillets and chamfers (curved edges, chains, vertex blends) | planned | 30 |
| M4 | Files: STEP import (done: all 29 Fusion STEP files — [docs/step-import.md](docs/step-import.md)), 3MF export and 3MF/STL import as mesh bodies (done), DXF sketches, OBJ import | in progress | 6 |
| M5 | Persistent naming of faces and edges | planned | 20 |
| M6 | UI depth: selection feedback and origin (done), timeline editing (done: edit feature, history marker drag, reorder with dependency checks, suppress, rename, delete with dependents, re-resolved references), live previews, measure tool, section view, drag-solve | in progress | 16 |
| M7 | Sweep, loft, rib, web, emboss, thread, coil, pipe | planned | 24 |
| M8 | Surface workspace | deferred | — |
| M9 | Components and assemblies (Fusion-style): New Component / Create Components from Bodies, active component, per-component origin/bodies/sketches/timeline entries, occurrences with transforms, Copy/Paste vs Paste New, nested Browser tree with visibility and isolate; Ground, Move/Copy of occurrences, joints (rigid, revolute, slider, cylindrical, pin-slot, planar, ball) by joint origins with limits and Drive Joints, As-built Joint, Rigid Group, contact sets later; interference, per-component physical properties; STEP with assembly structure. Groundwork done: component tree, occurrences with transforms (instances, Paste New), world placement, active component as authoring frame, ground, Capture/Revert Position, moving bodies and sketches | in progress | 32 |
| M10 | Sheet metal: flange, contour flange, bend, unfold/refold, flat pattern + DXF, rules (thickness, K-factor, bend radius); then the PLASTIC enclosure subset (Boss, Rib, Web, Lip/Groove, Snap Fit) | planned | 36 |
| M11 | 2D drawings | deferred | — |
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
- STEP import (our own reader, crates/kernel/src/step_in; all 29 Fusion STEP files pass
  `cargo xtask step-corpus`) reads solids (B-rep with analytic and B-spline geometry), units,
  product names, colours and assemblies; offset surfaces and pcurve-only edges are not read yet,
  and assembly components are flattened into bodies. Closed periodic faces (torus, B-spline
  bands) are split in two for meshing; measure() uses a finer tolerance for small radii.
- No persistent naming (fillet edges are re-found by position; projected sketch geometry too).
- Sketch free-form curves (ellipses, splines, conics, text) enter profiles as polylines, so
  features built on them are faceted; sketches are planar (no 3D sketch curves yet).
- Shell works on planar bodies (convex or not), draft on convex planar bodies; loft is ruled.

## Kernel

truck (Apache-2.0) behind our own `kernel` crate, with our own blend operation and boolean
retry/validation; see `plan/adr/0001-geometry-kernel.md` (local) — summary in README.
