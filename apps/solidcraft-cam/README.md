# solidcraft-cam

Secured loopback HTTP companion that bridges the SolidCraft manufacture domain
to FreeCAD Path / ocp-freecad-cam via a Python subprocess adapter.

## Quick start

```sh
# Build
cargo build -p solidcraft-cam

# Run (binds to 127.0.0.1 only; prints token + address)
./target/debug/solidcraft-cam --freecad-exec freecadcmd

# Output (one line):
# solidcraft-cam token=<64-char hex> address=http://127.0.0.1:<PORT>
```

## Security model

| Property | Detail |
|---|---|
| Loopback only | Binds to `127.0.0.1` (IPv4) only. |
| Bearer token | Random 32-byte token printed once to stdout at launch. |
| No URL tokens | Token is **never** accepted in URL query parameters or path. |
| CORS allowlist | Strict `Origin` parse; userinfo (`@`) and paths are rejected. |
| PNA support | `Access-Control-Allow-Private-Network: true` on matching preflight. |
| Bounded I/O | Request body ≤ 64 MiB; STEP ≤ 50 MiB; result ≤ 10 MiB. |
| Path jail | Per-job temp directory; no path traversal possible. |
| No shell | Subprocess spawned with `Command::new()`, never via a shell string. |
| Process cleanup | Child process is killed on cancel or timeout; temp dir removed. |

## API

All authenticated routes require `Authorization: Bearer <TOKEN>`.

| Method | Path | Auth | Description |
|---|---|---|---|
| GET | `/version` | — | Version and pinned dependency info |
| GET | `/cam/v1/health` | ✓ | Backend availability probe |
| POST | `/cam/v1/jobs` | ✓ | Submit a CAM job |
| GET | `/cam/v1/jobs/:id/status` | ✓ | Poll job status |
| GET | `/cam/v1/jobs/:id/result` | ✓ | Retrieve result when done |
| DELETE | `/cam/v1/jobs/:id` | ✓ | Cancel a running or pending job |

### Submit job

```json
POST /cam/v1/jobs
Authorization: Bearer <TOKEN>
Content-Type: application/json

{
  "step_text": "<ISO 10303-21 STEP file content>",
  "manufacture": { <ManufactureSettings> }
}
```

Alternatively use `"step_b64"` (standard base64) instead of `"step_text"`.

Returns `202 Accepted` with `{ "job_id": "<uuid>", "status": "pending" }`.

### Result

```json
{
  "job_id": "...",
  "status": "done",
  "result": {
    "segments": [
      { "kind": "rapid", "to": [x, y, z] },
      { "kind": "linear", "to": [x, y, z], "feed": 1000.0 },
      { "kind": "arc", "center": [cx,cy,cz], "to": [x,y,z], "feed": 800.0, "clockwise": false }
    ],
    "post_output": "G21\nG0 X0 Y0 Z5\n..."
  }
}
```

## Geometric selection limitation

`ManufactureSettings` (crates/manufacture) stores operation parameters (depth, feedrate, …)
but not face/edge/hole identifiers from the STEP model geometry.

| Operation | Status | Detail |
|---|---|---|
| `face` | ✓ Automatable | Auto-selects highest-Z face |
| `profile` | ✗ PreflightError | Needs `edge_ids` in `ProfileOperation` |
| `pocket` | ✗ PreflightError | Needs `face_ids` in `PocketOperation` |
| `drill` | ✗ PreflightError | Needs `hole_face_ids` in `DrillOperation` |
| `adaptive` | ✗ PreflightError | Needs `face_ids` in `AdaptiveOperation` |

Preflight failures return HTTP 400 with a `"preflight_error"` adapter status
and a message describing exactly which domain field must be extended.

## FreeCAD setup

The companion calls `freecad_cam_adapter.py` through FreeCAD's Python
interpreter.  Install FreeCAD and ocp-freecad-cam in that interpreter:

```sh
# Pinned versions (see src/config.rs)
# FreeCAD: 1.1.1
# ocp-freecad-cam: 2.0.2 (requires cadquery 2.4+)
pip install ocp-freecad-cam==2.0.2   # inside FreeCAD's Python
pip install cadquery                  # required by ocp-freecad-cam 2.x
```

Point the companion at the right executable:

```sh
solidcraft-cam --freecad-exec /opt/FreeCAD/bin/python --adapter adapter/freecad_cam_adapter.py
```

When FreeCAD is not installed the adapter returns `{ "status": "unavailable" }`
and `GET /cam/v1/health` reports `"backend_available": false`.

## CLI flags

```
--port <PORT>          Port to bind (default: 0, OS-assigned ephemeral)
--origin <ORIGIN>      Allowed CORS origin, repeatable
--temp-dir <PATH>      Base dir for per-job temp workspaces
--freecad-exec <PATH>  FreeCAD Python executable (default: freecadcmd)
--adapter <PATH>       Path to freecad_cam_adapter.py
--timeout <SECS>       Per-job subprocess timeout in seconds (default: 300)
--token <TOKEN>        Fixed bearer token (for automated testing only)
--help                 Print help
```
