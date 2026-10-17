# Manufacture jobs (CNC)

SolidCraft's Manufacture workspace prepares 3-axis mill/router jobs, runs toolpath generation through a local **solidcraft-cam** companion, simulates removal on a bounded height field, and exports review G-code.

## Architecture

- **Document (format 22+)**: `manufacture` settings — setups (stock, WCS, tools, operations), machine profiles, post profiles.
- **Engine**: `manufacture.*` commands (settings, preflight, start/status/cancel/result, simulate, gcode, export).
- **Companion**: [`apps/solidcraft-cam`](../apps/solidcraft-cam/) — loopback HTTP API, FreeCAD Path via `ocp-freecad-cam` 2.0.2 (pinned).
- **UI**: Manufacture workspace; cross-platform HTTP client (`ehttp`) with bearer token in `Authorization` only.

3D printing is **not** part of Manufacture — export 3MF from Design and open in OrcaSlicer.
Galvo fiber laser mark is also separate — see [laser-lightburn.md](laser-lightburn.md) (`laser.*`, LightBurn package).

## Starting the companion

```text
solidcraft-cam --origin http://127.0.0.1:8099
```

The process prints a bearer token and binds to loopback. In Manufacture → Setup, set:

- **Endpoint**: `http://127.0.0.1:38473` (default)
- **Token**: paste the token (session only; never stored on disk)

For the browser build (`/app-move/`), pass `--origin` with the exact page origin (scheme, host, port). The companion rejects userinfo URLs and non-loopback hosts.

## Job lifecycle

1. `manufacture.preflight` — STEP snapshot + setup validation (no queue).
2. `manufacture.start` — immutable snapshot + bounded session job.
3. Poll `manufacture.status` (in-memory; no HTTP from status itself). The UI calls `CamHost::pump()` to drive companion HTTP.
4. `manufacture.result` — normalised `PathSegment`s and optional companion `post_output`.
5. `manufacture.simulate` — deterministic 3-axis stock simulation (read-only).
6. `manufacture.gcode` — companion G-code or **local post** from the setup's post profile.
7. `manufacture.export` — atomic write under the agent path scope.

## Post profiles

Built-in dialects: **GRBL**, **Mach3/4**, **LinuxCNC**, **Fanuc**, **Haas** (`manufacture.posts.ensure_presets` / `post::preset_posts()`). Assign a post on the setup (`post: "GRBL"`). Local posting runs when the companion returns segments only (`manufacture.gcode`). Output is for **operator review** — confirm units, WCS, tool numbers, and safe heights on the machine. Golden post fixtures live in `crates/manufacture/src/post.rs` tests.

## Machine profiles (easy OEMs + ATC)

`manufacture.machines.ensure_presets` installs cited machine profiles with **tool-changer** limits and **integration** metadata:

| Preset | ATC | Integration |
|--------|-----|-------------|
| Haas VF-2 / UMC-750 | Side-mount 20 / 40 pockets | MTConnect `:8082` |
| Okuma GENOS M560 | Dual-arm 32 | THINC (+ MTConnect often `:5000`) |
| Fanuc 0i-MF class | Carousel 24 | FOCAS `:8193` (covers DN Solutions Fanuc controls) |
| Mazak VCN-530C | Carousel 30 | MTConnect Smooth |

Skipped on purpose: DMG CELOS partner apps, TRUMPF deep APIs, Makino one-offs — not transparent enough for a first cut.

Soft warnings fire when a setup’s tool count / T-numbers / diameter / length exceed the assigned machine’s ATC limits. Live FOCAS/THINC/MTConnect clients are **not** in v1 — profiles are capability + planning metadata.

Fanuc/Haas (and Mach) local posts emit **`T… M6` + `G43 H…`**. GRBL/LinuxCNC comment the tool (manual change).

## Probe snippets (Inspection Plus)

`manufacture.probe.snippet` emits **review-only** Renishaw Inspection Plus G-code (`G65 P9810`–`P9816`, Haas `P9832`/`P9833` on/off). Cycles: `protected_move`, `single_surface`, `web_pocket`, `bore_boss`, `internal_corner`, `external_corner`. Haas VF-2 / UMC-750 and Fanuc 0i-MF presets mark `probe.present` with style + typical pocket. Confirm macros, stylus calib, and `S` work-offset mapping on the machine — SolidCraft does not drive the probe live.

## Machine plugins (OEM adapters)

Session-only plugins behind `MachineBackend` (same idea as CAM host — not saved in the document):

| Command | Role |
|---------|------|
| `manufacture.machine.plugins` | List fake / mtconnect / planned focas·thinc + active plugin |
| `manufacture.machine.use` | Install `fake`, `mtconnect` (`endpoint`, optional `fixture:true`), or `none` |
| `manufacture.machine.status` | Poll execution / program / tool / spindle / feed |

**Test without hardware:**

```text
manufacture.machine.use {"plugin":"fake"}
manufacture.machine.status {}

manufacture.machine.use {"plugin":"mtconnect","fixture":true,"endpoint":"http://127.0.0.1:8082/VF-2"}
manufacture.machine.status {}
```

Live MTConnect uses loopback-only `StdHttpGet` (`http://127.0.0.1:8082/<MODEL>`). FOCAS / THINC plugins are listed as planned native companions.

In the app (milling ribbon + right panel):

**Ribbon** (while workspace = Manufacture): **SETUP** · **2D** (Face / Adaptive / Pocket / Contour) · **DRILLING** · **ACTIONS** (Generate / Simulate / Post Process) · **MANAGE** (Tool Library / Machine)

**Right panel:**
- **Setup** — machine profile (ATC / probe / integration), live machine plugin, post
- **Tools** — tool library with bit measurements (Ø, corner radius, overall / flute length, flutes, description); ATC overflow warnings when a machine is assigned
- **Operations** — ops list + companion generate
- **Simulate** / **Post** — scrubber and G-code preview

## Geometric selection

Profile, pocket, drill, and adaptive operations require `targets` on the operation (face sample points or drill points). Missing or ambiguous selection fails preflight on the companion with `preflight_error`.

## Pinned external versions

| Component | Pin |
|-----------|-----|
| FreeCAD | 1.1.1 |
| ocp-freecad-cam | 2.0.2 |

See `apps/solidcraft-cam/README.md` for install notes.

## Security notes

- Companion binds **loopback only**; CORS origins are exact-match (scheme/host/port), no userinfo, no path/query/fragment.
- Bearer token is required on mutating and result routes; query-string tokens are rejected.
- JSON bodies are size-capped; job store is bounded (reject when full).
- Browser builds discover the companion when available; toolpath generation fails closed if it is not running.

## Verification

Targeted gates for this slice: `solidcraft-manufacture`, `solidcraft-engine` manufacture commands, `solidcraft-cam` unit + integration (origin/auth/CORS), `solidcraft-ui-egui` check (native tests + `wasm32-unknown-unknown`), and `cargo xtask layers` (manufacture = standalone L0; cam = exempt). Full `cargo xtask ci` remains the merge gate on a complete toolchain.
