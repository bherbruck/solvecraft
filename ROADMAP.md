# SolveCraft roadmap

Status as of **2026-10-07**. SolveCraft targets Fusion-style parametric modelling (sketch →
features → parametric timeline) as an open, pure-Rust, agent-drivable application.

## At a glance

| Question | Answer |
|---|---|
| Where are we? | **M0 done; M1/M2 in progress.** Sketch with solver, extrude (taper, two-sided, through all), revolve, fillet/chamfer, holes, patterns, mirror, shell, draft, loft, sweep, split, construction planes, parameters that rebuild the timeline, STEP/STL export, desktop app with Fusion-style selection (hover/selected highlights, origin planes, click-to-pick sketch planes, multi-select, window/crossing box selection, animated view cube), CLI, MCP server, oracle harness. |
| Command parity (SOLID + SKETCH toolbar) | **57 / 291 (20%)** — [docs/parity.md](docs/parity.md) |
| Fusion oracle | **23 / 29 parts match** Fusion's measurements — [docs/oracle.md](docs/oracle.md) |
| Tests | 85 (solver, profiles, kernel booleans/blends/measures, expressions, timeline, file formats, camera, engine end-to-end, hostile-input fuzz over every command) |
| Gates | `cargo xtask ci`: fmt, clippy -D warnings, tests, asset attribution, layering — green |
| Weighted parity estimate | **≈ 5%** of Fusion's Design workspace by importance (sketch + basic solids are the core, but surfaces, assemblies, sheet metal, CAM, drawings are untouched) |
| Time to a useful alpha (M0–M6) | ≈ **150 agent hours** remain |

## Milestones

| # | Milestone | Status | Hours left |
|---|---|---|---|
| M0 | Vertical slice | done | — |
| M1 | Sketch depth, oracle sketch cases (arcs, taper, two-sided, sketch modify tools) | in progress | 16 |
| M2 | Robust modelling: coplanar booleans in the kernel, holes (placed on faces and on sketch points), threads (cosmetic, then modelled), patterns, mirror, shell, draft, split, construction geometry | in progress | 16 |
| M3 | General fillets and chamfers (curved edges, chains, vertex blends) | planned | 30 |
| M4 | Files: STEP import, mesh import, DXF sketches, 3MF | planned | 16 |
| M5 | Persistent naming of faces and edges | planned | 20 |
| M6 | UI depth: selection feedback and origin (done), timeline editing (done: edit feature, history marker drag, reorder with dependency checks, suppress, rename, delete with dependents, re-resolved references), live previews, measure tool, section view, drag-solve | in progress | 16 |
| M7 | Sweep, loft, rib, web, emboss, thread, coil, pipe | planned | 24 |
| M8 | Surface workspace | planned | 30 |
| M9 | Assemblies and joints | planned | 40 |
| M10 | Sheet metal | planned | 30 |
| M11 | 2D drawings | planned | 30 |
| M12 | Release: installers, signing, docs, performance | planned | 16 |

## Known limitations

- Fillets and chamfers: straight convex edges between planar faces with perpendicular planar end
  faces; two blended edges may not share a corner yet.
- Booleans of bodies with coincident faces fail unless they come from extrudes (where the tool is
  extended automatically); fully internal voids are not supported.
- Mass properties come from fine tessellation (curved faces within ~1e-4 relative).
- No STEP import yet; no persistent naming (fillet edges are re-found by position).
- Shell and draft work on convex bodies with planar faces; loft is ruled (no tangency end conditions).
- Dialogs have no live preview of the result yet.

## Kernel

truck (Apache-2.0) behind our own `kernel` crate, with our own blend operation and boolean
retry/validation; see `plan/adr/0001-geometry-kernel.md` (local) — summary in README.
