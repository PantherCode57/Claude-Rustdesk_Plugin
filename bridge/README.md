# The bridge: a RustDesk client with no window

`bin/rustdesk-claude.exe` is built from the official RustDesk source plus the files here.

- `src/claude_bridge.rs`: the client. It keeps the last frame in memory and serves it, with
  mouse and keyboard control, on `http://127.0.0.1:21500`.
- `src/claude_bridge_main.rs`: its `main`.
- `rustdesk.patch`: the lines added to RustDesk's own `Cargo.toml` and `src/lib.rs`.

## Building it (Windows)

Tools: Rust (toolchain 1.75, the one RustDesk's CI pins), Visual Studio Build Tools with the C++
workload, LLVM, and vcpkg at commit `9e593bb18ea69cc5095e012465dcd675a822ed0d`.

```powershell
git clone --recurse-submodules https://github.com/rustdesk/rustdesk
cd rustdesk
git apply <this folder>\rustdesk.patch
copy <this folder>\src\*.rs src\

cd C:\vcpkg
.\vcpkg install libvpx:x64-windows-static libyuv:x64-windows-static opus:x64-windows-static aom:x64-windows-static --overlay-ports=<rustdesk>\res\vcpkg

cd <rustdesk>
$env:VCPKG_ROOT = "C:\vcpkg"
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
cargo +1.75.0 build --release --bin rustdesk-claude
```

The first build takes about 25 minutes. Copy `target\release\rustdesk-claude.exe` to `bin\`.

## HTTP API (127.0.0.1 only; other hosts and other sites' requests are refused)

| | |
|---|---|
| `GET /` | live viewer page |
| `GET /status` | status, peer, screen size, peer names, Claude's pointer |
| `GET /frame.png?max=N` | the screen as PNG, longest side at most N |
| `GET /frame.svg?max=N` | the screen as a small JPEG inside an SVG, for the band above the prompt |
| `POST /connect` `{peer}` | connect to a peer from the peers file |
| `POST /disconnect` | close the session |
| `POST /mouse` `{action,x,y,button,dx,dy}` | move, click, double_click, down, up, scroll |
| `POST /key` `{key}` | a key or combination, e.g. `ctrl+s` |
| `POST /type` `{text}` | type text |

The peers file is `%APPDATA%\RustDeskClaude\peers.json`, or the path in `RUSTDESK_CLAUDE_PEERS`.
