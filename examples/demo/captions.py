#!/usr/bin/env python3
"""captions.py LOG START OUT.ass: the step log of enclosure_mcp.py --log as ASS captions
(top centre: the step, under it the MCP call), timed from START (the recording's epoch)."""

import json
import sys

log, start, out = sys.argv[1], float(sys.argv[2]), sys.argv[3]
marks = [json.loads(line) for line in open(log) if line.strip()]
head = """[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Step,DejaVu Sans,34,&H00FFFFFF,&H00FFFFFF,&H00000000,&H80000000,1,0,0,0,100,100,0,0,3,10,0,8,0,0,150,1
Style: Call,DejaVu Sans Mono,22,&H00A0E0FF,&H00FFFFFF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,3,8,0,8,0,0,206,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
"""


def ts(t):
    t = max(t, 0.0)
    return f"{int(t // 3600)}:{int(t % 3600 // 60):02}:{t % 60:05.2f}"


lines = []
for i, m in enumerate(marks):
    if not m["caption"]:
        continue
    a = m["t"] - start
    b = (marks[i + 1]["t"] - start) if i + 1 < len(marks) else a + 3
    lines.append(f"Dialogue: 0,{ts(a)},{ts(b)},Step,,0,0,0,,{m['caption']}")
    if m.get("command"):
        lines.append(f"Dialogue: 0,{ts(a)},{ts(b)},Call,,0,0,0,,MCP  tools/call execute  {m['command']}")
open(out, "w").write(head + "\n".join(lines) + "\n")
