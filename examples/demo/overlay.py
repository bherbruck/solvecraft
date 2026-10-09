#!/usr/bin/env python3
"""overlay.py LOG START OUTDIR: from enclosure_ui_mcp.py's --log (captions and pointer events)
and the recording's start (epoch seconds), write OUTDIR/captions.ass, OUTDIR/cursor.cmd
(ffmpeg sendcmd moves for the drawn pointer and the click ripple) and the two sprites
(cursor.png, ripple.png). record_v2.sh composites them onto the screen recording."""

import json
import math
import os
import struct
import sys
import zlib


def png(path, w, h, pixel):
    """A small RGBA PNG, pixel(x, y) -> (r, g, b, a)."""
    raw = b"".join(b"\0" + b"".join(bytes(pixel(x, y)) for x in range(w)) for y in range(h))

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    open(path, "wb").write(data)


def inside(px, py, poly):
    c = False
    for i in range(len(poly)):
        (x1, y1), (x2, y2) = poly[i], poly[i - 1]
        if (y1 > py) != (y2 > py) and px < (x2 - x1) * (py - y1) / (y2 - y1) + x1:
            c = not c
    return c


def cursor_sprite(path):
    # An arrow pointer (tip at 1,1): white with a dark outline, drawn 2x and box-filtered.
    arrow = [(1, 1), (1, 23), (6.5, 18), (10, 26), (13.5, 24.5), (10, 17), (17, 17)]
    s = 4

    def cover(x, y, poly, grow):
        n = 0
        for i in range(s):
            for j in range(s):
                px, py = x + (i + 0.5) / s, y + (j + 0.5) / s
                if grow:
                    n += any(inside(px + dx, py + dy, poly) for dx, dy in ((-1.3, 0), (1.3, 0), (0, -1.3), (0, 1.3), (0, 0)))
                else:
                    n += inside(px, py, poly)
        return n / (s * s)

    def pixel(x, y):
        fill, edge = cover(x, y, arrow, False), cover(x, y, arrow, True)
        a = max(fill, edge)
        if a == 0:
            return (0, 0, 0, 0)
        v = int(255 * fill / a) if a else 0
        return (v, v, v, int(255 * a))

    png(path, 22, 30, pixel)


def ripple_sprite(path):
    r = 22

    def pixel(x, y):
        d = math.hypot(x + 0.5 - r, y + 0.5 - r)
        ring = max(0.0, 1 - abs(d - (r - 5)) / 2.2)
        disc = 0.25 if d < r - 6 else 0.0
        a = max(ring, disc)
        return (90, 170, 255, int(255 * a))

    png(path, 2 * r, 2 * r, pixel)


def ts(t):
    t = max(t, 0.0)
    return f"{int(t // 3600)}:{int(t % 3600 // 60):02}:{t % 60:05.2f}"


def main():
    log, start, out = sys.argv[1], float(sys.argv[2]), sys.argv[3]
    marks = [json.loads(line) for line in open(log) if line.strip()]
    cursor_sprite(os.path.join(out, "cursor.png"))
    ripple_sprite(os.path.join(out, "ripple.png"))

    # Captions: the step, and under it how it was sent (UI click / MCP call) in its own colour.
    caps = [m for m in marks if "caption" in m]
    head = """[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Step,DejaVu Sans,36,&H00FFFFFF,&H00FFFFFF,&H00000000,&H90000000,1,0,0,0,100,100,0,0,3,10,0,8,0,0,150,1
Style: Via,DejaVu Sans Mono,24,&H00FFFFFF,&H00FFFFFF,&H00000000,&H90000000,0,0,0,0,100,100,0,0,3,8,0,8,0,0,210,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
"""
    lines = []
    for i, m in enumerate(caps):
        if not m["caption"]:
            continue
        a = m["t"] - start
        b = (caps[i + 1]["t"] - start) if i + 1 < len(caps) else a + 3
        lines.append(f"Dialogue: 0,{ts(a)},{ts(b)},Step,,0,0,0,,{m['caption']}")
        via = m.get("via", "")
        if via:
            colour = "&H0090E8A0&" if via.startswith("UI") else "&H0050C8FF&"
            lines.append(f"Dialogue: 0,{ts(a)},{ts(b)},Via,,0,0,0,,{{\\c{colour}}}{via}")
    open(os.path.join(out, "captions.ass"), "w").write(head + "\n".join(lines) + "\n")

    # Pointer: every move as logged; a drag's path spread over the time the app took to play it.
    pts = []
    for i, m in enumerate(marks):
        t = m["t"] - start
        if "cursor" in m:
            pts.append((t, m["cursor"][0], m["cursor"][1]))
        elif "drag_start" in m:
            end = next((n for n in marks[i + 1:] if "drag_end" in n), None)
            if end:
                (x0, y0), (x1, y1), t1 = m["drag_start"], end["drag_end"], end["t"] - start
                n = max(2, int((t1 - t) * 30))
                for k in range(n + 1):
                    u = k / n
                    pts.append((t + (t1 - t) * u, x0 + (x1 - x0) * u, y0 + (y1 - y0) * u))
    pts.sort()
    cmds = []
    for t, x, y in pts:
        cmds.append(f"{t:.3f} [enter] overlay@cur x {x:.1f}, [enter] overlay@cur y {y:.1f};")
    for m in marks:
        if m.get("click"):
            t = m["t"] - start
            x, y = m["cursor"]
            cmds.append(f"{t:.3f} [enter] overlay@rip x {x - 22:.1f}, [enter] overlay@rip y {y - 22:.1f};")
    open(os.path.join(out, "cursor.cmd"), "w").write("\n".join(sorted(cmds, key=lambda c: float(c.split()[0]))) + "\n")
    clicks = [m["t"] - start for m in marks if m.get("click")]
    open(os.path.join(out, "ripple.enable"), "w").write("+".join(f"between(t,{c:.3f},{c + 0.28:.3f})" for c in clicks) or "0")
    first = pts[0] if pts else (0, -100, -100)
    open(os.path.join(out, "cursor.start"), "w").write(f"{first[1]:.1f} {first[2]:.1f}\n")


if __name__ == "__main__":
    main()
