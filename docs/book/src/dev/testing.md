# Test harnesses

`cargo xtask ci` runs everything a pull request must pass: fmt, clippy with `-D warnings`, the
tests in release mode, `xtask layers`, `xtask assets` and the wasm build check.

| Harness | Where | What it catches |
|---|---|---|
| Unit and engine tests | next to the code, `*_tests.rs` | measured results: volume, area, topology counts against derived values |
| Hostile input | `hostile_params_never_panic`, `crates/engine/src/tests.rs` | every command × every documented parameter × junk values: errors, never panics |
| Sketch fuzz | `crates/engine/src/cmd/sketch_fuzz_tests.rs` | random sketches through the solver and profile finder |
| UI scenarios | `crates/ui-egui/tests/scenarios/*.json` | the real app, headless: clicks, keys, drags, assertions |
| Layout tests | `crates/ui-egui/tests/ui_layout.rs`, `window_widths.rs` | panels and dialogs at every window width |
| Design-file corpus | `apps/solvecraft-cli/tests/corpus` | every saved file version still opens and evaluates the same |
| Kill mid-save | `apps/solvecraft-cli/tests/kill_mid_save.rs` | a save killed at any moment leaves a readable file |
| Fusion oracle | `cargo xtask oracle` → [docs/oracle.md](oracle.md) | parts rebuilt from recipes, compared with Fusion's measurements |
| STEP corpus | `cargo xtask step-corpus` → [docs/step-import.md](step-import.md) | STEP files from Fusion opened and measured |

## UI scenarios

A scenario is a JSON list of steps run against the real app with a headless egui context:

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

A new file in `crates/ui-egui/tests/scenarios/` is picked up automatically. To record a bug
before it's fixed, add a `{"pending": "why"}` step; its test is then ignored with that reason, and
the fix removes the step.

## Release mode

CI runs the tests with `--release`, because some truck assertions only exist in debug builds
and fail on valid geometry. If a test fails only in debug, say so in the PR.
