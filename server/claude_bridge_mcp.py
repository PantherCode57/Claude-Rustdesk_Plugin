"""MCP server (stdio) that lets Claude drive a remote PC through the RustDesk Claude bridge.

It only talks to the bridge's local HTTP API (bin/rustdesk-claude.exe). Each server starts its
own bridge on its own port, so every conversation has its own remote session and its own view.
Standard library only.

Design: text first. Actions return a line of text, not a picture; a screenshot is taken only
when the caller asks for one. Known places are clicked by name from a per-PC map, programs are
opened by name, and keyboard shortcuts replace hunting for buttons.
"""
import base64
import json
import os
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# Set only to share one bridge that something else started; otherwise this server starts its own.
BASE = os.environ.get("RUSTDESK_CLAUDE_URL", "")
bridge = None  # the bridge process this server started
# The person's own data (peers with their passwords, the maps Claude learns) lives outside the
# plugin folder, so updating or reinstalling the plugin never touches it.
HOME = os.environ.get("RUSTDESK_CLAUDE_HOME") or (
    r"C:\RustDeskClaude" if os.name == "nt" else os.path.expanduser("~/.rustdesk-claude"))
PEERS = os.path.join(HOME, "peers.json")
MAPS = os.path.join(HOME, "maps")
SHORTCUTS = os.path.join(ROOT, "maps", "_atajos.json")
EXES = [os.environ.get("RUSTDESK_CLAUDE_EXE"), os.path.join(ROOT, "bin", "rustdesk-claude.exe"),
        r"C:\rustdesk-target\release\rustdesk-claude.exe"]
SIZES = {"small": 768, "normal": 1280}  # longest side of a screenshot sent to the model

scale = 1.0  # real pixels per screenshot pixel, from the last screenshot
have_shot = False


def ensure_home():
    os.makedirs(MAPS, exist_ok=True)
    if not os.path.exists(PEERS):
        with open(PEERS, "w", encoding="utf-8") as f:
            f.write("{}\n")


def kill_with_me(proc):
    """Tie the bridge's life to this server's: when this process ends, however it ends, Windows
    closes the job and the bridge with it, so no remote session is left open behind a closed chat."""
    if os.name != "nt":
        return
    import ctypes
    from ctypes import wintypes

    class Basic(ctypes.Structure):
        _fields_ = [("PerProcessUserTimeLimit", ctypes.c_int64), ("PerJobUserTimeLimit", ctypes.c_int64),
                    ("LimitFlags", wintypes.DWORD), ("MinimumWorkingSetSize", ctypes.c_size_t),
                    ("MaximumWorkingSetSize", ctypes.c_size_t), ("ActiveProcessLimit", wintypes.DWORD),
                    ("Affinity", ctypes.c_size_t), ("PriorityClass", wintypes.DWORD),
                    ("SchedulingClass", wintypes.DWORD)]

    class Extended(ctypes.Structure):
        _fields_ = [("Basic", Basic), ("IoInfo", ctypes.c_uint64 * 6), ("ProcessMemoryLimit", ctypes.c_size_t),
                    ("JobMemoryLimit", ctypes.c_size_t), ("PeakProcessMemoryUsed", ctypes.c_size_t),
                    ("PeakJobMemoryUsed", ctypes.c_size_t)]

    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.CreateJobObjectW.restype = wintypes.HANDLE
    k32.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
    k32.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
    k32.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
    job = k32.CreateJobObjectW(None, None)
    info = Extended()
    info.Basic.LimitFlags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    if job and k32.SetInformationJobObject(job, 9, ctypes.byref(info), ctypes.sizeof(info)):
        k32.AssignProcessToJobObject(job, int(proc._handle))
    proc.job = job  # kept open for as long as this server lives


def start_bridge():
    global BASE, bridge
    exe = next((e for e in EXES if e and os.path.exists(e)), None)
    if not exe:
        raise RuntimeError("rustdesk-claude.exe not found (looked in the plugin's bin folder)")
    ensure_home()
    with socket.socket() as s:  # a free port of our own
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
    env = dict(os.environ, RUSTDESK_CLAUDE_PEERS=PEERS)
    flags = 0x08000000 if os.name == "nt" else 0  # no console window
    bridge = subprocess.Popen([exe, str(port)], env=env, creationflags=flags, stdin=subprocess.DEVNULL,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    kill_with_me(bridge)
    BASE = f"http://127.0.0.1:{port}"
    for _ in range(20):
        time.sleep(0.5)
        try:
            _http("GET", "/status")
            return
        except ConnectionError:
            pass
    raise RuntimeError(f"Bridge could not be started from {exe}")


def http(method, path, body=None):
    if not BASE or (bridge is not None and bridge.poll() is not None):
        start_bridge()
    try:
        return _http(method, path, body)
    except ConnectionError:
        raise RuntimeError(f"Bridge not reachable at {BASE}")


def _http(method, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(BASE + path, data=data, method=method)
    if data:
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.read(), r.headers
    except urllib.error.HTTPError as e:
        try:
            msg = json.loads(e.read()).get("error", str(e))
        except ValueError:
            msg = str(e)
        raise RuntimeError(msg)
    except urllib.error.URLError as e:
        raise ConnectionError(str(e.reason))


def text(s):
    return [{"type": "text", "text": s}]


def get_status():
    return json.loads(http("GET", "/status")[0])


# --- Session

def status(_):
    ensure_home()
    s = get_status()
    lines = [f"status: {s['status']}" + (f" ({s['message']})" if s["message"] else ""),
             f"peer: {s['peer'] or '-'}", f"screen: {s['width']}x{s['height']}",
             f"peers available: {', '.join(s['peers']) or 'none'}", f"peers file: {s['peers_file']}",
             f"live view: {BASE}/"]
    return text("\n".join(lines))


def connect(a):
    ensure_home()
    http("POST", "/connect", {"peer": a["peer"]})
    s = {}
    for _ in range(60):  # up to 30 s for the first frame
        s = get_status()
        if s["status"] == "error":
            raise RuntimeError(s["message"])
        if s["status"] == "connected" and s["width"]:
            m = load_map()
            places = sorted(list(m.get("points", {})) + list(m.get("vistos", {})))
            out = [f"Connected to {a['peer']}: {s['width']}x{s['height']}.",
                   f"Live view: {BASE}/",
                   "No screenshot taken. Work by name and keyboard; look only when you must.",
                   "Places known on this PC: " + (", ".join(places) or "none yet")]
            out += ["Tip: " + t for t in m.get("tips", [])]
            return text("\n".join(out))
        time.sleep(0.5)
    raise RuntimeError("Timed out waiting for the remote screen: " + json.dumps(s))


def disconnect(_):
    http("POST", "/disconnect", {})
    return text("Disconnected")


def screenshot(a):
    global scale, have_shot
    png, h = http("GET", f"/frame.png?max={SIZES.get(a.get('size'), SIZES['normal'])}")
    have_shot = True
    w, real_w = int(h["X-Width"]), int(h["X-Real-Width"])
    scale = real_w / w
    return [
        {"type": "image", "data": base64.b64encode(png).decode(), "mimeType": "image/png"},
        {"type": "text", "text": f"Screenshot {w}x{h['X-Height']} (remote screen is {real_w}x{h['X-Real-Height']}). "
                                 "x/y for the next clicks are in this screenshot's pixels."},
    ]


def done(a, said):
    """What an action returns: a line of text, plus a look only if the caller asked for one."""
    if a.get("screenshot") is True:
        time.sleep(0.6)  # let the remote screen react first
        return text(said) + screenshot(a)
    return text(said)


# --- Map: named places on a peer's screen, in REAL remote pixels. One file per peer.

def map_path():
    peer = get_status()["peer"]
    if not peer:
        raise RuntimeError("Not connected")
    return os.path.join(MAPS, peer + ".json")


def load_map():
    try:
        with open(map_path(), encoding="utf-8") as f:
            return json.load(f)
    except FileNotFoundError:
        return {"tips": [], "points": {}, "vistos": {}}


def write_map(m):
    os.makedirs(MAPS, exist_ok=True)
    with open(map_path(), "w", encoding="utf-8") as f:
        json.dump(m, f, ensure_ascii=False, indent=1)


def show_map(_):
    m = load_map()
    try:
        with open(SHORTCUTS, encoding="utf-8") as f:
            m["atajos"] = json.load(f)
    except FileNotFoundError:
        pass
    return text(json.dumps(m, ensure_ascii=False, indent=1))


def save_point(a):
    x, y = shot_xy(a)
    m = load_map()
    m.setdefault("points", {})[a["name"]] = {"x": x, "y": y, "note": a.get("note", "")}
    m.get("vistos", {}).pop(a["name"], None)
    write_map(m)
    return text(f"Saved '{a['name']}'")


def save_tip(a):
    m = load_map()
    if a["tip"] not in m.setdefault("tips", []):
        m["tips"].append(a["tip"])
        write_map(m)
    return text("Tip saved")


def record_seen(label, app, x, y):
    """Every click made by coordinates is written down under "vistos", so next time the same
    thing is clicked by name. Seen twice in the same spot -> "estable": it does not move."""
    m = load_map()
    key = label.strip().lower().replace(" ", "_")
    if key in m.get("points", {}):
        return ""
    seen = m.setdefault("vistos", {})
    old = seen.get(key)
    if old and abs(old["x"] - x) <= 10 and abs(old["y"] - y) <= 10:
        old["veces"] += 1
        old["estable"] = True
        note = f"'{key}' seen again in the same place: marked stable."
    else:
        moved = bool(old)
        seen[key] = {"x": x, "y": y, "app": app or (old or {}).get("app", ""), "veces": 1, "estable": False}
        note = f"'{key}' moved since last time: position updated." if moved else f"'{key}' written to the map."
    write_map(m)
    return note


def confirm_seen(name):
    """A click by name counts as a confirmation of a "vistos" place. This script cannot see
    whether the click hit, so a place that turned out wrong is taken back with map_wrong."""
    m = load_map()
    p = m.get("vistos", {}).get(name)
    if not p:
        return ""
    p["veces"] += 1
    was_stable, p["estable"] = p["estable"], True
    write_map(m)
    return "" if was_stable else f"'{name}' confirmed: marked stable. If the click missed, call rustdesk_map_wrong."


def mark_wrong(a):
    m = load_map()
    if m.get("vistos", {}).pop(a["name"], None) is None and m.get("points", {}).pop(a["name"], None) is None:
        raise RuntimeError(f"'{a['name']}' is not in the map")
    write_map(m)
    return text(f"'{a['name']}' removed from the map; click it by coordinates to record its new place")


def shot_xy(a):
    if a.get("x") is None or a.get("y") is None:
        raise RuntimeError("Give x and y, or a name from the map")
    if not have_shot:
        raise RuntimeError("x/y are screenshot pixels: take a screenshot first, or use a name from the map")
    return round(a["x"] * scale), round(a["y"] * scale)


def real_xy(a):
    """Real remote pixels from either a map name or screenshot coordinates."""
    if a.get("name"):
        m = load_map()
        p = m.get("points", {}).get(a["name"]) or m.get("vistos", {}).get(a["name"])
        if not p:
            raise RuntimeError(f"'{a['name']}' is not in the map")
        return p["x"], p["y"]
    return shot_xy(a)


# --- Input

def send_mouse(action, a, **extra):
    x, y = real_xy(a)
    http("POST", "/mouse", {"action": action, "x": x, "y": y, **extra})
    return x, y


def click(a):
    if a.get("name"):
        note = confirm_seen(a["name"])
    else:
        if not a.get("label"):
            raise RuntimeError("A click by coordinates needs a 'label' (what is clicked) so it gets written to the map")
        note = record_seen(a["label"], a.get("app", ""), *real_xy(a))
    x, y = send_mouse("double_click" if a.get("double") else "click", a, button=a.get("button", "left"))
    return done(a, f"Clicked {a.get('name') or a['label']} at {x},{y}." + (" " + note if note else ""))


def move(a):
    x, y = send_mouse("move", a)
    return done(a, f"Pointer at {x},{y}.")


def drag(a):
    x, y = shot_xy(a)
    tx, ty = shot_xy({"x": a["to_x"], "y": a["to_y"]})
    http("POST", "/mouse", {"action": "down", "x": x, "y": y})
    time.sleep(0.15)
    http("POST", "/mouse", {"action": "move", "x": tx, "y": ty})
    time.sleep(0.15)
    http("POST", "/mouse", {"action": "up", "x": tx, "y": ty})
    return done(a, f"Dragged {x},{y} to {tx},{ty}.")


def scroll(a):
    send_mouse("scroll", a, dy=a.get("dy", 0), dx=a.get("dx", 0))
    return done(a, "Scrolled.")


def do_type(s):
    http("POST", "/type", {"text": s})


def do_key(k):
    http("POST", "/key", {"key": k})


def do_open(program):
    """Win+R, name, Enter."""
    do_key("win+r")
    time.sleep(0.7)
    do_type(program)
    time.sleep(0.2)
    do_key("enter")
    time.sleep(1.2)  # programs take a moment to draw their window


def type_text(a):
    do_type(a["text"])
    return done(a, "Typed.")


def key(a):
    do_key(a["key"])
    return done(a, f"Pressed {a['key']}.")


def open_program(a):
    do_open(a["program"])
    return done(a, f"Opened {a['program']}.")


def steps(a):
    """Several known actions in one call: the fewer round trips, the faster the task."""
    log = []
    for i, s in enumerate(a["steps"], 1):
        try:
            if "open" in s:
                do_open(s["open"])
            elif "key" in s:
                do_key(s["key"])
            elif "type" in s:
                do_type(s["type"])
            elif "click" in s:
                confirm_seen(s["click"])
                send_mouse("double_click" if s.get("double") else "click", {"name": s["click"]},
                           button=s.get("button", "left"))
            elif "wait" in s:
                time.sleep(min(float(s["wait"]), 30))
            else:
                raise RuntimeError("unknown step, use one of: open, key, type, click, wait")
        except Exception as e:
            log.append(f"{i}. FAILED {json.dumps(s, ensure_ascii=False)}: {e}. Stopped here.")
            break
        log.append(f"{i}. ok {json.dumps(s, ensure_ascii=False)}")
        time.sleep(0.3)
    return done(a, "\n".join(log))


# --- Tool table

XY = {"x": {"type": "integer"}, "y": {"type": "integer"}}
SHOT = {"screenshot": {"type": "boolean", "description": "true = also return a screenshot after the action. "
        "Leave it out unless you cannot go on without seeing the screen."}}
NAME = {"name": {"type": "string", "description": "A place from the map, instead of x/y"}}


def tool(name, desc, fn, props=None, required=()):
    schema = {"type": "object", "properties": props or {}, "required": list(required)}
    return name, {"name": name, "description": desc, "inputSchema": schema}, fn


TOOLS = [
    tool("rustdesk_status", "Bridge status, connected peer, the peer names available and where the peers file is.", status),
    tool("rustdesk_connect", "Connect to a remote PC by its name in peers.json. Returns text: the screen size, the "
         "places already known on that PC and its tips. It takes no screenshot.", connect,
         {"peer": {"type": "string"}}, ["peer"]),
    tool("rustdesk_disconnect", "Close the remote session.", disconnect),
    tool("rustdesk_screenshot", "Look at the remote screen. Costly: use it only when strictly necessary, that is, "
         "when you do not know what is on screen and need to, to find something that is not in the map, or to check "
         "a result that matters. 'small' is enough to see which window is in front.", screenshot,
         {"size": {"type": "string", "enum": ["small", "normal"]}}),
    tool("rustdesk_open", "Open a program, folder, settings page or web address by name through Win+R "
         "(e.g. 'notepad', 'calc', 'chrome', 'ms-settings:printers', 'C:\\Users', 'https://...'). The fastest way "
         "to open anything; rustdesk_map lists common names.", open_program, {"program": {"type": "string"}, **SHOT}, ["program"]),
    tool("rustdesk_key", "Press a key or combination, e.g. 'enter', 'ctrl+p', 'win+e', 'alt+f4'. Prefer shortcuts "
         "to hunting for buttons; rustdesk_map lists the useful ones.", key, {"key": {"type": "string"}, **SHOT}, ["key"]),
    tool("rustdesk_type", "Type text into the focused control.", type_text, {"text": {"type": "string"}, **SHOT}, ["text"]),
    tool("rustdesk_steps", "Run several known actions in one call, in order. Each step is one of "
         "{\"open\": \"notepad\"}, {\"key\": \"ctrl+s\"}, {\"type\": \"text\"}, {\"click\": \"<map name>\"}, "
         "{\"wait\": seconds}. Use it whenever you already know the sequence; it stops at the first failure.", steps,
         {"steps": {"type": "array", "items": {"type": "object"}}, **SHOT}, ["steps"]),
    tool("rustdesk_click", "Click a place from the map by name (no screenshot needed), or at coordinates taken from "
         "a screenshot. A click by coordinates must carry a 'label' naming what is clicked (e.g. 'calculadora_igual') "
         "and ideally 'app': it is written to the map automatically, so next time it is clicked by name. A click by "
         "name confirms the place as stable; if it missed, call rustdesk_map_wrong.", click,
         {**XY, **NAME, **SHOT, "label": {"type": "string", "description": "Short name of the thing clicked; required with x/y"},
          "app": {"type": "string", "description": "Program or window it belongs to"},
          "button": {"type": "string", "enum": ["left", "right", "middle"]}, "double": {"type": "boolean"}}),
    tool("rustdesk_move", "Move the pointer (hover) to a map place or screenshot coordinates.", move, {**XY, **NAME, **SHOT}),
    tool("rustdesk_drag", "Drag with the left button from (x,y) to (to_x,to_y), in screenshot coordinates.", drag,
         {**XY, "to_x": {"type": "integer"}, "to_y": {"type": "integer"}, **SHOT}, ["x", "y", "to_x", "to_y"]),
    tool("rustdesk_scroll", "Scroll the wheel at a map place or screenshot coordinates. dy in notches: positive up, "
         "negative down.", scroll, {**XY, **NAME, "dy": {"type": "integer"}, "dx": {"type": "integer"}, **SHOT}),
    tool("rustdesk_map", "Everything known about this PC without looking at it: named places, tips, and the list of "
         "keyboard shortcuts and program names.", show_map),
    tool("rustdesk_map_save", "Name a place that stays put (a maximized app's button, a fixed toolbar), at screenshot "
         "coordinates.", save_point, {**XY, "name": {"type": "string"}, "note": {"type": "string"}}, ["name", "x", "y"]),
    tool("rustdesk_map_tip", "Write down something learned about this PC that saves time next visit (a shortcut that "
         "works in its programs, a quirk, where a file lives).", save_tip, {"tip": {"type": "string"}}, ["tip"]),
    tool("rustdesk_map_wrong", "A place clicked by name was not where the map said: remove it, so the next click by "
         "coordinates records where it is now.", mark_wrong, {"name": {"type": "string"}}, ["name"]),
]
BY_NAME = {name: fn for name, _, fn in TOOLS}


def handle(msg):
    method, params = msg.get("method"), msg.get("params") or {}
    if method == "initialize":
        return {
            "protocolVersion": params.get("protocolVersion", "2024-11-05"),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "rustdesk-claude", "version": "0.2.0"},
            "instructions": "Controls another computer through RustDesk without using the local screen, mouse or "
                            "keyboard. Text first: connect, then work with rustdesk_open, rustdesk_key, rustdesk_type, "
                            "rustdesk_steps and clicks by map name. Take a screenshot only when strictly necessary. "
                            "Never ask for a RustDesk password in chat: it goes in the peers file.",
        }
    if method == "tools/list":
        return {"tools": [spec for _, spec, _ in TOOLS]}
    if method == "tools/call":
        try:
            return {"content": BY_NAME[params["name"]](params.get("arguments") or {})}
        except Exception as e:  # tool errors go back to the model, not the transport
            return {"content": text(f"Error: {e}"), "isError": True}
    if method == "ping":
        return {}
    raise LookupError(method)


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        msg = json.loads(line)
        if "id" not in msg:  # notification
            continue
        try:
            out = {"jsonrpc": "2.0", "id": msg["id"], "result": handle(msg)}
        except LookupError:
            out = {"jsonrpc": "2.0", "id": msg["id"], "error": {"code": -32601, "message": "Method not found"}}
        sys.stdout.write(json.dumps(out) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    try:
        main()
    finally:
        if bridge is not None:
            bridge.terminate()
