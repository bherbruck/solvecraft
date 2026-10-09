#!/usr/bin/env python3
"""Demo v2: build an electronics enclosure in a running SolveCraft the way a person would, with
the parametric steps sent over MCP.

    solvecraft --control 7878 &
    python3 examples/demo/enclosure_ui_mcp.py --port 7878 --cli solvecraft-cli

Modelling steps go through the app's UI over its control channel: the pointer glides to a
toolbar button and clicks it (the real dialog opens), glides to the face or point it picks and
clicks, and values are typed into the dialog, as by hand. Sketches go on the model's faces and
the camera follows like a user's would (Look At on a new sketch, back out to a 3/4 view on
Finish Sketch, a little orbit between features). The parameters and the dimensions that use them
are MCP `execute` calls through `solvecraft-cli mcp --connect` (enclosure_mcp.py's client).

--log writes one JSON line per caption and per pointer event, which record.sh turns into burnt-in
captions and a drawn cursor (the app gets synthetic pointer events, so the screen's own pointer
never moves).
"""

import argparse
import json
import math
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from enclosure_mcp import Mcp, control  # noqa: E402


class Demo:
    def __init__(self, port, mcp, log, speed):
        self.port, self.mcp, self.log, self.speed = port, mcp, log, speed
        self.pos = None
        self.stop = 10**6

    # -- logging -------------------------------------------------------------------------------
    def mark(self, **k):
        if self.log:
            self.log.write(json.dumps({"t": time.time(), **k}) + "\n")
            self.log.flush()

    def caption(self, text, via=""):
        self.n = getattr(self, "n", 0) + 1
        if self.n > self.stop:
            raise StopIteration
        print(f"  {text}  [{via}]", file=sys.stderr)
        self.mark(caption=text, via=via)

    def wait(self, s):
        time.sleep(s * self.speed)

    # -- the app -------------------------------------------------------------------------------
    def ui(self, method, params=None):
        r = control(self.port, method, params)
        if not r.get("ok", False):
            raise RuntimeError(f"{method} {params}: {r.get('error')}")
        return r.get("result")

    def at(self, **where):
        p = self.ui("ui.at", where)
        return (p[0], p[1])

    def glide(self, to, dur=0.35):
        """Move the pointer to `to` along an eased path (the app sees every step: hover
        highlights follow it)."""
        frm = self.pos or (1150.0, 620.0)
        n = max(2, int(dur * 40))
        for i in range(1, n + 1):
            t = i / n
            e = t * t * (3 - 2 * t)
            x, y = frm[0] + (to[0] - frm[0]) * e, frm[1] + (to[1] - frm[1]) * e
            self.ui("ui.move", {"x": x, "y": y})
            self.mark(cursor=[x, y])
            time.sleep(dur / n)
        self.pos = to

    def click(self, to, dur=0.35, settle=0.25):
        self.glide(to, dur)
        self.ui("ui.click", {"x": to[0], "y": to[1]})
        self.mark(cursor=list(to), click=True)
        self.wait(settle)

    def tool(self, command, panel=None):
        """Click a toolbar button (opening its panel's drop-down first when it's in one)."""
        if panel:
            self.click(self.at(handle=panel), settle=0.3)
        self.click(self.at(handle=f"toolbar:{command}"), settle=0.35)

    def type(self, text, enter=True):
        self.ui("ui.text", {"text": text})
        self.wait(0.35)
        if enter:
            self.ui("ui.key", {"key": "Enter"})
            self.wait(0.5)

    def key(self, k):
        self.ui("ui.key", {"key": k})
        self.wait(0.15)

    def view(self, v, settle=0.7):
        self.ui("ui.view", {"view": v, "animate": True})
        self.wait(settle)

    def orbit(self, dx, dy=0, steps=40):
        """A right-button drag in the viewport, as a user orbits."""
        vp = self.ui("ui.inspect").get("viewport") or [250, 126, 1350, 800]
        cx, cy = vp[0] + vp[2] * 0.62, vp[1] + vp[3] * 0.55
        self.glide((cx, cy), 0.25)
        self.mark(drag_start=[cx, cy])
        self.ui("ui.drag", {"x0": cx, "y0": cy, "x1": cx + dx, "y1": cy + dy, "button": "right", "steps": steps})
        self.settle_camera()
        # The pointer moved with the camera (the app plays the drag one event a frame).
        self.mark(drag_end=[cx + dx, cy + dy])
        self.pos = (cx + dx, cy + dy)

    def settle_camera(self):
        """Wait until the app has played queued pointer events (the camera stops moving)."""
        last, still = None, 0
        for _ in range(80):
            time.sleep(0.1)
            cam = json.dumps(self.ui("ui.inspect").get("camera"))
            still = still + 1 if cam == last else 0
            last = cam
            if still >= 2:
                return

    def execute(self, command, params):
        self.mcp.tool("execute", {"command": command, "params": params})


def run(d):
    d.caption("Parameters over MCP: width 80, depth 60, height 30, wall 2", "MCP tools/call execute parameters.change")
    for name, expr in [("width", "80 mm"), ("depth", "60 mm"), ("height", "30 mm"), ("wall", "2 mm")]:
        d.execute("parameters.change", {"name": name, "expression": expr})
        d.wait(0.25)
    d.wait(0.6)

    d.caption("Create Sketch on the XY plane", "UI click")
    d.tool("sketch.create")
    d.click(d.at(plane="XY"), settle=0.8)

    d.caption("Center rectangle from the origin", "UI click")
    d.tool("sketch.rectangle.center")
    d.click(d.at(sketch=[0, 0]), settle=0.15)
    d.click(d.at(sketch=[40, 30]), dur=0.45, settle=0.2)
    d.key("Escape")

    d.caption("Constraints over MCP: centred, width × depth", "MCP tools/call execute sketch.dimension")
    sk = json.loads(d.mcp.tool("execute", {"command": "sketch.inspect", "params": {}}))
    sk = sk.get("result", sk)
    centre = next((p["id"] for p in sk.get("points", []) if p["id"] != "origin" and abs(p["at"][0]) + abs(p["at"][1]) < 1e-6), None)
    if centre:
        d.execute("sketch.constraint.coincident", {"a": centre, "b": "origin"})
    d.execute("sketch.dimension", {"entities": ["l1"], "value": "width"})
    d.wait(0.3)
    d.execute("sketch.dimension", {"entities": ["l2"], "value": "depth"})
    d.wait(0.8)

    d.caption("Finish Sketch", "UI click")
    d.tool("sketch.finish")
    # The origin planes are in the way of picking faces from here on.
    d.ui("ui.set", {"showOrigin": False})
    d.view("home", 0.6)

    d.caption("Extrude: height", "UI click")
    d.tool("solid.extrude")
    d.wait(0.3)
    d.type("height")

    d.caption("Fillet the four vertical edges: 6 mm", "UI click")
    d.tool("solid.fillet")
    for p in ([40, -30, 15], [-40, -30, 15], [40, 30, 15]):
        d.click(d.at(world=p), settle=0.15)
    d.orbit(-260, 20)
    d.click(d.at(world=[-40, 30, 15]), settle=0.2)
    d.type("6")
    d.view("home", 0.5)

    d.caption("Shell: open the top face, wall thickness", "UI click")
    d.tool("solid.shell", panel="panel:SOLID:MODIFY")
    d.click(d.at(world=[0, 0, 30]), settle=0.3)
    d.type("wall")

    d.caption("Sketch on the side face for a USB port", "UI click")
    d.tool("sketch.create")
    d.click(d.at(world=[40, 10, 20]), settle=0.9)
    d.tool("sketch.rectangle.center")
    d.click(d.at(world=[40, 0, 12]), settle=0.15)
    d.click(d.at(world=[40, 6, 15.5]), dur=0.4, settle=0.2)
    d.key("Escape")
    d.tool("sketch.finish")
    d.view("home", 0.5)

    # Over MCP with the profile named by a point inside it: a profile picked in the Extrude
    # dialog is kept by its index, which a face sketch renumbers when the face changes shape.
    d.caption("Cut the port through the wall", "MCP tools/call execute solid.extrude")
    d.execute("solid.extrude", {"profiles": [{"point": [0, 12]}], "distance": "-wall", "operation": "cut"})
    d.wait(0.6)
    d.orbit(-140, 30)

    d.caption("Sketch on the inner floor: four standoffs", "UI click")
    d.tool("sketch.create")
    d.click(d.at(world=[6, -9, 2]), settle=0.9)
    d.tool("sketch.circle.center")
    for x, y in [(-30, -20), (30, -20), (30, 20), (-30, 20)]:
        d.click(d.at(world=[x, y, 2]), dur=0.3, settle=0.1)
        d.click(d.at(world=[x + 3.5, y, 2]), dur=0.2, settle=0.15)
    d.key("Escape")
    d.tool("sketch.finish")

    d.view("home", 0.5)

    d.caption("Extrude the standoffs: 12 mm", "MCP tools/call execute solid.extrude")
    d.execute("solid.extrude", {"profiles": [{"point": [x, y]} for x, y in [(-30, -20), (30, -20), (30, 20), (-30, 20)]], "distance": 12, "operation": "join"})
    d.wait(0.4)

    d.caption("Change one parameter: width 80 → 100 mm", "MCP tools/call execute parameters.change")
    d.execute("parameters.change", {"name": "width", "expression": "100 mm"})
    d.wait(1.8)

    d.caption("", "")
    d.orbit(420, 0, steps=90)
    d.view("home", 1.5)


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--port", type=int, default=7878, help="the app's --control port")
    ap.add_argument("--cli", default="solvecraft-cli", help="path to solvecraft-cli")
    ap.add_argument("--log", help="write a JSON line per caption and pointer event")
    ap.add_argument("--until", type=int, default=10**6, help="stop before step N (trying the script out)")
    ap.add_argument("--speed", type=float, default=1.0, help="scale the pauses (2 = twice as long)")
    a = ap.parse_args()
    mcp = Mcp([a.cli, "mcp", "--connect", f"127.0.0.1:{a.port}"])
    mcp.send("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "enclosure-demo-v2", "version": "2"}})
    mcp.send("notifications/initialized", notify=True)
    d = Demo(a.port, mcp, open(a.log, "w") if a.log else None, a.speed)
    d.stop = a.until - 1
    t0 = time.time()
    try:
        run(d)
    except StopIteration:
        pass
    d.mark(caption="", via="")
    mcp.close()
    print(f"done in {time.time() - t0:.1f}s", file=sys.stderr)


if __name__ == "__main__":
    main()
