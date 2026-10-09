# SolveCraft

SolveCraft is an open-source parametric 3D CAD application written in pure Rust. It works the way
Fusion-style modellers do: sketch on a plane, constrain and dimension the sketch, turn profiles
into solids with features on a timeline, and change a parameter to watch the whole timeline
rebuild.

![The sample plate in SolveCraft](generated/sample-plate.png)

It runs as:

- a **desktop app** for Windows and Linux (egui + wgpu);
- a **web app** in the browser (wasm, WebGPU or WebGL2), [try it here](app/);
- a **headless CLI**, `solvecraft-cli`, that runs command scripts, measures and exports;
- an **MCP server** that AI agents model with, headless or driving the running app.

Everything the user can do is a *command* with JSON parameters. The toolbar, the dialogs, scripts,
the CLI, the control channel and MCP all run the same commands, so anything you can click you can
also script, test or hand to an agent.

SolveCraft is early. Sketching, solid modelling, parameters, components with joints, sheet metal
and a plastic-enclosure subset work; surfaces, drawings, CAM and simulation are deferred. The
[roadmap](https://github.com/bherbruck/solvecraft/blob/main/ROADMAP.md) has the details, and
[command parity](dev/parity.md) and the [Fusion oracle](dev/oracle.md) measure how far along it
is.

This book has two halves: the **user guide** for people modelling with SolveCraft, and the
**developer guide** for people working on it. The [command reference](reference/commands.md) is
generated from the code on every build.

SolveCraft is licensed MIT OR Apache-2.0. It is a clean-room implementation and is not affiliated
with Autodesk; Fusion is a trademark of Autodesk, Inc.
