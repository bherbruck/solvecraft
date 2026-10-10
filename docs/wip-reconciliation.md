# WIP branch reconciliation (#6)

The branches predate the dotted command IDs. Their remaining changes were ported through
`crates/engine/src/legacy_ids.rs` and reconciled with the current implementation.

| Branches | Result |
|---|---|
| `solvecraft-sk`, `solvecraft-d2` | Ported `62d22e2` and `e0b3138`: sketches/projections see placed components; cache keys include placements; moved feature dialogs resolve world picks; Joints uses Browser rows. |
| `solvecraft-b4` | Ported `2c18dac`: canvases carry component ownership and named joint origins follow component placements. |
| `solvecraft-asm` | Ported `bc05805`: joint snap feedback, Drive limits, joint menus, and Motion Study editing/playback controls. |
| `solvecraft-shell`, `solvecraft-shell-b` | Browser chevrons, shared occurrence Move triad, toolbar customization, workspace switcher and status labels are already present on main. Their scenarios remain active; the Joints chevron scenario is now active too. |
| `solvecraft-kdev3`, `solvecraft-kdev`, `solvecraft-kdev2` | Previously landed as `67c79ec`; the old refs were already removed. |
| `solvecraft-kdev4` | The earlier robustness, shell, blends, wasm and measurement work is already present. Ported the remaining `9f98449` stacked-prism sewing and repaired curved-boundary face picking. |
| `solvecraft-params(-build)` | Previously landed as `517b7a9`. |

The source WIP refs are retained until this batch merges. Delete them only after the merged
main contains the reconciled changes and passes CI.

Regression coverage includes root sketches on translated and rotated components, subsequent
component moves, canvas save/reopen, joint origins during pending moves, moved feature dialog
refill, Browser chevrons, sample building, and the UI scenarios. `cargo xtask ci` covers native
tests, wasm, layering and asset attribution.
