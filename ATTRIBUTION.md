# Attribution

Every non-code asset in this repository (images, icons, fonts, example parts) is listed here with
its author, source and licence. `cargo xtask assets` (part of `cargo xtask ci`) fails if an asset
file is missing from this table.

**Policy (mandatory):** SolidCraft contains **no Autodesk iconography, images, artwork, fonts,
templates, materials or help text.** Every asset is original work by SolidCraft contributors or
third-party material under an open licence. Screenshots of Autodesk software are never committed.

Editable drawing code, native projects and shipped exports are collected under `assets/`.

## Files

| File | Author | Source | Licence |
|---|---|---|---|
| examples/render-studio.json | SolidCraft contributors | original procedural box/cylinder enclosure script for the integrated Render workspace | MIT OR Apache-2.0 |
| examples/render-opaque-studio.json | SolidCraft contributors | original procedural sphere study varying opaque metal fraction and roughness independently; no external assets or engineering data | MIT OR Apache-2.0 |
| crates/gpu/src/pbr.rs | SolidCraft contributors | original WGSL translation of SolidCraft's opaque surface and fixed studio reference, embedded as Rust source; no external shader code | MIT OR Apache-2.0 |
| assets/archive/icons/macos-v1/exports/icon.svg | SolidCraft contributors | original macOS adaptation of the code-drawn SolidCraft mark, edited and exported with VectorCraft; Apple HIG and production templates consulted as layout references only | MIT OR Apache-2.0 |
| assets/icons/app/macos/source/SolidCraft.icon/icon.json | SolidCraft contributors | approved Icon Composer composition and appearance settings for the original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/app/macos/source/SolidCraft.icon/Assets/02-grid.svg | SolidCraft contributors | original perspective grid with elliptical radial alpha, exported from VectorCraft | MIT OR Apache-2.0 |
| assets/icons/app/macos/source/SolidCraft.icon/Assets/02-grid-dark.svg | SolidCraft contributors | original blue perspective grid with elliptical radial alpha, exported from VectorCraft for the Dark appearance | MIT OR Apache-2.0 |
| assets/icons/app/macos/source/SolidCraft.icon/Assets/03-solid-blue-cutout.svg | SolidCraft contributors | original saturated blue CAD solid with transparent topology cutouts, edited and exported with VectorCraft | MIT OR Apache-2.0 |
| assets/icons/app/macos/exports/Assets.car | SolidCraft contributors | Apple actool compilation of assets/icons/app/macos/source/SolidCraft.icon for macOS; contains original artwork only | MIT OR Apache-2.0 |
| assets/archive/icons/macos-v1/source/icon.vectorcraft | SolidCraft contributors | editable VectorCraft source for assets/archive/icons/macos-v1/exports/icon.svg, with named face and background layers | MIT OR Apache-2.0 |
| assets/archive/icons/macos-v1/exports/icon.png | SolidCraft contributors | 1024 × 1024 transparent sRGB export of assets/archive/icons/macos-v1/exports/icon.svg from VectorCraft | MIT OR Apache-2.0 |
| assets/icons/app/macos/exports/icon.icns | SolidCraft contributors | macOS fallback compiled by Apple's actool from the approved assets/icons/app/macos/source/SolidCraft.icon composition | MIT OR Apache-2.0 |
| assets/archive/icons/macos-v1/exports/icon.iconset/icon_16x16.png | SolidCraft contributors | native sips export from assets/archive/icons/macos-v1/exports/icon.png | MIT OR Apache-2.0 |
| assets/archive/icons/macos-v1/exports/icon.iconset/icon_16x16@2x.png | SolidCraft contributors | native sips export from assets/archive/icons/macos-v1/exports/icon.png | MIT OR Apache-2.0 |
| assets/archive/icons/macos-v1/exports/icon.iconset/icon_32x32.png | SolidCraft contributors | native sips export from assets/archive/icons/macos-v1/exports/icon.png | MIT OR Apache-2.0 |
| assets/archive/icons/macos-v1/exports/icon.iconset/icon_32x32@2x.png | SolidCraft contributors | native sips export from assets/archive/icons/macos-v1/exports/icon.png | MIT OR Apache-2.0 |
| assets/archive/icons/macos-v1/exports/icon.iconset/icon_128x128.png | SolidCraft contributors | native sips export from assets/archive/icons/macos-v1/exports/icon.png | MIT OR Apache-2.0 |
| assets/archive/icons/macos-v1/exports/icon.iconset/icon_128x128@2x.png | SolidCraft contributors | native sips export from assets/archive/icons/macos-v1/exports/icon.png | MIT OR Apache-2.0 |
| assets/archive/icons/macos-v1/exports/icon.iconset/icon_256x256.png | SolidCraft contributors | native sips export from assets/archive/icons/macos-v1/exports/icon.png | MIT OR Apache-2.0 |
| assets/archive/icons/macos-v1/exports/icon.iconset/icon_256x256@2x.png | SolidCraft contributors | native sips export from assets/archive/icons/macos-v1/exports/icon.png | MIT OR Apache-2.0 |
| assets/archive/icons/macos-v1/exports/icon.iconset/icon_512x512.png | SolidCraft contributors | native sips export from assets/archive/icons/macos-v1/exports/icon.png | MIT OR Apache-2.0 |
| assets/archive/icons/macos-v1/exports/icon.iconset/icon_512x512@2x.png | SolidCraft contributors | native sips export from assets/archive/icons/macos-v1/exports/icon.png | MIT OR Apache-2.0 |
| docs/screenshots/class-a-blend.png | SolidCraft contributors | screenshot of SolidCraft itself: two boxes bridged by `surface.blend` (a 40x10 sheet, `solidcraft-cli snapshot` of plan/evidence script ev_blend) | MIT OR Apache-2.0 |
| docs/screenshots/class-a-crown.png | SolidCraft contributors | screenshot of SolidCraft itself: a patch crowned with `surface.patch crown=0.3` over a saddle rim (`solidcraft-cli snapshot`) | MIT OR Apache-2.0 |
| docs/screenshots/class-a-boundary.png | SolidCraft contributors | screenshot of SolidCraft itself: a patch through two sketch lines built by `surface.boundary` with `crown: 0.4` (`solidcraft-cli snapshot`) | MIT OR Apache-2.0 |
| docs/screenshots/sample-plate.png | SolidCraft contributors | screenshot of SolidCraft itself (`solidcraft --sample`) | MIT OR Apache-2.0 |
| docs/dev/f3d-screenshots/christmas-f3d-open.png | SolidCraft contributors | desktop screenshot: File → Open of a Fusion `.f3d` (Christmas tree, solid B-rep bodies) | MIT OR Apache-2.0 |
| docs/dev/f3d-screenshots/cylindrical-holder-f3d-open.png | SolidCraft contributors | desktop screenshot: File → Open of a Fusion `.f3d` (21700 cylindrical holder, solid B-rep bodies) | MIT OR Apache-2.0 |
| docs/screenshots/demo-0.8.gif | SolidCraft contributors | original screen recording of the actual SolidCraft 0.8 English desktop UI modelling an original Raspberry Pi 4 case (no logos or branding; the engraved text is set in the system's DejaVu Sans Bold), generated with examples/demo/record_ui.sh and its original animated pointer overlay (9× playback) | MIT OR Apache-2.0 |
| docs/screenshots/demo-0.8.mp4 | SolidCraft contributors | original screen recording of the actual SolidCraft 0.8 English desktop UI modelling an original Raspberry Pi 4 case (no logos or branding; the engraved text is set in the system's DejaVu Sans Bold), generated with examples/demo/record_ui.sh and its original animated pointer overlay (9× playback) | MIT OR Apache-2.0 |
| docs/screenshots/rover-build.gif | 0xlillith (model); SolidCraft contributors (recording) | screen recording of the actual SolidCraft desktop UI replaying the timeline of the SELENE rover (`Rover-recorded.solidcraft` from https://github.com/0xlillith/solidcraft-rover, an original model by 0xlillith built through SolidCraft MCP); the model's lettering follows Ubuntu Light outlines (Canonical Ltd., Ubuntu Font Licence 1.0, as noted in that repository); the emblem is the SolidCraft project icon (2× playback) | MIT OR Apache-2.0 |
| docs/screenshots/rover-build.mp4 | 0xlillith (model); SolidCraft contributors (recording) | screen recording of the actual SolidCraft desktop UI replaying the timeline of the SELENE rover (`Rover-recorded.solidcraft` from https://github.com/0xlillith/solidcraft-rover, an original model by 0xlillith built through SolidCraft MCP); the model's lettering follows Ubuntu Light outlines (Canonical Ltd., Ubuntu Font Licence 1.0, as noted in that repository); the emblem is the SolidCraft project icon | MIT OR Apache-2.0 |
| assets/branding/web/source/og.png | SolidCraft contributors | original 1280 × 640 social preview card (the SolidCraft mark, name and tagline), served by the docs site for link previews | MIT OR Apache-2.0 |
| docs/book/src/generated/sample-plate.png | SolidCraft contributors | copy of docs/screenshots/sample-plate.png made by `cargo xtask book` (gitignored) | MIT OR Apache-2.0 |
| docs/book/src/generated/bracket.png | SolidCraft contributors | rendered by `cargo xtask book` (`solidcraft-cli snapshot examples/bracket.json`; gitignored) | MIT OR Apache-2.0 |
| docs/book/src/generated/enclosure.png | SolidCraft contributors | rendered by `cargo xtask book` (`solidcraft-cli snapshot docs/book/scripts/enclosure.json`; gitignored) | MIT OR Apache-2.0 |
| docs/book/src/generated/revolve.png | SolidCraft contributors | rendered by `cargo xtask book` (`solidcraft-cli snapshot docs/book/scripts/revolve.json`; gitignored) | MIT OR Apache-2.0 |
| docs/book/src/generated/sheet-metal.png | SolidCraft contributors | rendered by `cargo xtask book` (`solidcraft-cli snapshot docs/book/scripts/sheet-metal.json`; gitignored) | MIT OR Apache-2.0 |
| assets/fonts/liberation-sans/LiberationSans-Regular.ttf | Red Hat, Inc. (Liberation Fonts 2.x; digitized data Google) | https://github.com/liberationfonts (Debian fonts-liberation), unmodified; licence text in assets/fonts/liberation-sans/OFL.txt; used for sketch Text outlines | SIL OFL 1.1 |
| assets/fonts/biz-udp-gothic/BIZUDPGothic-Regular.ttf | The BIZ UDGothic Project Authors | https://github.com/googlefonts/morisawa-biz-ud-gothic (Google Fonts), unmodified; licence text in assets/fonts/biz-udp-gothic/OFL.txt; used for web UI Japanese font | SIL OFL 1.1 |
| tests/oracle/17-loft/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/17-loft/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/23-bracket-multifeature/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/23-bracket-multifeature/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/28-blind-pocket-fillets/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/28-blind-pocket-fillets/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/30-fillet-after-shell/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/30-fillet-after-shell/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/31-fillet-l-all-edges/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/31-fillet-l-all-edges/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/34-chamfer-then-fillet/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/34-chamfer-then-fillet/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/35-fillet-curved-curved/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/35-fillet-curved-curved/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/36-corner-blend-two-radii/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/36-corner-blend-two-radii/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/37-pyramid-apex-blend/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/37-pyramid-apex-blend/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/39-tee-pipe/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/39-tee-pipe/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/41-tube-radial-holes/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/41-tube-radial-holes/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/42-overlapping-patterns/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/42-overlapping-patterns/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/47-draft-curved-pocket/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/47-draft-curved-pocket/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/51-loft-rect-circle-offset/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/51-loft-rect-circle-offset/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/56-holes-on-curved-face/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/56-holes-on-curved-face/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/58-phone-stand/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/58-phone-stand/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/59-enclosure-base/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/59-enclosure-base/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/61-shaft-keyway/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/61-shaft-keyway/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/63-hinge-knuckle/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/63-hinge-knuckle/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/65-sketch-gear-tooth/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/65-sketch-gear-tooth/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |

| assets/appearances/library.rs | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/fonts/biz-udp-gothic/OFL.txt | The BIZ UDGothic Project Authors | Unmodified accompanying font licence | SIL OFL 1.1 |
| assets/fonts/liberation-sans/OFL.txt | Red Hat, Inc.; Liberation Fonts contributors | Unmodified accompanying font licence | SIL OFL 1.1 |
| assets/icons/app/linux/exports/hicolor/128x128/apps/ai.storyteller.solidcraft.png | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/linux/exports/hicolor/16x16/apps/ai.storyteller.solidcraft.png | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/linux/exports/hicolor/24x24/apps/ai.storyteller.solidcraft.png | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/linux/exports/hicolor/256x256/apps/ai.storyteller.solidcraft.png | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/linux/exports/hicolor/32x32/apps/ai.storyteller.solidcraft.png | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/linux/exports/hicolor/48x48/apps/ai.storyteller.solidcraft.png | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/linux/exports/hicolor/512x512/apps/ai.storyteller.solidcraft.png | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/linux/exports/hicolor/64x64/apps/ai.storyteller.solidcraft.png | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/shared/source/app_icon.rs | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/icons/app/shared/source/windows_ico.rs | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/icons/app/windows/exports/solidcraft-executable.ico | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/windows/exports/solidcraft.ico | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/ui/source/glyphs.rs | SolidCraft / VectorCraft contributors | Original glyphs; caption geometry adapted from VectorCraft titlebar.rs (https://github.com/storytold/vectorcraft) | MIT OR Apache-2.0 |
| assets/icons/ui/source/icons.rs | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/icons/ui/source/markers.rs | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/icons/ui/source/navigation.rs | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/icons/ui/source/pointer.rs | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/locales/de.json | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/locales/en.json | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/locales/fr.json | rlartot | French translation of the English catalog, contributed upstream | MIT OR Apache-2.0 |
| assets/locales/ja.json | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/locales/ko.json | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/locales/ru.json | Alex Pro (del000) | Russian translation of the English catalog, contributed upstream | MIT OR Apache-2.0 |
| assets/locales/pt-BR.json | DuBearte | Brazilian Portuguese translation of the English catalog, contributed upstream | MIT OR Apache-2.0 |
| assets/locales/tr.json | macrulz54 | Turkish translation of the English catalog, contributed upstream | MIT OR Apache-2.0 |
| assets/locales/uk.json | Igor Bronovskyi (BrunIF) | Ukrainian translation of the English catalog, contributed upstream | MIT OR Apache-2.0 |
| assets/locales/zh.json | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/materials/library.rs | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/platforms/linux/ai.storyteller.solidcraft.desktop | PhotoCraft / SolidCraft contributors | PhotoCraft packaging templates (https://github.com/storytold/photocraft), adapted for SolidCraft | Apache-2.0 |
| assets/platforms/linux/ai.storyteller.solidcraft.metainfo.xml.in | PhotoCraft / SolidCraft contributors | PhotoCraft packaging templates (https://github.com/storytold/photocraft), adapted for SolidCraft | Apache-2.0 |
| assets/platforms/linux/ai.storyteller.solidcraft.mime.xml | PhotoCraft / SolidCraft contributors | PhotoCraft packaging templates (https://github.com/storytold/photocraft), adapted for SolidCraft | Apache-2.0 |
| assets/platforms/macos/Info.plist.in | PhotoCraft / SolidCraft contributors | PhotoCraft packaging templates (https://github.com/storytold/photocraft), adapted for SolidCraft | Apache-2.0 |
| assets/platforms/macos/README.txt | PhotoCraft / SolidCraft contributors | PhotoCraft packaging templates (https://github.com/storytold/photocraft), adapted for SolidCraft | Apache-2.0 |
| assets/platforms/windows/solidcraft.en-us.wxl | PhotoCraft / SolidCraft contributors | PhotoCraft packaging templates (https://github.com/storytold/photocraft), adapted for SolidCraft | Apache-2.0 |
| assets/platforms/windows/solidcraft.wxs | PhotoCraft / SolidCraft contributors | PhotoCraft packaging templates (https://github.com/storytold/photocraft), adapted for SolidCraft | Apache-2.0 |
| assets/shaders/viewport.wgsl | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/styles/ui/palettes.rs | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/styles/web/loading.css | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| crates/io/src/native_cad/fixtures/block.FCStd | SolidCraft contributors | original 10 × 20 × 30 mm test box saved by FreeCAD 1.1.4 run as an external program (`make_block_fcstd.py` beside it); no third-party geometry | MIT OR Apache-2.0 |
| crates/io/src/native_cad/fixtures/block.3dm | SolidCraft contributors | original 10 × 20 × 30 mm test box written with McNeel's MIT-licensed rhino3dm Python package (`make_block_3dm.py` beside it); no third-party geometry | MIT OR Apache-2.0 |

| docs/book/src/og.png | SolidCraft contributors | copy of assets/branding/web/source/og.png staged by cargo xtask book (gitignored) | MIT OR Apache-2.0 |

## Code adapted from sibling projects

| SolidCraft file | Adapted from | Licence |
|---|---|---|
| xtask/src/layers.rs | CADCraft `xtask/src/layers.rs` (dependency layering rules and tests) | MIT OR Apache-2.0 |
| apps/solidcraft/src/control_server.rs | CADCraft `apps/cadcraft/src/control_server.rs` (loopback JSON-lines server) | MIT OR Apache-2.0 |
| crates/ui-egui/src/lib.rs (screenshot queue, synthetic input) | CADCraft `crates/ui-egui/src/lib.rs` | MIT OR Apache-2.0 |
| crates/ui-egui/src/control.rs (method dispatch and screenshot saving) | CADCraft `crates/ui-egui/src/control.rs` | MIT OR Apache-2.0 |
| crates/ui-egui/src/gpu.rs (paint-callback structure) | CADCraft `crates/ui-egui/src/gpu.rs` | MIT OR Apache-2.0 |
| crates/mcp/src/server.rs, crates/mcp/src/backend.rs (JSON-RPC framing, MCP lifecycle, control-port bridge) | CADCraft `crates/mcp/src/{server,backend}.rs` | MIT OR Apache-2.0 |
| crates/mcp/src/tools.rs (tool schema helpers and argument checking) | CADCraft `crates/mcp/src/tools.rs`, GridCraft `crates/mcp/src/tools.rs` | MIT OR Apache-2.0 |
| crates/io/src/threemf.rs (bounded zip part reading and writing) | GridCraft `crates/xlsx/src/package.rs` | MIT OR Apache-2.0 |
| crates/ui-egui/src/titlebar.rs (title bar drag, caption buttons, resize zones, their tests) and the window setup in apps/solidcraft/src/main.rs | VectorCraft `crates/ui-egui/src/titlebar.rs`, `apps/vectorcraft/src/main.rs` | MIT OR Apache-2.0 |
| apps/solidcraft/src/open_documents.rs (macOS: files opened from Finder and the Dock, its test) | VectorCraft `apps/vectorcraft/src/open_documents.rs` (https://github.com/storytold/vectorcraft) | MIT OR Apache-2.0 |
| LICENSE-MIT, LICENSE-APACHE (licence text) | CADCraft | — |
| .github/workflows/release.yml, .github/workflows/packaging-lint.yml and packaging/ (release pipeline: Linux AppImage/deb/rpm/Flatpak/tar.gz, FreeBSD, macOS dmg, Windows MSI/portable zip, web zip; AppRun, nfpm, Flatpak, WiX and Info.plist templates) | PhotoCraft (https://github.com/storytold/photocraft) `.github/workflows/{release,packaging-lint}.yml`, `packaging/` | Apache-2.0 |

## Incremental HDR codec adapter

`crates/render/src/lighting/hdr.rs` adapts the scanline/component/old-RLE decoding
state machines from the Radiance decoder in `image` 0.25.10
([source](https://docs.rs/crate/image/0.25.10/source/src/codecs/hdr/decoder.rs)),
MIT OR Apache-2.0. Metadata parsing still uses that library. The adapter makes row
work cooperative and adds checked run lengths, zero-run rejection, width validation,
header/input/radiance bounds and cancellation. Original licence texts are retained
in THIRD-PARTY-LICENSES.txt; no images or artwork from that project are copied.

## File-format crates

| Crate | Used for | Licence |
|---|---|---|
| gltf 1.4.1, gltf-json 1.4.1, gltf-derive 1.4.1 (https://github.com/gltf-rs/gltf) | glTF 2.0 import (crates/io/src/gltf_in.rs): the JSON document model and its reference checks; the GLB container, buffers and accessors are read by SolidCraft's own code | MIT OR Apache-2.0 |
| inflections 1.1.1 (build-time, through gltf-derive) | case conversion in gltf's derive macros | MIT |
| lazy_static 1.5.1 (through gltf-json) | | MIT OR Apache-2.0 |

The glTF test files are built by the tests themselves (crates/io/src/gltf_in_tests.rs,
crates/engine/src/tests/gltf_import.rs); no third-party models are committed.

## FreeCAD, Rhino and Fusion import library

FreeCAD `.FCStd` and Rhino `.3dm` files are read with [cadmpeg](https://github.com/cadmpeg/cadmpeg)
0.6.1 (Apache-2.0), linked as the crates `cadmpeg-core`, `cadmpeg-ir`, `cadmpeg-registry`,
`cadmpeg-container`, `cadmpeg-codec-freecad`, `cadmpeg-codec-rhino` and `cadmpeg-codec-step`
in native builds (`crates/io/src/native_cad.rs`). Those open-format codecs are enabled through
cadmpeg-registry without default features (no ShapeManager/ACIS `sat` codec). Autodesk Fusion
`.f3d`/`.f3z` files are read separately via `cadmpeg-codec-f3d` and a patched
`cadmpeg-codec-step` / `cadmpeg-core` (`crates/io/src/f3d.rs`, vendored under `vendor/`).
Its licence text is in THIRD-PARTY-LICENSES.txt.

## Recovered contributions

| SolidCraft file | Original contribution | Licence |
|---|---|---|
| crates/ui-egui/src/viewport.rs (`nearest_quarter_turn`, `axis_view_orbit`) | N3MI-DG's [PR #101](https://github.com/bherbruck/solidcraft/pull/101): [minimal TOP/BOTTOM turns](https://github.com/bherbruck/solidcraft/commit/c28e780f52490784ef5886f094b9d3f6d10c3e3f) and [drag-to-orbit](https://github.com/bherbruck/solidcraft/commit/f30b30be6ecc4e2e6544a851243137c01a354934), adapted to the axis gizmo | MIT OR Apache-2.0 |

## Vendored third-party code

| Directory | Source | Licence | Changes |
|---|---|---|---|
| vendor/truck-meshalgo | truck-meshalgo 0.4.0, https://github.com/ricosjp/truck (RICOS Co. Ltd.) | Apache-2.0 | meshing: bounded parameter searches, collapsed-boundary projection, subdivision and provisional fixed-grid previews (vendor/README.md) |
| vendor/truck-stepio | truck-stepio 0.3.0, https://github.com/ricosjp/truck (RICOS Co. Ltd.) | Apache-2.0 | STEP writer: intersection-curve surfaces written at unique ids (vendor/README.md) |
| vendor/truck-shapeops | truck-shapeops 0.4.0, https://github.com/ricosjp/truck (RICOS Co. Ltd.) | Apache-2.0 | robust meshing and nearest-parameter fallback in booleans; degenerate intersection-curve rejection; face and edge history of booleans; trace macro; corner choice at vertices several intersection curves end on (vendor/README.md) |
| vendor/truck-geometry | truck-geometry 0.5.0, https://github.com/ricosjp/truck (RICOS Co. Ltd.) | Apache-2.0 | intersection curves divided from their neighbours' surface parameters (vendor/README.md) |
| vendor/matext4cgmath | matext4cgmath 0.1.0, https://github.com/ricosjp/matext4cgmath (RICOS Co. Ltd.) | Apache-2.0 | root refinement stops after 100 Newton steps (vendor/README.md) |
| vendor/cadmpeg-codec-f3d | cadmpeg-codec-f3d 0.6.1, https://github.com/cadmpeg/cadmpeg | Apache-2.0 | arrangement walk budget; skip unparsable sketch points (vendor/README.md) |
| vendor/cadmpeg-codec-step | cadmpeg-codec-step 0.6.1, https://github.com/cadmpeg/cadmpeg | Apache-2.0 | cone semi-angle 0 exported as cylinder (vendor/README.md) |
| vendor/cadmpeg-core | cadmpeg-core 0.6.1, https://github.com/cadmpeg/cadmpeg | Apache-2.0 | higher retained/materialized decode multipliers (vendor/README.md) |

## Vendored renderer compatibility fix

`vendor/egui-wgpu` is egui-wgpu 0.36.2 from https://github.com/emilk/egui
(MIT OR Apache-2.0; both original licence texts are included). Rust sources and
its original `egui.wgsl` shader are retained; no images or other artwork are copied.
SolidCraft's narrow patch selects a safe 1x MSAA path on the chosen OpenGL adapter
and shares that effective count with the surface attachments and CAD callbacks.
Vulkan, DirectX 12, Metal and WebGPU retain their requested multisampling.
A frame whose meshes the GPU refuses to stage (device lost or out of memory) is
skipped with a warning instead of panicking (#1469).
