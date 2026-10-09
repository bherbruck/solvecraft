# Getting started

## Desktop

Download the latest build from the
[releases page](https://github.com/bherbruck/solvecraft/releases), or build it yourself with
stable Rust (1.90 or newer):

```sh
git clone https://github.com/bherbruck/solvecraft && cd solvecraft
cargo run --release -p solvecraft -- --sample   # opens with a sample part
```

`solvecraft part.step` opens a STEP, IGES, 3MF, STL or OBJ file as a new design, and
`solvecraft design.solvecraft` opens a saved design. Windows builds cross-compile from Linux with
`cargo xwin build --release --target x86_64-pc-windows-msvc -p solvecraft`.

## Web

The same app runs in the browser: [open SolveCraft on the web](../app/) (`?sample` opens the sample
part, `?webgl` forces WebGL2 where WebGPU is missing). Designs are saved in the browser's private
file storage, which needs HTTPS or localhost. To build it yourself:

```sh
rustup target add wasm32-unknown-unknown
cd apps/solvecraft-web && trunk serve --release
```

## CLI

`solvecraft-cli` runs the same commands without a window:

```sh
cargo run --release -p solvecraft-cli -- commands                       # every command, as JSON
cargo run --release -p solvecraft-cli -- run examples/bracket.json --out bracket.step
cargo run --release -p solvecraft-cli -- eval examples/bracket.json     # volume, area, faces…
cargo run --release -p solvecraft-cli -- snapshot examples/bracket.json --out bracket.png
cargo run --release -p solvecraft-cli -- exec solid.box '{"length": 20, "width": 10, "height": 5}'
```

A script is a list of commands:

```json
{"commands": [
  {"command": "sketch.create", "params": {"plane": "XY"}},
  {"command": "sketch.rectangle.two_point", "params": {"p0": [0, 0], "p1": [40, 30]}},
  {"command": "sketch.finish", "params": {}},
  {"command": "solid.extrude", "params": {"distance": 20}},
  {"command": "solid.fillet", "params": {"edges": [[0, 0, 10]], "radius": 3}}
]}
```

![The bracket example, rendered by solvecraft-cli snapshot](../generated/bracket.png)

Units are millimetres and degrees unless an expression says otherwise (`"1 in"`, `"30 deg"`),
and Z is up.
