# Attribution

Every non-code asset in this repository (images, icons, fonts, example parts) is listed here with
its author, source and licence. `cargo xtask assets` (part of `cargo xtask ci`) fails if an asset
file is missing from this table.

**Policy (mandatory):** SolveCraft contains **no Autodesk iconography, images, artwork, fonts,
templates, materials or help text.** Every asset is original work by SolveCraft contributors or
third-party material under an open licence. Screenshots of Autodesk software are never committed.

Generated-in-code assets are original and have no file to list:
- the UI icon set (`crates/ui-egui/src/icons.rs`);
- the view cube, navigation bar and timeline glyphs (`crates/ui-egui/src/viewport.rs`,
  `crates/ui-egui/src/timeline.rs`).

## Files

| File | Author | Source | Licence |
|---|---|---|---|
| docs/screenshots/sample-plate.png | SolveCraft contributors | screenshot of SolveCraft itself (`solvecraft --sample`) | MIT OR Apache-2.0 |
| docs/book/src/generated/sample-plate.png | SolveCraft contributors | copy of docs/screenshots/sample-plate.png made by `cargo xtask book` (gitignored) | MIT OR Apache-2.0 |
| docs/book/src/generated/bracket.png | SolveCraft contributors | rendered by `cargo xtask book` (`solvecraft-cli snapshot examples/bracket.json`; gitignored) | MIT OR Apache-2.0 |
| docs/book/src/generated/enclosure.png | SolveCraft contributors | rendered by `cargo xtask book` (`solvecraft-cli snapshot docs/book/scripts/enclosure.json`; gitignored) | MIT OR Apache-2.0 |
| docs/book/src/generated/revolve.png | SolveCraft contributors | rendered by `cargo xtask book` (`solvecraft-cli snapshot docs/book/scripts/revolve.json`; gitignored) | MIT OR Apache-2.0 |
| docs/book/src/generated/sheet-metal.png | SolveCraft contributors | rendered by `cargo xtask book` (`solvecraft-cli snapshot docs/book/scripts/sheet-metal.json`; gitignored) | MIT OR Apache-2.0 |
| crates/sketch/fonts/LiberationSans-Regular.ttf | Red Hat, Inc. (Liberation Fonts 2.x; digitized data Google) | https://github.com/liberationfonts (Debian fonts-liberation), unmodified; licence text in crates/sketch/fonts/OFL.txt; used for sketch Text outlines | SIL OFL 1.1 |

## Code adapted from sibling projects

| SolveCraft file | Adapted from | Licence |
|---|---|---|
| xtask/src/layers.rs | CADCraft `xtask/src/layers.rs` (dependency layering rules and tests) | MIT OR Apache-2.0 |
| apps/solvecraft/src/control_server.rs | CADCraft `apps/cadcraft/src/control_server.rs` (loopback JSON-lines server) | MIT OR Apache-2.0 |
| crates/ui-egui/src/lib.rs (screenshot queue, synthetic input) | CADCraft `crates/ui-egui/src/lib.rs` | MIT OR Apache-2.0 |
| crates/ui-egui/src/control.rs (method dispatch and screenshot saving) | CADCraft `crates/ui-egui/src/control.rs` | MIT OR Apache-2.0 |
| crates/ui-egui/src/gpu.rs (paint-callback structure) | CADCraft `crates/ui-egui/src/gpu.rs` | MIT OR Apache-2.0 |
| crates/mcp/src/server.rs, crates/mcp/src/backend.rs (JSON-RPC framing, MCP lifecycle, control-port bridge) | CADCraft `crates/mcp/src/{server,backend}.rs` | MIT OR Apache-2.0 |
| crates/mcp/src/tools.rs (tool schema helpers and argument checking) | CADCraft `crates/mcp/src/tools.rs`, GridCraft `crates/mcp/src/tools.rs` | MIT OR Apache-2.0 |
| crates/io/src/threemf.rs (bounded zip part reading and writing) | GridCraft `crates/xlsx/src/package.rs` | MIT OR Apache-2.0 |
| crates/ui-egui/src/titlebar.rs (title bar drag, caption buttons, resize zones, their tests) and the window setup in apps/solvecraft/src/main.rs | VectorCraft `crates/ui-egui/src/titlebar.rs`, `apps/vectorcraft/src/main.rs` | MIT OR Apache-2.0 |
| LICENSE-MIT, LICENSE-APACHE (licence text) | CADCraft | — |

## Vendored third-party code

| Directory | Source | Licence | Changes |
|---|---|---|---|
| vendor/truck-meshalgo | truck-meshalgo 0.4.0, https://github.com/ricosjp/truck (RICOS Co. Ltd.) | Apache-2.0 | meshing: parameter searches kept inside the surface domain (vendor/README.md) |
| vendor/truck-stepio | truck-stepio 0.3.0, https://github.com/ricosjp/truck (RICOS Co. Ltd.) | Apache-2.0 | STEP writer: intersection-curve surfaces written at unique ids (vendor/README.md) |
| vendor/truck-shapeops | truck-shapeops 0.4.0, https://github.com/ricosjp/truck (RICOS Co. Ltd.) | Apache-2.0 | robust meshing and nearest-parameter fallback in booleans; trace macro (vendor/README.md) |
