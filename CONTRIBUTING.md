# Contributing to SolveCraft

SolveCraft is an open-source parametric 3D CAD app written in pure Rust, in the style of
Autodesk Fusion. You sketch on a plane, constrain the sketch, turn profiles into solids, and
change a parameter to watch the timeline rebuild. It runs as a desktop app (Windows, Linux), in
the browser (wasm + WebGPU/WebGL2), headless from a CLI, and as an MCP server that AI agents can
model with.

Help is welcome, whether that's a one-line fix or owning a whole area. This page covers how to
build it, how the code is laid out, the few rules that aren't negotiable, and where help is
needed most.

## Get it running (about 10 minutes on a fast machine)

You need stable Rust 1.90 or newer. For the web
build you also need `rustup target add wasm32-unknown-unknown` and [`trunk`](https://trunkrs.dev).

```sh
git clone https://github.com/bherbruck/solvecraft && cd solvecraft
cargo run --release -p solvecraft -- --sample      # desktop app with a sample part
cargo xtask test                                   # unit, engine and UI-scenario tests
cargo xtask ci                                     # everything a PR must pass (see below)
```

Web: `cd apps/solvecraft-web && trunk serve --release`, then open the printed URL (`?sample`
opens the sample part, `?webgl` forces WebGL2). Saving and opening files in the browser needs
HTTPS or localhost, because it uses the Origin Private File System.

Windows from Linux: `cargo xwin build --release --target x86_64-pc-windows-msvc -p solvecraft`.

## How the code is laid out

The [README](README.md#architecture) has the layer table. The short version:

- `crates/geom`: vectors, planes, meshes, measures.
- `crates/sketch`: our own constraint solver, plus profile finding.
- `crates/kernel`: the B-rep kernel boundary. It wraps [truck](https://github.com/ricosjp/truck),
  and it's the only crate allowed to touch truck. Fillets, chamfers, shell/offset, Delete Face,
  Split Face, STEP/IGES read and write, and face provenance are our own code here. Patched
  copies of three truck crates live in `vendor/` and are excluded from our fmt/clippy rules.
- `crates/doc`: the document. It holds parameters and expressions, the feature timeline and its
  incremental rebuild, components and occurrences, joints, appearances, and persistent naming.
- `crates/io`: design files and mesh formats (3MF, STL, OBJ).
- `crates/engine`: the session and the command registry. **Everything the user can do is a
  command**, and the toolbar, dialogs, scripts, CLI, control channel and MCP all run the same
  commands.
- `crates/ui-egui`: the desktop/web front end (egui + wgpu). It's swappable; nothing below it
  depends on egui.
- `crates/mcp`, `apps/solvecraft`, `apps/solvecraft-cli`, `apps/solvecraft-web`.

`cargo xtask layers` fails the build if a lower layer depends on a higher one.

## Rules that aren't negotiable

1. **Clean-room.** We observe Fusion as a black box: its UI, how parts it builds measure, and
   what it does with a given input. We never copy Autodesk code, icons, artwork, help text,
   tooltips, templates or materials, and we never commit files Fusion produced. Command ids are
   our own (`solid.fillet`, not Fusion's internal names).
2. **No GPL/LGPL/AGPL code, not even reading it.** That rules out OpenCASCADE, FreeCAD
   (including planegcs), SolveSpace, CadQuery/OCP, LibreCAD and QCAD. SolveCraft is
   MIT OR Apache-2.0, and its solver and blends are written from the maths. Permissive
   dependencies are fine. Note which one you're adding and why in the PR, and run
   `cargo xtask licences` so THIRD-PARTY-LICENSES.txt lists it (CI checks that it is current).
3. **Every asset is original or openly licensed and has a row in
   [ATTRIBUTION.md](ATTRIBUTION.md).** Icons are drawn in code (`crates/ui-egui/src/icons.rs`).
   `cargo xtask assets` enforces this.
4. **Never crash.** People trust a CAD app with their work.
   - Non-test code never uses `unwrap`, `expect`, `panic!` or indexing on input-derived data.
     There's no `unsafe`, and clippy denies it.
   - An unsupported case returns an error ("not supported yet: …"); it never panics.
   - Every call into truck goes through `solvecraft_kernel::guard`.
   - Every crash fix comes with a regression test.
5. **Everything is a command, with a test.** A new feature means a `CommandSpec` in
   `crates/engine/src/cmd/`, engine tests that measure the result (volume, area,
   face/edge/vertex counts against a value you can derive), and for UI work a scenario (below).

[AGENTS.md](AGENTS.md) has the long form of these rules. It's written for AI agents, but it's
the most complete description of the house style.

## Tests

| What | Where | Run |
|---|---|---|
| Unit and engine tests | next to the code, or `*_tests.rs` files | `cargo xtask test` (or `cargo test --workspace --profile ci`) |
| UI scenarios | `crates/ui-egui/tests/scenarios/*.json` | part of `cargo test` |
| Fusion oracle | `cargo xtask oracle` | needs the local oracle data (see below) |

**UI scenarios** drive the real app headless: start a tool, click a plane or a sketch point,
press keys, then assert on the result.

```json
[
  {"start": "sketch.create"},
  {"click": {"plane": "XY"}},
  {"start": "sketch.rectangle.two_point"},
  {"click": {"sketch": [0, 0]}},
  {"click": {"sketch": [40, 30]}},
  {"expect": {"errors": 0}}
]
```

A new `.json` file in that folder is picked up automatically. To record a bug before it's fixed,
put `{"pending": "why"}` in the scenario, and remove it in the PR that fixes the bug.

**Use the `ci` profile.** CI runs the tests with `--profile ci`: light optimisation, line tables,
no LTO, and debug assertions off (truck has debug-only assertions that reject valid geometry). It
builds several times faster than `--release`. If a test fails only in a plain debug build, say so
in the PR. `cargo xtask test` builds the tests with it and then runs every test binary at once
(`cargo test` runs them one after another); arguments after `--` go to each binary, e.g.
`cargo xtask test -- shell`. Tests run beside each other on a busy machine, so a test must not
depend on wall-clock timing.

**The Fusion oracle** is a set of 76 reference parts, each built in Fusion from a recipe.
SolveCraft rebuilds the same recipe and compares volume, area and topology counts with Fusion's
measurements; [docs/oracle.md](docs/oracle.md) has the current results. The recipes and
measurements live outside the repo (`plan/`, gitignored), because they come from a Fusion
install. If you work on geometry and want to check against it, ask in an issue.

## Sending a change

1. Work is tracked in [GitHub issues](https://github.com/bherbruck/solvecraft/issues), labelled
   by area (`area:kernel`, `area:sketch`, `area:doc`, `area:ui`, `area:infra`). Before starting,
   comment on the issue so it gets the `in progress` label and two people don't build the same
   thing. For anything bigger than a bug fix that has no issue yet, open one first.
2. Branch from `main` and keep the PR to one topic.
3. Run `cargo xtask ci` before pushing. It runs fmt, the asset, licence and layer checks,
   clippy (`-D warnings`), then the tests with the wasm build check alongside. A PR has to pass it.
4. Reference the issue (`Fixes #12`). In the PR, say what you tested and how. For geometry work, include the measured numbers (and
   the expected ones).

Much of SolveCraft so far was written by AI coding agents (Claude) working in parallel, each
owning one area, with a human setting direction and reporting bugs. Their working notes are in
[AGENTS.md](AGENTS.md). Human contributors are just as welcome. Use whatever tools you like, as
long as the change meets the rules above.

## Where help is wanted most

See the [`help wanted`](https://github.com/bherbruck/solvecraft/labels/help%20wanted) and
[`good first issue`](https://github.com/bherbruck/solvecraft/labels/good%20first%20issue) labels.
In order of how much they'd help users:

1. **Fillet and blend robustness.** Fillets are our own code (truck has no fillet operation).
   Constant, variable-radius and chord fillets work, and so do corners where fillets of
   different radii meet, including three rounds meeting at a vertex. Next are setback corners, fillets along curved edges between curved
   faces, and fillets that consume neighbouring faces.
2. **Boolean robustness.** A random-design fuzzer currently gets about half of its designs
   through without a kernel error. Each failing seed is a reproducible bug.
3. **Y-up / Z-up.** SolveCraft is Z-up only, so most STEP files (from SolidWorks, Onshape, or
   Fusion's default) open lying on their backs. Fusion solves this with a "default modeling
   orientation" preference.
4. **Imported appearances.** Colours from STEP files are lost when a feature changes the
   imported body. They should become design appearances on import.
5. **Drawings, CAM and mesh tools.** These are out of scope for now (see
   [ROADMAP.md](ROADMAP.md)), but anyone who wants to own one is welcome.

## Licence

By contributing you agree that your contribution is licensed as MIT OR Apache-2.0, like the rest
of SolveCraft. SolveCraft isn't affiliated with Autodesk. Fusion is a trademark of Autodesk, Inc.
