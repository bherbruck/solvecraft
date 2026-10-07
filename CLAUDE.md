# SolveCraft — instructions for agents

SolveCraft is a clean-room, open-source, pure-Rust parametric 3D CAD application in the style of
Autodesk Fusion: sketches with a constraint solver, a parametric timeline of features, a B-rep
kernel, and a desktop UI that agents can drive. Siblings with the same conventions live next to
it in `storytold/`: `cadcraft` (AutoCAD-style, the closest relative), `photocraft`, `vectorcraft`,
`gridcraft`, `designcraft`, `wordcraft`, `deckcraft`, `soundcraft`. Repos don't share code; when
code is adapted from a sibling (MIT OR Apache-2.0) it is noted in ATTRIBUTION.md.

## Start every session here
1. Read `plan/STATUS.md` (current milestone, next task), then the task in `plan/execution-plan.md`
   and the relevant `plan/architecture.md` section. Behaviour reference: `plan/fusion/`
   (`menu-tree.json` is Fusion's toolbar dump; `oracle/*/` are parts built in Fusion with their
   recipes and measurements).
2. Kernel and library decisions: `plan/adr/0001-geometry-kernel.md` (what we may depend on, what
   is rejected for licence reasons).
3. Follow the autonomous operation protocol (`plan/execution-plan.md` §6). Don't stop to ask
   unless it lists the decision as the owner's.

`plan/` is gitignored (local only).

## Never crash
People trust SolveCraft with their designs; a crash loses their work. **This outranks feature work.**
- **No panics in non-test code:** no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`,
  `unimplemented!`; no `unsafe` (`unsafe_code = "forbid"`).
- **Errors are `Result<T, E>`** through the crate's error type and `?`. An unfinished feature
  returns an error ("not supported yet: …"); it never panics.
- **Input-derived numbers are hostile** (command parameters, design files, control-channel
  input): `get()` not `[i]`, finite checks, caps on sizes and loop counts.
- **Third-party kernel calls are guarded:** everything that calls `truck` goes through
  `solvecraft_kernel::guard`, which turns a panic into `KernelError::Internal`.
- **Last-resort guard:** `Session::execute` runs every command under `catch_unwind`; an escaped
  panic restores the design and reports `EngineError::Internal`.
- **Prove it:** every crash fix lands with a regression test (see `hostile_params_never_panic`).
- Every production crate root carries
  `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.

## Non-negotiables
- **Clean-room.** Fusion may be *observed* black-box (its UI, its toolbar dump, parts built in it
  and their measurements, kept under `plan/fusion/`, never committed). Never copy Autodesk icons,
  artwork, help text, tooltips, templates or materials, and never commit files produced by
  Fusion. Command ids and command names are used as identifiers only (`xtask/data/fusion-catalog.tsv`
  holds ids and names, nothing else). File formats come from public specifications. **Never read,
  copy or link GPL/LGPL/AGPL code**: OpenCASCADE, FreeCAD (incl. planegcs), SolveSpace,
  CadQuery/OCP, LibreCAD, QCAD — don't even read their sources. The sketch solver and the blend
  operations are our own.
- **Assets — absolutely essential.** SolveCraft contains **no Autodesk iconography, images,
  fonts or artwork — ever.** Every icon is drawn in code (`crates/ui-egui/src/icons.rs`). Any
  file asset must be original or openly licensed and **must have a row in ATTRIBUTION.md**
  (`cargo xtask assets` enforces it). Screenshots of Autodesk software are never committed.
  Breaking this rule is the most serious mistake you can make in this repo.
- **Everything is a command.** User-visible behaviour = a command in `crates/engine/src/cmd/*`
  (`CommandSpec`: id = Fusion's command id where Fusion has the command, else a dotted id; label;
  toolbar tab and panel; icon; shortcut; params doc; `enabled`; JSON `run`) + tests. The toolbar,
  dialogs, sketch tools, palette, scripts, CLI and control channel all reach the same commands.
- **Programmatic calls never open dialogs.** `engine.execute` runs the JSON form; only toolbar
  clicks (`ui.start`) start interactive tools and dialogs, which then run commands.
- **Layering** is enforced by `cargo xtask layers`: L0 `geom` → L1 `sketch`, `kernel`, `render`
  → L2 `doc` → L3 `io` → L4 `engine` → L5 `ui-egui` → apps. Nothing below L5 depends on
  egui/eframe/winit/wgpu/rfd. **The UI crate is swappable.** Only `crates/kernel` may use `truck`.
- **The UI is thin**: panels read engine state and act through `app.run(id, params)` /
  `app.start(id)`. Colours come from `theme::Tokens`.
- **Rust only.** Units are millimetres and radians internally; Z is up.
- **Quality gates** before every commit: `cargo xtask ci` (fmt, clippy -D warnings, tests,
  assets, layers). Commit after every feature arc that builds.

## Running and looking at the app
- `cargo run --release -p solvecraft -- --sample --control PORT` (sample design + control channel).
  Pick a free port. Without a display, run it under `Xvfb :99` with `DISPLAY=:99`.
- Drive it with JSON lines on `127.0.0.1:PORT` (see `docs/control-protocol.md`):
  - `{"id":1,"method":"engine.execute","params":{"command":"Extrude","params":{"distance":20}}}`
  - `{"id":2,"method":"ui.start","params":{"command":"SketchCreate"}}` then `ui.click {x, y}`
  - `{"id":3,"method":"ui.screenshot","params":{"path":"/tmp/shot.png"}}` then read the PNG.
- **For UI work, look at the result** (screenshot, read the PNG). Without a window use
  `ui.render` or `solvecraft-cli snapshot`.
- Headless: `solvecraft-cli run script.json --out part.step`, `solvecraft-cli eval design.solvecraft`,
  `solvecraft-cli snapshot script.json --out shot.png`, `solvecraft-cli commands`.
- Fusion oracle: `cargo xtask oracle` replays `plan/fusion/oracle/*/recipe.json` and compares
  with Fusion's `measure.json` (writes `docs/oracle.md`).
- Build with `CARGO_TARGET_DIR=<repo>/target`. Windows: `cargo xwin build --release --target x86_64-pc-windows-msvc -p solvecraft`.

## Roadmap
`ROADMAP.md` (committed) tracks status, milestones, parity and estimates. Update it whenever a
milestone task lands. `cargo xtask parity` recomputes command parity in `docs/parity.md`;
`cargo xtask oracle` recomputes `docs/oracle.md`.
