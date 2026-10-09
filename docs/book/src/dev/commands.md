# The command engine

Every user-visible action is a **command**: a `CommandSpec` registered in
`crates/engine/src/cmd/`, with a dotted, lower-case id (`solid.extrude`, `sketch.create`,
`timeline.rollback`). The toolbar, dialogs, marking menu, S box, scripts, the CLI, the control
channel and MCP all run commands through `Session::execute(id, params)`. Commands take JSON
parameters and never open dialogs; a dialog is a front-end form that builds the parameters.

```rust
CommandSpec::new("solid.fillet", "Fillet", fillet)
    .at("SOLID", "MODIFY")      // toolbar tab and panel ("" = not on the toolbar)
    .icon("fillet")             // drawn in code, crates/ui-egui/src/icons.rs
    .key("F")                   // default shortcut
    .params("edges: [[x,y,z] | {body, index}]; radius: expr; …"),
```

`run` is `fn(&mut Session, &Value) -> Result<Value>`, and `enabled` is
`fn(&Session) -> Result<(), String>`; the error string is the disabled-button tooltip. The
`params` string is the one-line documentation shown by `solvecraft-cli commands`, MCP's
`list_commands` and the [command reference](../reference/commands.md).

A command that changes the design is undoable by default: the session snapshots the document
before it runs, and a failing command leaves the document unchanged. Feature commands add a
feature to the timeline and let the document evaluate it; they validate their inputs first
(units, ranges, references) and return `BadParams` instead of building a broken feature.

Ids used before the 2026-10 rename still resolve through `crates/engine/src/legacy_ids.rs`, so
old scripts, recipes, saved shortcuts and toolbar layouts keep working.

## Adding a command

1. Add a `CommandSpec` to the right file in `crates/engine/src/cmd/` and implement `run`.
2. Write engine tests that measure the result: volume, area, and face/edge/vertex counts
   against values you can derive by hand.
3. The hostile-input test (`hostile_params_never_panic` in `crates/engine/src/tests.rs`) runs
   your command automatically with every documented parameter set to junk. It must return
   errors, never panic.
4. For UI work, add a scenario (see [Test harnesses](testing.md)).
