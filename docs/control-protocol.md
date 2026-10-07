# Control protocol

`solvecraft --control PORT` (or `SOLVECRAFT_CONTROL_PORT=PORT`) listens on `127.0.0.1:PORT` for
JSON lines. Each request is one line `{"id": …, "method": "…", "params": {…}}`; each reply is one
line `{"id": …, "ok": true, "result": …}` or `{"id": …, "ok": false, "error": "…"}`. Requests are
handled on the UI thread between frames; pointer and keyboard input is injected as real egui
events, one pointer event per frame.

## Engine

| Method | Params | Result |
|---|---|---|
| `engine.execute` | `command`: command id, `params`: JSON object | the command's result |
| `engine.script` | `commands`: `[{command, params}, …]` | results, stops at the first error |
| `engine.commands` | — | every command: id, label, tab, panel, icon, shortcut, params doc, enabled |
| `document.inspect` | `measure?`: bool | parameters, timeline (errors, timings), bodies, sketches, selection |

Commands never open dialogs when run this way. `solvecraft-cli commands` prints the same registry.

## UI

| Method | Params | Notes |
|---|---|---|
| `ui.inspect` | — | UI state, viewport rect, camera, active tool/dialog, hover, renderer, timings |
| `ui.set` | any `UiState` field (`tab`, `perspective`, `showGrid`, `hiddenBodies`, …) | |
| `ui.view` | `view`: `front`, `back`, `top`, `bottom`, `left`, `right`, `iso`, `home`, `fit`; `animate` (bool, default false: snap) | |
| `ui.start` | `command` | like a toolbar click: starts the sketch tool or dialog for the command |
| `ui.click` | `x`, `y` (screen points), `button?`: left/right/middle, `shift?`, `ctrl?` | real pointer input |
| `ui.move` | `x`, `y` | |
| `ui.drag` | `x0`, `y0`, `x1`, `y1`, `button?`, `shift?`, `ctrl?`, `steps?` | press, move, release; a left drag on the model is a box selection (left to right: window, right to left: crossing) |
| `ui.selection` | | the selection, the open dialog's inputs and the hovered item |
| `ui.scroll` | `x`, `y`, `delta?` | wheel zoom at the cursor |
| `ui.key` | `key` (egui key name, `Enter`, `Escape`…), `cmd?`, `shift?` | |
| `ui.text` | `text` | typed text |
| `ui.screenshot` | `path?` | PNG of the window (needs a presented frame) |
| `ui.render` | `path`, `width?`, `height?` | CPU render of the model with the current camera (no window needed) |
| `ui.resize` | `width`, `height` | |
| `app.quit` | — | |

## MCP

`solvecraft-cli mcp --connect 127.0.0.1:PORT` bridges an MCP client (an AI agent) to this
channel; see [mcp.md](mcp.md).

## Example

```text
{"id":1,"method":"engine.execute","params":{"command":"SketchCreate","params":{"plane":"XY"}}}
{"id":2,"method":"engine.execute","params":{"command":"ShapeRectangleTwoPoint","params":{"p0":[0,0],"p1":[40,30]}}}
{"id":3,"method":"engine.execute","params":{"command":"Extrude","params":{"distance":20}}}
{"id":4,"method":"engine.execute","params":{"command":"FusionFilletEdgesCommand","params":{"edges":[[0,0,10]],"radius":3}}}
{"id":5,"method":"ui.screenshot","params":{"path":"/tmp/part.png"}}
```
