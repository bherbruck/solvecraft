# Laser mark → LightBurn (SolidCraft)

SolidCraft exports a **LightBurn handoff package** (DXF/SVG + job JSON). LightBurn (or Mach/Marlin hosts) own the machine connection; SolidCraft does not drive hardware in v1.

Built-in profiles cite vendor / LightBurn published beds. See each profile’s `source` from `laser.profiles`.

## Package contents

| File | Role |
|------|------|
| `{stem}.dxf` | ASCII R12 LINE geometry (mm) |
| `{stem}.svg` | Same paths |
| `{stem}.laser-job.json` | Profile, bed WxH, LightBurn device/model, notes, source |

**Commands:** `laser.profiles`, `laser.exportPackage` (`profile?`, `field_mm?` square override, `path?`, `sketch?` / `polylines?`).

## Profile catalog

### ComMarker (JCZFiber)

| Id | Default bed | Notes |
|----|-------------|--------|
| `commarker-60w-mopa` | 110×110 | MOPA; lens options 70–300 |
| `commarker-b4` | 110×110 | Fiber; lenses to 400 |
| `commarker-b6` | 150×150 | Fiber/MOPA family |
| `commarker-omni-uv` | 150×150 | UV; also 70×70 |

### xTool (LightBurn `xTool` type)

| Id | Bed | Model |
|----|-----|--------|
| `xtool-f1` | 115×115 | F1 |
| `xtool-f1-ultra` | 220×220 | F1 Ultra |
| `xtool-d1` / `xtool-d1-pro` | 432×406 | D1 / D1 Pro |
| `xtool-s1` | 498×319 | S1 |
| `xtool-p2` | 600×305 | P2 / P2S |

### Snapmaker (Marlin; vendor `.lbdev`)

| Id | Bed | Controller hint |
|----|-----|-----------------|
| `snapmaker-artisan` | 400×400 | Marlin / Artisan lbdev |
| `snapmaker-a350` | 320×350 | Marlin / 2.0 (A350/F350) |
| `snapmaker-a250` | 230×250 | Marlin / 2.0 (A250/F250) |
| `snapmaker-ray` | 400×400 | GRBL / Ray lbdev |

### Makera

| Id | Bed | Hint |
|----|-----|------|
| `makera-carvera` | 360×240 | GRBL-class + 2.5W diode |
| `makera-carvera-air` | 300×200 | optional 5W diode |

### ChinaCNCzone ([china-cncrouter.com](https://www.china-cncrouter.com/)) — Mach3-class

| Id | Bed | Hint |
|----|-----|------|
| `chinacnc-3040` | 300×400 | Mach3 mini router |
| `chinacnc-6040` | 400×600 | Mach3 |
| `chinacnc-6090` | 600×900 | Mach3/DSP (effective travel may be ~580×880) |

These are the common cheap beds; clones vary — confirm travel on your machine.

## Non-goals (v1)

In-app USB/WebSerial drive; replacing LightBurn process control; VectorCraft host.

## Future

Serial/USB/Bluetooth transport traits (native + web providers); more models as specs are confirmed.
