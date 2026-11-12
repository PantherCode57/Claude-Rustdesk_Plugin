//! Claude bridge: a RustDesk client with no window.
//!
//! It connects to a peer like the normal client, but instead of drawing the remote screen in a
//! window it keeps the last frame in memory and serves it, plus mouse/keyboard control, over a
//! small HTTP API on 127.0.0.1. An MCP server (tools/claude_bridge_mcp.py) talks to that API, and
//! `GET /` is a live viewer page for the human.
//!
//! Nothing here touches the local mouse, keyboard, clipboard or speakers, so the local user can
//! keep working while the session runs.

use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{atomic::AtomicUsize, Arc, Mutex, RwLock},
    time::Duration,
};

use base::message_proto::*;
use hbb_common::{log, rendezvous_proto::ConnType};
use serde_json::{json, Value};

use crate::{
    client::{send_mouse, Key, KEY_MAP},
    common::input::*,
    ui_session_interface::{InvokeUiSession, Session},
};

pub const DEFAULT_PORT: u16 = 21500;

#[derive(Default)]
struct State {
    /// Last decoded frame, B,G,R,A byte order, tightly packed.
    frame: Vec<u8>,
    w: usize,
    h: usize,
    seq: u64,
    /// Top-left of the current remote display in the peer's virtual desktop.
    origin: (i32, i32),
    /// idle | connecting | connected | closed | error
    status: String,
    message: String,
    peer: String,
    /// Where Claude last put the pointer (display pixels) and how many times it pressed a
    /// button. The viewer draws Claude's own pointer from these.
    cursor: Option<(i32, i32)>,
    clicks: u64,
}

lazy_static::lazy_static! {
    static ref STATE: Mutex<State> = Mutex::new(State { status: "idle".into(), ..Default::default() });
    static ref SESSION: Mutex<Option<Session<BridgeHandler>>> = Default::default();
}

fn set_status(status: &str, message: &str) {
    let mut s = STATE.lock().unwrap();
    s.status = status.to_owned();
    s.message = message.to_owned();
}

#[derive(Clone, Default)]
pub struct BridgeHandler;

impl InvokeUiSession for BridgeHandler {
    fn on_rgba(&self, _display: usize, rgba: &mut scrap::ImageRgb) {
        // The decoder reuses this buffer, so copy it out.
        let mut s = STATE.lock().unwrap();
        s.frame.clear();
        s.frame.extend_from_slice(&rgba.raw);
        s.w = rgba.w;
        s.h = rgba.h;
        s.seq += 1;
        if s.status == "connecting" {
            s.status = "connected".into();
            s.message.clear();
        }
    }

    fn msgbox(&self, msgtype: &str, title: &str, text: &str, _link: &str, _retry: bool) {
        log::info!("msgbox {msgtype}: {title}: {text}");
        match msgtype {
            "input-password" => set_status("error", "No password for this peer in peers.json"),
            "re-input-password" => set_status("error", "Wrong password in peers.json"),
            "input-2fa" => set_status("error", "Peer asks for a 2FA code (not supported yet)"),
            t if t.contains("error") => set_status("error", &format!("{title}: {text}")),
            _ => {
                let mut s = STATE.lock().unwrap();
                if s.status != "connected" {
                    s.status = "connecting".into();
                    s.message = format!("{title}: {text}");
                }
            }
        }
    }

    fn set_display(&self, x: i32, y: i32, _w: i32, _h: i32, _cursor_embedded: bool, _scale: f64) {
        STATE.lock().unwrap().origin = (x, y);
    }

    fn set_peer_info(&self, pi: &PeerInfo) {
        log::info!("peer: {}@{} ({})", pi.username, pi.hostname, pi.platform);
    }

    fn on_connected(&self, _conn_type: ConnType) {
        // Deliberately no keyboard grab loop here: that would hook the local keyboard.
        set_status("connected", "");
    }

    fn close_success(&self) {
        // "Close the 'connecting...' box": the first frame arrived (or a file transfer is ready).
        set_status("connected", "");
    }

    fn get_rgba(&self, _display: usize) -> *const u8 {
        std::ptr::null()
    }

    fn set_cursor_data(&self, _cd: CursorData) {}
    fn set_cursor_id(&self, _id: String) {}
    fn set_cursor_position(&self, _cp: CursorPosition) {}
    fn switch_display(&self, _display: &SwitchDisplay) {}
    fn set_displays(&self, _displays: &Vec<DisplayInfo>) {}
    fn set_platform_additions(&self, _data: &str) {}
    fn update_privacy_mode(&self) {}
    fn set_permission(&self, _name: &str, _value: bool) {}
    fn update_quality_status(&self, _qs: crate::client::QualityStatus) {}
    fn set_connection_type(&self, _is_secured: bool, _direct: bool, _stream_type: &str) {}
    fn set_fingerprint(&self, _fingerprint: String) {}
    fn job_error(&self, _id: i32, _err: String, _file_num: i32) {}
    fn job_done(&self, _id: i32, _file_num: i32) {}
    fn clear_all_jobs(&self) {}
    fn new_message(&self, _msg: String) {}
    fn update_transfer_list(&self) {}
    fn load_last_job(&self, _cnt: i32, _job_json: &str, _auto_start: bool) {}
    fn update_folder_files(
        &self,
        _id: i32,
        _entries: &Vec<FileEntry>,
        _path: String,
        _is_local: bool,
        _only_count: bool,
    ) {
    }
    fn confirm_delete_files(&self, _id: i32, _i: i32, _name: String) {}
    fn override_file_confirm(
        &self,
        _id: i32,
        _file_num: i32,
        _to: String,
        _is_upload: bool,
        _is_identical: bool,
    ) {
    }
    fn update_block_input_state(&self, _on: bool) {}
    fn job_progress(&self, _id: i32, _file_num: i32, _speed: f64, _finished_size: f64) {}
    fn adapt_size(&self) {}
    fn cancel_msgbox(&self, _tag: &str) {}
    fn switch_back(&self, _id: &str) {}
    fn portable_service_running(&self, _running: bool) {}
    fn on_voice_call_started(&self) {}
    fn on_voice_call_closed(&self, _reason: &str) {}
    fn on_voice_call_waiting(&self) {}
    fn on_voice_call_incoming(&self) {}
    fn next_rgba(&self, _display: usize) {}
    fn set_multiple_windows_session(&self, _sessions: Vec<WindowsSession>) {}
    fn set_current_display(&self, _disp_idx: i32) {}
    fn update_record_status(&self, _start: bool) {}
    fn printer_request(&self, _id: i32, _path: String) {}
    fn handle_screenshot_resp(&self, _sid: String, _msg: String) {}
    fn handle_terminal_response(&self, _response: TerminalResponse) {}
}

// ---------------------------------------------------------------------------------------------
// Peers file: the password lives here so it never goes through the chat.
// { "oficina": { "id": "123456789", "password": "..." } }

fn peers_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("RUSTDESK_CLAUDE_PEERS") {
        return p.into();
    }
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    std::path::Path::new(&base)
        .join("RustDeskClaude")
        .join("peers.json")
}

fn load_peers() -> Result<HashMap<String, (String, String)>, String> {
    let path = peers_path();
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let v: Value =
        serde_json::from_str(&text).map_err(|e| format!("Bad JSON in {}: {e}", path.display()))?;
    let mut out = HashMap::new();
    for (name, p) in v.as_object().ok_or("peers.json must be an object")? {
        let id: String = p["id"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let password = p["password"].as_str().unwrap_or_default().to_owned();
        out.insert(name.clone(), (id, password));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Session control

fn disconnect() {
    if let Some(s) = SESSION.lock().unwrap().take() {
        s.close();
    }
    let mut st = STATE.lock().unwrap();
    *st = State {
        status: "idle".into(),
        ..Default::default()
    };
}

fn connect(peer: &str) -> Result<(), String> {
    let peers = load_peers()?;
    let (id, password) = peers
        .get(peer)
        .cloned()
        .ok_or_else(|| format!("Unknown peer '{peer}' (not in peers.json)"))?;
    if id.is_empty() {
        return Err(format!("Peer '{peer}' has no id in peers.json"));
    }
    disconnect();

    let session: Session<BridgeHandler> = Session {
        password,
        args: vec![],
        server_keyboard_enabled: Arc::new(RwLock::new(true)),
        server_file_transfer_enabled: Arc::new(RwLock::new(true)),
        server_clipboard_enabled: Arc::new(RwLock::new(true)),
        reconnect_count: Arc::new(AtomicUsize::new(0)),
        ..Default::default()
    };
    {
        let mut lc = session.lc.write().unwrap();
        lc.initialize(id, ConnType::DEFAULT_CONN, None, false, None, None, None, "");
        // Keep the remote's sound and clipboard away from the local user.
        for opt in ["disable-audio", "disable-clipboard"] {
            if !lc.get_toggle_option(opt) {
                lc.toggle_option(opt.to_owned());
            }
        }
    }
    {
        let mut st = STATE.lock().unwrap();
        st.status = "connecting".into();
        st.message.clear();
        st.peer = peer.to_owned();
    }
    session.reconnect(false);
    *SESSION.lock().unwrap() = Some(session);
    Ok(())
}

fn session() -> Result<Session<BridgeHandler>, String> {
    SESSION
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "Not connected".to_owned())
}

// ---------------------------------------------------------------------------------------------
// Input. These build the protocol messages directly instead of going through
// Session::send_mouse / input_key, because those mix in the state of the LOCAL modifier keys.

fn mouse(s: &Session<BridgeHandler>, mask: i32, x: i32, y: i32) {
    send_mouse(mask, x, y, false, false, false, false, s);
}

fn handle_mouse(body: &Value) -> Result<(), String> {
    let s = session()?;
    let (ox, oy) = STATE.lock().unwrap().origin;
    let action = body["action"].as_str().unwrap_or("click");
    let x = body["x"].as_i64().unwrap_or(0) as i32 + ox;
    let y = body["y"].as_i64().unwrap_or(0) as i32 + oy;
    let button = match body["button"].as_str().unwrap_or("left") {
        "right" => MOUSE_BUTTON_RIGHT,
        "middle" => MOUSE_BUTTON_WHEEL,
        _ => MOUSE_BUTTON_LEFT,
    };
    let down = (button << 3) | MOUSE_TYPE_DOWN;
    let up = (button << 3) | MOUSE_TYPE_UP;
    let pause = || std::thread::sleep(Duration::from_millis(40));
    {
        let mut st = STATE.lock().unwrap();
        if body.get("x").is_some() {
            st.cursor = Some((x - ox, y - oy));
        }
        if matches!(action, "click" | "double_click" | "down") {
            st.clicks += 1;
        }
    }

    if action != "up" || body.get("x").is_some() {
        mouse(&s, MOUSE_TYPE_MOVE, x, y);
        pause();
    }
    match action {
        "move" => {}
        "down" => mouse(&s, down, x, y),
        "up" => mouse(&s, up, x, y),
        "click" | "double_click" => {
            let n = if action == "click" { 1 } else { 2 };
            for _ in 0..n {
                mouse(&s, down, x, y);
                pause();
                mouse(&s, up, x, y);
                pause();
            }
        }
        "scroll" => {
            // Notches. Positive dy scrolls up, negative scrolls down.
            let dx = body["dx"].as_i64().unwrap_or(0) as i32;
            let dy = body["dy"].as_i64().unwrap_or(0) as i32;
            mouse(&s, MOUSE_TYPE_WHEEL, dx, dy);
        }
        other => return Err(format!("Unknown mouse action '{other}'")),
    }
    Ok(())
}

/// "enter", "a", "f5", "ctrl" ... -> name in client::KEY_MAP
fn key_map_name(k: &str) -> Option<String> {
    let k = k.to_lowercase();
    let name = match k.as_str() {
        "enter" | "return" => "VK_ENTER",
        "tab" => "VK_TAB",
        "esc" | "escape" => "VK_ESCAPE",
        "backspace" => "VK_BACK",
        "delete" | "del" => "VK_DELETE",
        "insert" | "ins" => "VK_INSERT",
        "home" => "VK_HOME",
        "end" => "VK_END",
        "pageup" | "pgup" => "VK_PRIOR",
        "pagedown" | "pgdn" => "VK_NEXT",
        "left" => "VK_LEFT",
        "right" => "VK_RIGHT",
        "up" => "VK_UP",
        "down" => "VK_DOWN",
        "space" => "VK_SPACE",
        "ctrl" | "control" => "VK_CONTROL",
        "alt" => "VK_MENU",
        "shift" => "VK_SHIFT",
        "win" | "meta" | "cmd" | "super" => "Meta",
        "capslock" => "VK_CAPITAL",
        "printscreen" | "prtsc" => "VK_SNAPSHOT",
        "apps" | "menu" => "Apps",
        "," => "VK_COMMA",
        "/" => "VK_SLASH",
        ";" => "VK_SEMICOLON",
        "'" => "VK_QUOTE",
        "[" => "VK_LBRACKET",
        "]" => "VK_RBRACKET",
        "\\" => "VK_BACKSLASH",
        "-" => "VK_MINUS",
        "=" => "VK_PLUS",
        _ => {
            let up = k.to_uppercase();
            let is_fn = up.starts_with('F') && up[1..].parse::<u8>().map_or(false, |n| n <= 12);
            let is_alnum = up.len() == 1 && up.chars().all(|c| c.is_ascii_alphanumeric());
            return (is_fn || is_alnum).then(|| format!("VK_{up}"));
        }
    };
    Some(name.to_owned())
}

/// Presses a combination such as "ctrl+shift+esc", "win+r", "enter" or "a".
fn handle_key(combo: &str) -> Result<(), String> {
    let s = session()?;
    let parts: Vec<&str> = combo.split('+').map(|p| p.trim()).filter(|p| !p.is_empty()).collect();
    let (key, mods) = parts.split_last().ok_or("Empty key")?;
    let mut ev = KeyEvent::new();
    let name = key_map_name(key).ok_or_else(|| format!("Unknown key '{key}'"))?;
    match KEY_MAP.get(name.as_str()) {
        Some(Key::Chr(c)) | Some(Key::_Raw(c)) => ev.set_chr(*c),
        Some(Key::ControlKey(k)) => ev.set_control_key(k.clone()),
        None => return Err(format!("Unknown key '{key}'")),
    }
    for m in mods {
        let ck = match m.to_lowercase().as_str() {
            "ctrl" | "control" => ControlKey::Control,
            "alt" => ControlKey::Alt,
            "shift" => ControlKey::Shift,
            "win" | "meta" | "cmd" | "super" => ControlKey::Meta,
            other => return Err(format!("Unknown modifier '{other}'")),
        };
        ev.modifiers.push(ck.into());
    }
    ev.press = true;
    ev.mode = KeyboardMode::Legacy.into();
    s.send_key_event(&ev);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Frame -> PNG

/// The last frame as tightly packed RGBA, shrunk so its longest side is at most `max` pixels.
/// Returns (rgba, sent width, sent height, real width, real height).
fn frame_rgba(max: usize) -> Option<(Vec<u8>, usize, usize, usize, usize)> {
    let (src, w, h) = {
        let s = STATE.lock().unwrap();
        if s.w == 0 || s.h == 0 || s.frame.len() < s.w * s.h * 4 {
            return None;
        }
        (s.frame.clone(), s.w, s.h)
    };
    let stride = src.len() / h;
    let long = w.max(h);
    let (tw, th) = if max == 0 || long <= max {
        (w, h)
    } else {
        ((w * max / long).max(1), (h * max / long).max(1))
    };
    let mut out = vec![255u8; tw * th * 4];
    for ty in 0..th {
        let (y0, y1) = (ty * h / th, ((ty + 1) * h / th).max(ty * h / th + 1));
        for tx in 0..tw {
            let (x0, x1) = (tx * w / tw, ((tx + 1) * w / tw).max(tx * w / tw + 1));
            // Box average: keeps small text readable when shrinking.
            let (mut b, mut g, mut r, mut n) = (0u32, 0u32, 0u32, 0u32);
            for y in y0..y1 {
                let row = y * stride;
                for x in x0..x1 {
                    let i = row + x * 4;
                    b += src[i] as u32;
                    g += src[i + 1] as u32;
                    r += src[i + 2] as u32;
                    n += 1;
                }
            }
            let o = (ty * tw + tx) * 4;
            out[o] = (r / n) as u8;
            out[o + 1] = (g / n) as u8;
            out[o + 2] = (b / n) as u8;
        }
    }
    Some((out, tw, th, w, h))
}

fn frame_png(max: usize) -> Option<(Vec<u8>, usize, usize, usize, usize)> {
    let (rgba, tw, th, w, h) = frame_rgba(max)?;
    let mut png = Vec::new();
    repng::encode(&mut png, tw as u32, th as u32, &rgba).ok()?;
    Some((png, tw, th, w, h))
}

/// The Claude app's band above the prompt can only draw an SVG of at most 131072 characters, so
/// the frame goes in as a JPEG small enough to fit, with Claude's pointer drawn over it.
/// Empty while there is no frame, which the band takes as "nothing to show".
fn frame_svg(max: usize) -> String {
    const MAX_BASE64: usize = 120_000;
    let Some((rgba, tw, th, w, _)) = frame_rgba(max) else {
        return String::new();
    };
    let mut b64 = String::new();
    for quality in [60, 45, 30, 18] {
        let mut jpeg = Vec::new();
        let encoder = jpeg_encoder::Encoder::new(&mut jpeg, quality);
        if encoder
            .encode(&rgba, tw as u16, th as u16, jpeg_encoder::ColorType::Rgba)
            .is_err()
        {
            return String::new();
        }
        b64 = crate::common::encode64(&jpeg);
        if b64.len() <= MAX_BASE64 {
            break;
        }
    }
    if b64.len() > MAX_BASE64 {
        return String::new();
    }
    let cursor = STATE.lock().unwrap().cursor.map_or(String::new(), |(x, y)| {
        let k = tw as f64 / w as f64;
        format!(
            "<g transform='translate({:.1} {:.1})'><path d='M0 0 L0 15 L4.2 11.2 L7 18 L10 16.7 L7.2 10 L13 10 Z' fill='#D97757' stroke='#fff' stroke-width='1.3' stroke-linejoin='round'/><rect x='13' y='15' width='46' height='15' rx='7.5' fill='#D97757'/><text x='36' y='26' font-family='sans-serif' font-size='10' fill='#fff' text-anchor='middle'>Claude</text></g>",
            x as f64 * k,
            y as f64 * k
        )
    });
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{tw}' height='{th}' viewBox='0 0 {tw} {th}'><image href='data:image/jpeg;base64,{b64}' width='{tw}' height='{th}'/>{cursor}</svg>"
    )
}

// ---------------------------------------------------------------------------------------------
// HTTP

const VIEWER_HTML: &str = r##"<!doctype html><html><head><meta charset="utf-8">
<title>RustDesk · Claude</title>
<style>
 html,body{margin:0;height:100%;background:#111;color:#ddd;font:13px system-ui,sans-serif}
 #bar{padding:6px 10px;background:#1c1c1c;border-bottom:1px solid #333}
 #wrap{height:calc(100% - 30px);display:flex;align-items:center;justify-content:center}
 #box{position:relative;display:none;line-height:0}
 img{max-width:100vw;max-height:calc(100vh - 30px);display:block}
 #cur{position:absolute;left:0;top:0;display:none;pointer-events:none;transition:transform .25s ease-out;line-height:1}
 #cur span{position:absolute;left:18px;top:20px;background:#D97757;color:#fff;font:500 11px system-ui,sans-serif;padding:2px 7px;border-radius:9px;white-space:nowrap}
 #rip{position:absolute;left:-14px;top:-14px;width:28px;height:28px;border-radius:50%;border:2px solid #D97757;opacity:0}
 #rip.go{animation:rip .5s ease-out}
 @keyframes rip{from{transform:scale(.3);opacity:.9}to{transform:scale(1.6);opacity:0}}
</style></head><body>
<div id="bar">…</div><div id="wrap"><div id="box"><img id="v">
<div id="cur"><div id="rip"></div>
<svg width="22" height="26" viewBox="0 0 22 26"><path d="M2 2 L2 20 L7 15.5 L10.5 23.5 L14 22 L10.5 14 L17.5 14 Z" fill="#D97757" stroke="#fff" stroke-width="1.6" stroke-linejoin="round"/></svg>
<span>Claude</span></div></div></div>
<script>
const v=document.getElementById('v'),bar=document.getElementById('bar'),box=document.getElementById('box'),
 cur=document.getElementById('cur'),rip=document.getElementById('rip');let seq=-1,clicks=-1;
async function tick(){
  try{
    const s=await (await fetch('/status')).json();
    bar.textContent=(s.peer?s.peer+' — ':'')+s.status+(s.message?' — '+s.message:'')+(s.width?'  ('+s.width+'×'+s.height+')':'');
    if(s.seq!==seq&&s.width){seq=s.seq;
      await new Promise(r=>{v.onload=v.onerror=r;v.src='/frame.png?max=1920&t='+seq});
      box.style.display='block';}
    if(s.cursor&&s.width){const k=v.clientWidth/s.width;
      cur.style.display='block';cur.style.transform='translate('+(s.cursor.x*k-2)+'px,'+(s.cursor.y*k-2)+'px)';
      if(clicks>=0&&s.clicks!==clicks){rip.classList.remove('go');void rip.offsetWidth;rip.classList.add('go')}
      clicks=s.clicks;
    }else cur.style.display='none';
  }catch(e){bar.textContent='bridge not reachable'}
  setTimeout(tick,200);
}
tick();
</script></body></html>"##;

fn respond(stream: &mut TcpStream, code: &str, ctype: &str, extra: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {code}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n{extra}Connection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

fn respond_json(stream: &mut TcpStream, code: &str, v: Value) {
    respond(stream, code, "application/json", "", v.to_string().as_bytes());
}

fn handle_conn(mut stream: TcpStream, port: u16) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let head_end = loop {
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
        match stream.read(&mut tmp) {
            Ok(n) if n > 0 && buf.len() < 1 << 20 => buf.extend_from_slice(&tmp[..n]),
            _ => return,
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let method = first.next().unwrap_or_default().to_owned();
    let target = first.next().unwrap_or_default().to_owned();
    let headers: HashMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_owned()))
        .collect();

    // This API controls another computer, so only this machine's own pages and tools may use it:
    // refuse other hostnames (DNS rebinding) and requests made by other websites (Origin).
    let host_ok = headers.get("host").map_or(false, |h| {
        h == &format!("127.0.0.1:{port}") || h == &format!("localhost:{port}")
    });
    let origin_ok = headers.get("origin").map_or(true, |o| {
        o == &format!("http://127.0.0.1:{port}") || o == &format!("http://localhost:{port}")
    });
    if !host_ok || !origin_ok {
        return respond_json(&mut stream, "403 Forbidden", json!({"error": "forbidden"}));
    }

    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end..].to_vec();
    while body.len() < len.min(1 << 20) {
        match stream.read(&mut tmp) {
            Ok(n) if n > 0 => body.extend_from_slice(&tmp[..n]),
            _ => break,
        }
    }
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));

    let result: Result<Value, String> = match (method.as_str(), path) {
        ("GET", "/") => {
            return respond(&mut stream, "200 OK", "text/html; charset=utf-8", "", VIEWER_HTML.as_bytes())
        }
        ("GET", "/status") => {
            let s = STATE.lock().unwrap();
            let peers: Vec<String> = load_peers().map(|p| p.into_keys().collect()).unwrap_or_default();
            Ok(json!({
                "status": s.status, "message": s.message, "peer": s.peer,
                "width": s.w, "height": s.h, "seq": s.seq,
                "cursor": s.cursor.map(|(x, y)| json!({"x": x, "y": y})), "clicks": s.clicks,
                "peers": peers, "peers_file": peers_path().display().to_string(),
            }))
        }
        ("GET", "/frame.png") => {
            let max = query
                .split('&')
                .find_map(|kv| kv.strip_prefix("max="))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            return match frame_png(max) {
                Some((png, tw, th, w, h)) => {
                    let extra = format!("X-Width: {tw}\r\nX-Height: {th}\r\nX-Real-Width: {w}\r\nX-Real-Height: {h}\r\n");
                    respond(&mut stream, "200 OK", "image/png", &extra, &png)
                }
                None => respond_json(&mut stream, "404 Not Found", json!({"error": "No frame yet"})),
            };
        }
        ("GET", "/frame.svg") => {
            let max = query
                .split('&')
                .find_map(|kv| kv.strip_prefix("max="))
                .and_then(|v| v.parse().ok())
                .unwrap_or(640);
            return respond(&mut stream, "200 OK", "text/plain; charset=utf-8", "", frame_svg(max).as_bytes());
        }
        ("POST", "/connect") => connect(body["peer"].as_str().unwrap_or_default()).map(|_| json!({})),
        ("POST", "/disconnect") => {
            disconnect();
            Ok(json!({}))
        }
        ("POST", "/mouse") => handle_mouse(&body).map(|_| json!({})),
        ("POST", "/key") => handle_key(body["key"].as_str().unwrap_or_default()).map(|_| json!({})),
        ("POST", "/type") => session().map(|s| {
            s.input_string(body["text"].as_str().unwrap_or_default());
            json!({})
        }),
        _ => return respond_json(&mut stream, "404 Not Found", json!({"error": "not found"})),
    };
    match result {
        Ok(v) => respond_json(&mut stream, "200 OK", v),
        Err(e) => respond_json(&mut stream, "400 Bad Request", json!({"error": e})),
    }
}

/// Runs the bridge until the process is killed. `port` 0 means [`DEFAULT_PORT`].
pub fn run(port: u16) {
    let port = if port == 0 { DEFAULT_PORT } else { port };
    // Own config folder (%APPDATA%\RustDeskClaude), so an installed RustDesk is left untouched.
    *hbb_common::config::APP_NAME.write().unwrap() = "RustDeskClaude".to_owned();
    hbb_common::init_log(false, "claude-bridge");
    crate::common::global_init();

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Cannot listen on 127.0.0.1:{port}: {e}");
            std::process::exit(1);
        }
    };
    println!("Claude bridge listening on http://127.0.0.1:{port}");
    println!("Peers file: {}", peers_path().display());
    for stream in listener.incoming().flatten() {
        std::thread::spawn(move || handle_conn(stream, port));
    }
}
