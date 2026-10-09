# SolveCraft roadmap

Status as of **2026-10-07**. SolveCraft targets Fusion-style parametric modelling (sketch →
features → parametric timeline) as an open, pure-Rust, agent-drivable application.

## At a glance

| Question | Answer |
|---|---|
| Where are we? | **M0 done; M1/M2 in progress.** Sketch with solver, extrude (taper, two-sided, through all), revolve, fillet/chamfer, holes, patterns, mirror, shell, draft, loft, sweep, split, construction planes, parameters that rebuild the timeline, STEP/STL export, desktop and browser (wasm) app with Fusion-style selection (hover/selected highlights, origin planes, click-to-pick sketch planes, multi-select, window/crossing box selection, animated view cube), CLI, MCP server, oracle harness. |
| Command parity (in scope) | **139 / 541 (26%)** in-scope commands; SOLID + SKETCH **102 / 291 (35%)** — [docs/parity.md](docs/parity.md) |
| Fusion oracle | **72 / 76 parts match** (batch 1: 29/29; batch 2 in progress) — [docs/oracle.md](docs/oracle.md) |
| Tests | 85 (solver, profiles, kernel booleans/blends/measures, expressions, timeline, file formats, camera, engine end-to-end, hostile-input fuzz over every command) |
| Gates | `cargo xtask ci`: fmt, clippy -D warnings, tests, asset attribution, layering, wasm32 build — green |
| Weighted parity estimate | **≈ 5%** of Fusion's Design workspace by importance (sketch + basic solids are the core, but surfaces, assemblies, sheet metal, CAM, drawings are untouched) |
| Time to a useful alpha (M0–M6) | ≈ **150 agent hours** remain |

## Scope (owner decision, 2026-10-07)

In scope, in priority order:
1. SOLID and SKETCH (now).
2. Parameters / functional constraints (done: see `parameters.*`, `expr.evaluate`; unit-aware,
   cycles reported with their path, rename follows references, deletes list their users):
   user parameters with units and expressions (+-*/, ^,
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
| M1 | Sketch depth: done — projection/intersect/include as linked geometry (auto-project on faces, lost-reference warnings, break link), full SKETCH create/modify set (slots, polygons, tangent arcs/circles, ellipse, fit/control splines, conic, text, trim/extend/break, fillet/chamfer, offset, mirror, patterns, move/scale, blend curve, centerline), constraints incl. curvature and polygon with over-constraint refusal, driven/arc-length/linear-diameter dimensions, AutoConstrain, DXF/SVG insert, 3D sketch curves (Project To Surface, Intersection Curve, Isoparametric Curve, Spun Profile), exact free-form profiles (B-spline/NURBS edges, any spline degree, curves cut by crossings), sweeps and pipes along 3D sketch curves, canvases and decals, curvature combs | done | 4 |
| M2 | Robust modelling: coplanar booleans in the kernel, holes (placed on faces and on sketch points), threads (cosmetic, then modelled), patterns, mirror, shell, draft, split, construction geometry | in progress | 16 |
| M3 | General fillets and chamfers (curved edges, chains, vertex blends) | planned | 30 |
| M4 | Files: STEP import (done: all 29 Fusion STEP files — [docs/step-import.md](docs/step-import.md)), 3MF export and 3MF/STL import as mesh bodies (done), DXF sketches, OBJ import | in progress | 6 |
| M5 | Persistent naming of faces and edges (plan/adr/0002): faces named from their history (sketch curve sides, caps, revolve sections, primitive sides, blends, pattern/mirror copies; split pieces numbered), edges by their faces; fillets/chamfers, sketches on faces, shell/draft/offset/replace faces, hole placements, joint origins and face appearances resolve by name first and warn when the entity split or vanished: done. sheet metal edges by base sketch line and flange side, shell inner faces, sweep and loft roles: done. Left: kernel-reported face history (exact through coincident surfaces) | in progress | 20 |
| M6 | UI depth: selection feedback and origin (done), timeline editing (done: edit feature, history marker drag, reorder with dependency checks, suppress, rename, delete with dependents, re-resolved references), right-docked dialogs (done), live previews for every feature dialog (done), keyboard-first commands with on-canvas value boxes and sketch inline dimensions (done), drag manipulators with automatic join/cut (done: extrude incl. two sides, fillet, shell, hole depth, move, pattern spacing, revolve rotator; taper handle open), dark theme (done), right-click menu with Repeat (done), measure tool (done), section analysis (done), selection filter (done), drag-solve of sketch points (done); appearances, preferences | in progress | 6 |
| M7 | Sweep, loft (incl. to a point), rib and web (to next / depth), emboss (planar faces), thread (cosmetic), coil (smooth B-spline helical walls), pipe, pattern on path, patterns of bodies and components, replace face (parallel), align, remove, feature copy/paste: done. Left: emboss on curved faces, thicken (needs surfaces), split face, modelled threads | in progress | 12 |
| M8 | Surface workspace | deferred | — |
| M9 | Components and assemblies (Fusion-style): New Component / Create Components from Bodies, active component, per-component origin/bodies/sketches/timeline entries, occurrences with transforms, Copy/Paste vs Paste New, nested Browser tree with visibility and isolate; Ground, Move/Copy of occurrences, joints (rigid, revolute, slider, cylindrical, pin-slot, planar, ball) by joint origins with limits and Drive Joints, As-built Joint, Rigid Group, Contact Sets (driving and motion studies stop where bodies in a set would pass through each other), Motion Study (joint values keyed along a timeline, playback, export of occurrence positions as JSON/CSV), Exploded views (automatic or by hand, shown without changing placements); interference, per-component physical properties; STEP with assembly structure. Groundwork done: component tree, occurrences with transforms (instances, Paste New), world placement, active component as authoring frame, ground, Capture/Revert Position, moving bodies and sketches. UI: ASSEMBLE panel dialogs for New Component, Joint (snaps on faces, edges, circles and vertices; animated motion preview; offsets, flip, limits), As-built Joint, Joint Origin, Rigid Group, Drive Joints (live slider), Motion Link and Interference (overlaps shown red); joints in the timeline and the Browser | in progress | 32 |
| M10 | Sheet metal: flange (base, edge, contour), hem, unfold/refold, convert, flat pattern + DXF, rules: done (oracle 67-76). PLASTIC enclosure subset: Boss, Rib, Web, Lip/Groove, Snap Fit, Rest, Plastic Rules (library, edit, assign; rule draft and clearance as feature defaults): done, no Fusion oracle (licence), tested against hand-computed geometry. UI: SHEET METAL and PLASTIC tabs in Fusion's layout with dialogs (live preview, Edit Feature, pre-selection) for Flange, Hem, Unfold/Refold, Convert, Sheet Metal Rules, Create Flat Pattern (flat view with bend lines, bend table, DXF export), Boss, Lip/Groove, Snap Fit, Rest, Manage and Assign Plastic Rules; Bend (Fold) has no engine command yet | in progress | 36 |
| M11 | 2D drawings | deferred | — |
| M12 | Release: installers, signing, docs, performance | planned | 16 |
| M14 | Configurations: a table of rows (parameter expressions and feature suppressions per row) switched with config.activate (undoable, all or nothing), kept through parameter renames and feature deletes (config.configure, config.table, config.column/row/activate). Left: configuration rules, per-row materials and appearances | in progress | 4 |
| M13 | Never lose work: atomic saves (temporary file, flush, rename; a crash at any moment leaves the old file or the new one) with the replaced version kept as `<file>.bak`; autosave of unsaved changes to the recovery folder every 5 minutes (preference) and after big operations, only when something changed; "Recover unsaved design?" on the next launch (`doc.recovery_list`, `doc.recover`, `doc.recovery_discard`); a kill-mid-save test (process killed while saving a 0.5 MB design 25 times). File format: `"format": "solvecraft/2"`, older formats upgraded step by step (`doc::format`), newer ones refused with a message, out-of-range numbers and broken placements refused; a corpus of designs saved by M0, components-era, joints-era and current builds (apps/solvecraft-cli/tests/corpus) that must open and evaluate as saved; damaged and hostile variants (truncation, bit flips, hostile values, removed keys, repeated features, deep nesting) never panic | done | 6 |

## Known limitations

- Fillets and chamfers: straight edges (convex or concave) between planar faces with perpendicular
  planar end faces; whole smooth loops of a planar face, picked whole or by one edge (tangent
  chain), including circles and walls at any angle (boss bases and tops, bores, pocket floors,
  plate outlines), loops with sharp corners between straight edges (the blends meet in mitres), runs of
  neighbouring edges along a planar face's loop (two top edges of a box: mitred, ending square;
  with the edge between them too, the corner becomes a sphere octant),
  rounded corners rounded again (sphere octants, tori at concave corners); every edge of a
  convex planar body or of an extruded part; fillets inside patterns; closed chains of curved
  edges between any two smooth sides (a branch pipe on a main pipe), by a rolling ball (the
  blend a rational B-spline through exact arcs), closed or ending at planar faces square to the
  edges.
- Booleans: prisms along one direction (plates with holes, slots, patterns of them, equal
  sections stacked end to end) are combined exactly in 2D; bodies touching along one whole face
  (a part and its mirror image) are stitched; coincident planar faces are pushed apart when
  their neighbours are planes or cylinders. Other coincident curved configurations can still
  fail. Fully internal voids are not supported. Spheres are built from six pole-free patches.
- Mass properties come from fine tessellation (curved faces within ~1e-4 relative); triangles
  that cut through curved faces (trimmed faces after booleans) are split onto the surface first.
- STEP import (our own reader, crates/kernel/src/step_in; all 29 Fusion STEP files pass
  `cargo xtask step-corpus`) reads solids (B-rep with analytic and B-spline geometry), units,
  product names, colours and assemblies; offset surfaces and pcurve-only edges are not read yet,
  and assembly components are flattened into bodies. Closed periodic faces (torus, B-spline
  bands) are split in two for meshing; measure() uses a finer tolerance for small radii.
- No persistent naming (fillet edges are re-found by position; projected sketch geometry too).
- Free-form curves cut at a tangent touch (no clean crossing) enter that profile as polylines.
  Sweeps along 3D sketch curves follow their polyline (smooth walls when the path is smooth).
- Shell and offset faces work on bodies of planes, cylinders, cones and spheres (convex or not:
  filleted boxes, bosses, domes, countersinks); draft on convex planar bodies and on walls (planes, cylinders → cones) between caps; lofts through three or more sections are smooth (B-spline sides through every section), two
  sections ruled.

## Kernel

truck (Apache-2.0) behind our own `kernel` crate, with our own blend operation and boolean
retry/validation; see `plan/adr/0001-geometry-kernel.md` (local) — summary in README.
