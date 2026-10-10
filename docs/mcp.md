# SolveCraft and AI agents (MCP)

SolveCraft speaks the [Model Context Protocol](https://modelcontextprotocol.io) over stdio
(JSON-RPC 2.0, one message per line; protocol revisions 2025-06-18, 2025-03-26 and 2024-11-05).
An agent models exactly like a person does: every tool runs engine commands, the same ones the
toolbar, palette, scripts and control channel use, and never opens a dialog.

## Start it

```sh
# Headless: an in-process design session, no window (CPU renders for screenshots).
solvecraft-cli mcp

# Headless, starting from a design file or a command script.
solvecraft-cli mcp --in part.solvecraft

# Bridged to the running app, so you can watch the agent model:
solvecraft --control 7878 &
solvecraft-cli mcp --connect 127.0.0.1:7878
```

stdout carries only protocol messages; errors go to stderr. Bridged screenshots are written to a
temporary file by the app and read back by the server, so both must run on the same machine.

### Client configuration

Claude Code (project `.mcp.json`, or `claude mcp add solvecraft -- solvecraft-cli mcp`):

```json
{
  "mcpServers": {
    "solvecraft": { "command": "solvecraft-cli", "args": ["mcp"] }
  }
}
```

Claude Desktop (`claude_desktop_config.json`), bridged to a running app:

```json
{
  "mcpServers": {
    "solvecraft": { "command": "/path/to/solvecraft-cli", "args": ["mcp", "--connect", "127.0.0.1:7878"] }
  }
}
```

## Tools

| Tool | Arguments | What it does |
|---|---|---|
| `list_commands` | `filter?` | The command catalog: id, label, tab/panel, shortcut, parameter docs, enabled now. |
| `execute` | `command`, `params?` | Run any command with JSON parameters (`list_commands` shows them). Failures leave the design unchanged. |
| `batch` | `commands: [{command, params}]`, `rollback?` (default true) | Run steps in order, stop at the first error and undo the completed steps. The reply names the failing step (`failed.index`, `failed.command`, `failed.error`) and every result before it. |
| `inspect_design` | `measure?` (default true), `sketches?` (default true) | Parameters, timeline (type, suppressed, rolled back, error, warning, timing), bodies (volume, area, centre of mass, bbox, face/edge/vertex counts, visible), sketches (curves, points, constraints, profiles, DOF, solve status, visible), selection, undo depth. Hide helper sketches, bodies or construction planes with `execute` `browser.visibility` (`{items, visible}` or `{folder, visible}`). |
| `measure` | `body?` | Volume (mm³), area (mm²), centre of mass, bbox and topology counts per body, with totals. |
| `body_topology` | `body` | Edges (index, midpoint, length, ends) and faces (index, area, centroid, normal). An edge midpoint is the `[x,y,z]` that picks that edge for fillet and chamfer. |
| `set_parameter` | `name`, `value` (number or expression), `unit?`, `comment?` | Create or change a user parameter and recompute; reports features that now fail. |
| `screenshot` | `view?` (iso, top, front, back, bottom, left, right, home, fit), `path?`, `width?`, `height?`, `source?` (window, model) | A PNG image content block. Headless: CPU render fitted to the model. Bridged: the app window (default) or a model render with the app's camera. |
| `export` | `path`, `format?` (step, 3mf, stl, stla, obj), `bodies?` | STEP, 3MF or mesh export. |
| `undo`, `redo` | — | Undo / redo one change. |
| `new_design` | `name?` | Start an empty design. |
| `open` | `path` | Open a `.solvecraft` design, a STEP file (`.step`/`.stp`) as a new design holding its bodies (an Import base feature), or a 3MF/STL file as mesh bodies. |
| `save` | `path?` | Save the design (to its current file without `path`). |

Resources: `solvecraft://design` (inspect JSON with measurements) and `solvecraft://commands`
(the catalog).

Errors: an unknown tool or arguments that don't match the tool's schema are JSON-RPC
`-32602` errors; an engine failure (bad command parameters, a feature that fails) is a tool
result with `isError: true` and the engine's message, which the model can read and correct.

`batch` rollback undoes as many steps as the batch added to the undo history; batches that
themselves call `edit.undo`, `file.new` or `doc.open` are not rolled back exactly.

## Demo: an enclosure built over MCP

`examples/demo/enclosure_mcp.py` is a small MCP client. It drives a running app (`solvecraft
--control PORT`) through `solvecraft-cli mcp --connect` and builds an electronics enclosure one
`execute` call at a time (`enclosure_steps.json`): a rounded box, shelled open at the top, a
USB port cut through a side wall, four screw bosses, and then a change of the `width`
parameter that the timeline rebuilds. `examples/demo/record.sh` records the run with ffmpeg,
with each step's caption burnt in.

`examples/demo/enclosure_ui_mcp.py` (v2) builds the same part the way a person would: the
modelling goes through the app's UI over its control channel. The pointer glides to toolbar
buttons and faces and clicks them, the real dialogs open, the sketches go on the model's faces,
and the camera follows. The parameters, the sketch dimensions bound to them and two extrudes go
over MCP. With `--story v3` (the default) one standoff is patterned 2 × 2 in the Rectangular
Pattern dialog with spacings from the parameters, and width and wall are then changed in the
Parameters dialog. `record_ui.sh` records it and draws the pointer and the captions.

## A typical session

```text
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"me","version":"1"}}}
→ {"jsonrpc":"2.0","method":"notifications/initialized"}
→ {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"batch","arguments":{"commands":[
     {"command":"sketch.create","params":{"plane":"XY"}},
     {"command":"sketch.rectangle.two_point","params":{"p0":[0,0],"p1":[40,30]}},
     {"command":"sketch.finish"},
     {"command":"solid.extrude","params":{"distance":20}},
     {"command":"solid.fillet","params":{"edges":[[0,0,10]],"radius":3}},
     {"command":"sketch.create","params":{"plane":"XY"}},
     {"command":"sketch.circle.center","params":{"center":[20,15],"radius":5}},
     {"command":"solid.extrude","params":{"distance":20,"operation":"cut"}}]}}}
← {"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"{\"ok\": true, \"completed\": 8, \"results\": [...]}"}]}}
→ {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"measure","arguments":{}}}
← … "total": {"volume_mm3": 22390.6, …}
→ {"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"screenshot","arguments":{"view":"iso"}}}
← {"jsonrpc":"2.0","id":4,"result":{"content":[{"type":"image","mimeType":"image/png","data":"iVBORw0…"},{"type":"text","text":"{\"source\":\"model\",…}"}]}}
→ {"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"export","arguments":{"path":"/tmp/part.step"}}}
```

The tests in `crates/mcp/src/tests.rs` run sessions like this (box → fillet → hole, parameters,
batch rollback, hostile arguments) and check the results only through MCP.
