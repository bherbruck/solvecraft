#!/usr/bin/env python3
"""Build an electronics enclosure in a running SolveCraft over MCP, one command at a time.

    solvecraft --control 7878 &
    python3 examples/demo/enclosure_mcp.py --port 7878

The script is a minimal MCP client: it starts `solvecraft-cli mcp --connect 127.0.0.1:PORT`
(stdio, JSON-RPC 2.0, one message per line), initializes, then sends one `execute` tool call
per step of enclosure_steps.json, waiting `pause` seconds after each so the timeline can be
watched growing. Camera moves (fit, home view, the closing orbit) go over the app's control
channel, since they change the view, not the design. With --log it writes one JSON line per
step (time, caption) for captions.
"""

import argparse
import json
import os
import socket
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))


class Mcp:
    """A stdio MCP client for `solvecraft-cli mcp`."""

    def __init__(self, argv):
        self.p = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
        self.next_id = 0

    def send(self, method, params=None, notify=False):
        msg = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        if not notify:
            self.next_id += 1
            msg["id"] = self.next_id
        self.p.stdin.write(json.dumps(msg) + "\n")
        self.p.stdin.flush()
        if notify:
            return None
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError("the MCP server exited")
            r = json.loads(line)
            if r.get("id") == self.next_id:
                if "error" in r:
                    raise RuntimeError(r["error"])
                return r["result"]

    def tool(self, name, arguments):
        r = self.send("tools/call", {"name": name, "arguments": arguments})
        text = "".join(c.get("text", "") for c in r.get("content", []) if c.get("type") == "text")
        if r.get("isError"):
            raise RuntimeError(f"{name}: {text}")
        return text

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=10)


def control(port, method, params=None):
    """One request on the app's control channel (camera moves)."""
    with socket.create_connection(("127.0.0.1", port)) as s:
        s.sendall((json.dumps({"id": 1, "method": method, "params": params or {}}) + "\n").encode())
        buf = b""
        while not buf.endswith(b"\n"):
            chunk = s.recv(1 << 16)
            if not chunk:
                break
            buf += chunk
    return json.loads(buf or b"{}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--port", type=int, default=7878, help="the app's --control port")
    ap.add_argument("--cli", default="solvecraft-cli", help="path to solvecraft-cli")
    ap.add_argument("--steps", default=os.path.join(HERE, "enclosure_steps.json"))
    ap.add_argument("--pause", type=float, default=2.5, help="seconds after each step (unless the step says)")
    ap.add_argument("--log", help="write a JSON line per step: {t, caption, command}")
    ap.add_argument("--no-orbit", action="store_true", help="skip the closing orbit")
    a = ap.parse_args()

    steps = json.load(open(a.steps))["steps"]
    log = open(a.log, "w") if a.log else None
    t0 = time.time()

    def mark(caption, command=""):
        if log:
            log.write(json.dumps({"t": time.time(), "caption": caption, "command": command}) + "\n")
            log.flush()

    mcp = Mcp([a.cli, "mcp", "--connect", f"127.0.0.1:{a.port}"])
    init = mcp.send("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "enclosure-demo", "version": "1"}})
    mcp.send("notifications/initialized", notify=True)
    print(f"connected to {init.get('serverInfo', {}).get('name')}", file=sys.stderr)

    for i, st in enumerate(steps):
        if st.get("caption"):
            mark(st["caption"], st["command"])
        print(f"[{time.time() - t0:5.1f}s] {i + 1:2}/{len(steps)} {st['command']} {json.dumps(st.get('params', {}))}", file=sys.stderr)
        mcp.tool("execute", {"command": st["command"], "params": st.get("params", {})})
        if st.get("view"):
            control(a.port, "ui.view", {"view": st["view"], "animate": True})
        time.sleep(st.get("pause", a.pause))

    if not a.no_orbit:
        # The home view, a slow orbit (a right drag across the viewport), back home to end on.
        mark("")
        control(a.port, "ui.view", {"view": "home", "animate": True})
        time.sleep(1.5)
        vp = control(a.port, "ui.inspect").get("result", {}).get("viewport") or [250, 126, 1350, 800]
        cx, cy = vp[0] + vp[2] / 2, vp[1] + vp[3] / 2
        mark("Orbit")
        control(a.port, "ui.drag", {"x0": cx - 350, "y0": cy, "x1": cx + 350, "y1": cy + 30, "button": "right", "steps": 240})
        # The app plays the drag one event per frame: wait until the camera stops turning.
        last, still = None, 0
        for _ in range(120):
            time.sleep(0.5)
            yaw = control(a.port, "ui.inspect").get("result", {}).get("camera", {}).get("yaw")
            still = still + 1 if yaw == last else 0
            last = yaw
            if still >= 2:
                break
        # And back to the iso view to end on.
        control(a.port, "ui.view", {"view": "home", "animate": True})
        time.sleep(3)
    mark("")
    mcp.close()
    print(f"done in {time.time() - t0:.1f}s", file=sys.stderr)


if __name__ == "__main__":
    main()
