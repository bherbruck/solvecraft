# Architecture

SolveCraft is a Cargo workspace in layers. A crate only depends on crates in lower layers, and
`cargo xtask layers` fails the build otherwise.

{{#include ../../../../README.md:architecture}}

The web app, `apps/solvecraft-web`, is the desktop front end compiled to wasm with
[trunk](https://trunkrs.dev). `xtask` holds the repository's own tooling: `cargo xtask ci`,
`layers`, `assets`, `oracle`, `parity`, `step-corpus` and `book`.

A few rules follow from the layering:

- **Only `crates/kernel` touches truck.** Everything above it sees SolveCraft's own types:
  bodies, faces, edges, meshes and measures. See [The kernel boundary](kernel.md).
- **The document knows nothing about commands or the UI.** `crates/doc` holds the parameters,
  the feature timeline and the component tree, and evaluates them into a model.
- **Everything the user can do is a command** in `crates/engine`. See
  [The command engine](commands.md).
- **The front end is swappable.** Nothing below `ui-egui` depends on egui. The CLI and the MCP
  server run the same engine without a window, and `crates/render` has a CPU rasterizer so
  headless screenshots work anywhere.
