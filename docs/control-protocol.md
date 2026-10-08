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
| `ui.inspect` | — | UI state, viewport rect, camera, active tool/dialog, hover, renderer, timings, live preview (`active`, `busy`, `error`, `ms`, bodies it stands in for) |
| `ui.set` | any `UiState` field (`tab`, `perspective`, `showGrid`, `hiddenBodies`, `dark`, …) | `dark: false` switches to the light theme |
| `ui.view` | `view`: `front`, `back`, `top`, `bottom`, `left`, `right`, `iso`, `home`, `fit`; `animate` (bool, default false: snap) | |
| `ui.start` | `command` | like a toolbar click: starts the sketch tool or dialog for the command |
| `ui.click` | `x`, `y` (screen points), `button?`: left/right/middle, `shift?`, `ctrl?`, `double?` | real pointer input |
| `ui.move` | `x`, `y` | |
| `ui.drag` | `x0`, `y0`, `x1`, `y1`, `button?`, `shift?`, `ctrl?`, `steps?`, `hold?` | press, move, release; a left drag on the model is a box selection (left to right: window, right to left: crossing); `hold` keeps the button down that many frames first (a right press-and-hold opens the marking menu, releasing over a direction picks it) |
| `ui.menu` | `x?`, `y?`, `target?`, `close?` | with a point: open a context menu there as a right-click would (the viewport's marking menu, or a browser menu with `target: {"type": "body", "name"} \| {"type": "sketch", "id"} \| {"type": "component", "id"}`); without: the open menu (`radial` and `items`: id, label, shortcut, enabled) |
| `ui.menuPick` | `item`: id or label | runs an item of the open menu, as clicking it does |
| `ui.rename` | `text?`, `commit?` (default true) | finishes the rename box a Rename item opened |
| `ui.selection` | | the selection, the open dialog's inputs, the hovered item, and the active sketch's drawn dimensions (`dimensions`: id and text centre, `dimension_selected`, `dimension_editing`) |
| `ui.worldToScreen` | `points`: `[[x, y, z]…]` world points | their screen points |
| `ui.triad` | | screen points of the 3D sketch move triad's handles (`x`, `y`, `z`, `plane`) |
| `ui.sketchToScreen` | `points`: `[[x, y]…]` in active-sketch coordinates | their screen points (null when behind the camera) |
| `ui.dialogInput` | `index` | what clicking an input's Select chip does: picks go to that input |
| `ui.at` | `world: [x,y,z]` \| `sketch: [x,y]` \| `plane: "XY"` \| `axis: "Z"` \| `dimension: "d1"` (its value text) | the screen point (a scenario step's target; see `crates/ui-egui/src/scenario.rs`) |
| `ui.editFeature` | `feature` (id or name) | what double-clicking a timeline item does: rolls back to it and opens its dialog filled in (sketches open in sketch mode) |
| `ui.scroll` | `x`, `y`, `delta?` | wheel zoom at the cursor |
| `ui.key` | `key` (egui key name, `Enter`, `Escape`…), `cmd?`, `shift?` | |
| `ui.text` | `text` | typed text |
| `ui.screenshot` | `path?` | PNG of the window (needs a presented frame) |
| `ui.render` | `path`, `width?`, `height?` | CPU render of the model with the current camera (no window needed) |
| `ui.confirm` | `accept?`: bool | answers the Delete confirmation (shown when a delete takes other features with it or makes some fail); without `accept`, what it lists |
| `ui.documents` | `action?`: `new`, `switch`, `close`; `index?`; `force?` (close without asking) | the open designs (tabs): names, unsaved flags, the active one, a pending "save changes?" prompt |
| `ui.home` | `open?`: bool; `sample?`: index | the start page: show or hide it, open a built-in sample (once built); returns `open` and the recent designs |
| `ui.shortcut` | `command`, `key?`, `replace?` | a command's key, or rebinds it (`""` clears; a key another command uses is refused unless `replace`) |
| `ui.help` | `item?`: `about`, `shortcuts` (read only), `report` (copies diagnostics) | the version, commit, build date and diagnostic text |
| `ui.window` | `action`: minimize, maximize, restore, toggle, close | what the title bar's caption buttons do (on Windows and Linux the application bar is the title bar) |
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
