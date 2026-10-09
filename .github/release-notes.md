SolveCraft is an open-source parametric 3D CAD application in pure Rust: sketch on a plane,
constrain and dimension the sketch, turn profiles into solids on a feature timeline, and change a
parameter to watch the timeline rebuild. It is early, and we'd like your bug reports.

## Links

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
demo at the top of the [README](https://github.com/bherbruck/solvecraft#readme) shows an agent
building an enclosure through the UI and MCP.

## Downloads

| File | What |
|---|---|
| `SolveCraft-{tag}-windows-x86_64.exe` | Windows 10/11: the app, ready to run |
| `solvecraft-cli-{tag}-windows-x86_64.exe` | Windows: the command-line tool and MCP server |
| `SolveCraft-{tag}-macos-universal.dmg` | macOS 11 or newer, Apple silicon and Intel: open it and drag SolveCraft onto Applications |
| `solvecraft-cli-{tag}-macos-universal` | macOS: the command-line tool and MCP server |
| `SolveCraft-{tag}-x86_64.AppImage` | Linux x86_64 (glibc 2.35 or newer): the app as one file |
| `solvecraft-{tag}-linux-x86_64` | Linux: the app as a plain binary |
| `solvecraft-cli-{tag}-linux-x86_64` | Linux: the command-line tool and MCP server |
| `solvecraft-{tag}-web.zip` | the web app, to host yourself (any static server; HTTPS or localhost for saving files) |
| `SHA256SUMS` | checksums: `sha256sum -c SHA256SUMS` |

On Linux and macOS a downloaded program isn't executable yet: `chmod +x` it first
(`chmod +x SolveCraft-{tag}-x86_64.AppImage && ./SolveCraft-{tag}-x86_64.AppImage`). The licences
of SolveCraft and everything it is built from are inside each program: Help ▸ About ▸ Licences,
or `solvecraft-cli licences`.

### The binaries are not signed

- **macOS:** the first time, right-click (or Control-click) `SolveCraft.app` in Applications and
  choose **Open**, then **Open** again in the dialog. If macOS says the app is damaged, run
  `xattr -dr com.apple.quarantine /Applications/SolveCraft.app`. For the CLI:
  `xattr -d com.apple.quarantine solvecraft-cli-{tag}-macos-universal`.
- **Windows:** SmartScreen may say "Windows protected your PC". Click **More info**, then
  **Run anyway**.

## Known limitations

SolveCraft is early. Some fillet, chamfer and boolean configurations still fail (with an error,
never a crash); surfaces, drawings, CAM, mesh tools and simulation are out of scope for now; the
model is Z-up only, so STEP files from Y-up tools open lying on their backs. The full list is in
[ROADMAP.md](https://github.com/bherbruck/solvecraft/blob/main/ROADMAP.md#known-limitations).

SolveCraft is MIT OR Apache-2.0. It is a clean-room implementation and is not affiliated with
Autodesk; Fusion is a trademark of Autodesk, Inc.
