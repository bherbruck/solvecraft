SolveCraft is an open-source parametric 3D CAD application in pure Rust: sketch on a plane,
constrain and dimension the sketch, turn profiles into solids on a feature timeline, and change a
parameter to watch the timeline rebuild. This is the first public build, a **pre-release**: it is
early, and we'd like your bug reports.

- **Documentation:** https://bherbruck.github.io/solvecraft/
- **Try it in the browser:** https://bherbruck.github.io/solvecraft/app/ (`?sample` opens a sample part)
- **Contributing:** [CONTRIBUTING.md](https://github.com/bherbruck/solvecraft/blob/main/CONTRIBUTING.md)
- **Bugs and ideas:** [issues](https://github.com/bherbruck/solvecraft/issues)

## What works

- Sketches with SolveCraft's own constraint solver: lines, rectangles, circles, arcs, polygons,
  slots, splines, text; constraints and parameter-driven dimensions; automatic profiles.
- Solid features on a parametric timeline that rebuilds incrementally: extrude, revolve, sweep,
  loft, fillet and chamfer, shell, draft, holes and threads, patterns, mirror, combine, split,
  primitives. Faces and edges are referenced by persistent names, so upstream edits don't
  scramble downstream features.
- Parameters with unit-aware expressions, and configurations.
- Components, joints, motion studies, sheet metal with flat patterns, and a plastic-enclosure
  subset (bosses, lips, snap fits).
- Files: `.solvecraft` designs; STEP (AP203/214/242) and IGES import and export; 3MF, STL and OBJ;
  DXF. Atomic saves, autosave and crash recovery.
- 76 of 76 reference parts built in Fusion rebuild in SolveCraft with the same volume, area and
  topology.

## Built for AI agents

Everything the UI does is a command with JSON parameters, so an agent can do it too.
`solvecraft-cli mcp` is a Model Context Protocol server: headless, or bridged to the running app
(`solvecraft --control 7878`, then `solvecraft-cli mcp --connect 127.0.0.1:7878`) so you can
watch the agent model. In Claude Code: `claude mcp add solvecraft -- solvecraft-cli mcp`. The
attached video shows an agent building an enclosure through the UI and MCP.

## Downloads

| File | What |
|---|---|
| `solvecraft-v0.1.0-windows-x86_64.zip` | Windows 10/11: `solvecraft.exe` (the app) and `solvecraft-cli.exe` |
| `solvecraft-v0.1.0-linux-x86_64.tar.gz` | Linux x86_64 (glibc 2.35 or newer): `solvecraft` and `solvecraft-cli` |
| `SolveCraft-v0.1.0-x86_64.AppImage` | Linux, the app as one file: `chmod +x` and run it |
| `solvecraft-v0.1.0-macos-universal.zip` | macOS 11 or newer, Apple silicon and Intel: `SolveCraft.app` and `solvecraft-cli` |
| `solvecraft-v0.1.0-web.zip` | the web app, to host yourself (any static server; HTTPS or localhost for saving files) |
| `SHA256SUMS` | checksums: `sha256sum -c SHA256SUMS` |

### The binaries are not signed

- **macOS:** the first time, right-click (or Control-click) `SolveCraft.app` and choose **Open**,
  then **Open** again in the dialog. If macOS says the app is damaged, run
  `xattr -dr com.apple.quarantine SolveCraft.app`. Do the same for `solvecraft-cli`.
- **Windows:** SmartScreen may say "Windows protected your PC". Click **More info**, then
  **Run anyway**.

## Known limitations

SolveCraft is early. Some fillet, chamfer and boolean configurations still fail (with an error,
never a crash); surfaces, drawings, CAM, mesh tools and simulation are out of scope for now; the
model is Z-up only, so STEP files from Y-up tools open lying on their backs. The full list is in
[ROADMAP.md](https://github.com/bherbruck/solvecraft/blob/main/ROADMAP.md#known-limitations).

SolveCraft is MIT OR Apache-2.0. It is a clean-room implementation and is not affiliated with
Autodesk; Fusion is a trademark of Autodesk, Inc.
