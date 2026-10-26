//! `ctx mcp-serve computer-use`: the built-in Computer Use MCP server. It
//! speaks MCP over stdio like any other server and lets the model see the
//! screen and drive the mouse and keyboard of this Mac through Core
//! Graphics events. Screenshots come from `screencapture`, scaled to
//! points so the coordinates the model reads off an image are the ones it
//! clicks with.
//!
//! macOS attributes the Screen Recording and Accessibility permissions to
//! the responsible process (Context.app, or the terminal running `ctx`);
//! the server checks them before acting and asks the system to prompt.

use ctx_core::Result;
use serde_json::{json, Value};
use std::io::Write;
use tokio::io::{AsyncBufReadExt, BufReader};

const SERVER_NAME: &str = "context-computer-use";
const SUPPORTED: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// The macOS permissions Computer Use needs, as this process sees them
/// (`ctx serve` is the parent of the server, so what it sees is what the
/// server gets): `{"screen_recording": bool, "accessibility": bool}`.
pub fn permissions() -> Value {
    let (screen, input) = platform::permissions();
    json!({"screen_recording": screen, "accessibility": input, "platform": std::env::consts::OS})
}

/// Runs the named built-in server until stdin closes.
pub async fn serve(name: &str) -> Result<i32> {
    match name {
        "computer-use" => run_computer_use().await,
        other => Err(ctx_core::CtxError::Config(format!("unknown built-in MCP server `{other}` (known: computer-use)"))),
    }
}

async fn run_computer_use() -> Result<i32> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                write_message(&json!({"jsonrpc": "2.0", "id": Value::Null, "error": {"code": -32700, "message": format!("parse error: {e}")}}));
                continue;
            }
        };
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(|m| m.as_str()).unwrap_or_default().to_string();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        if id.is_none() || id == Some(Value::Null) {
            // Notifications need no answer.
            continue;
        }
        let id = id.unwrap_or(Value::Null);
        let reply = match method.as_str() {
            "initialize" => Ok(initialize_result(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools()})),
            "tools/call" => {
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or_default().to_string();
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                // Input events block; run them off the reader task so a
                // slow action never stalls `ping`.
                let outcome = tokio::task::spawn_blocking(move || call_tool(&name, &args)).await;
                match outcome {
                    Ok(Ok(result)) => Ok(result),
                    Ok(Err(text)) => Ok(json!({"isError": true, "content": [{"type": "text", "text": text}]})),
                    Err(e) => Err((-32603, format!("the action panicked: {e}"))),
                }
            }
            other => Err((-32601, format!("method not found: {other}"))),
        };
        match reply {
            Ok(result) => write_message(&json!({"jsonrpc": "2.0", "id": id, "result": result})),
            Err((code, text)) => write_message(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": text}})),
        }
    }
    Ok(0)
}

fn write_message(message: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut out, message);
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

fn initialize_result(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or(SUPPORTED[0]);
    let version = if SUPPORTED.contains(&requested) { requested } else { SUPPORTED[0] };
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {}},
        "serverInfo": {"name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION")},
        "instructions": "Computer Use drives this Mac. Always start with `screenshot` to see the screen; coordinates are points with the origin at the top-left of the main display, exactly as shown in the screenshot. After every click, key press or typed text take another screenshot before deciding the next step; never assume an action succeeded. Prefer keyboard shortcuts for reliability. Do not type passwords or payment details, and stop and ask the user before anything irreversible (sending, deleting, paying)."
    })
}

fn tools() -> Vec<Value> {
    let read_only = json!({"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false});
    let acts = json!({"readOnlyHint": false, "destructiveHint": false, "openWorldHint": true});
    vec![
        json!({"name": "screenshot", "title": "Screenshot", "description": "Capture the main display (or `display` 1, 2, … for another) as a PNG scaled to points, and report the screen size in points. Look at the image to find what to click.", "inputSchema": {"type": "object", "properties": {"display": {"type": "integer", "minimum": 1, "description": "Display number (1 = main)"}}, "additionalProperties": false}, "annotations": read_only}),
        json!({"name": "cursor_position", "title": "Cursor position", "description": "The mouse pointer's current position in points.", "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}, "annotations": read_only}),
        json!({"name": "click", "title": "Click", "description": "Move the mouse to (x, y) and click: `button` left (default), right or middle; `count` 1 (default), 2 (double) or 3; optional `modifiers` such as [\"cmd\"], [\"shift\"], [\"alt\"], [\"ctrl\"] held during the click.", "inputSchema": {"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}, "button": {"type": "string", "enum": ["left", "right", "middle"]}, "count": {"type": "integer", "minimum": 1, "maximum": 3}, "modifiers": {"type": "array", "items": {"type": "string", "enum": ["cmd", "shift", "alt", "ctrl"]}}}, "required": ["x", "y"], "additionalProperties": false}, "annotations": acts}),
        json!({"name": "move_mouse", "title": "Move mouse", "description": "Move the pointer to (x, y) without clicking (hover).", "inputSchema": {"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}}, "required": ["x", "y"], "additionalProperties": false}, "annotations": acts}),
        json!({"name": "drag", "title": "Drag", "description": "Press the left button at (from_x, from_y), move to (to_x, to_y) and release.", "inputSchema": {"type": "object", "properties": {"from_x": {"type": "number"}, "from_y": {"type": "number"}, "to_x": {"type": "number"}, "to_y": {"type": "number"}}, "required": ["from_x", "from_y", "to_x", "to_y"], "additionalProperties": false}, "annotations": acts}),
        json!({"name": "scroll", "title": "Scroll", "description": "Scroll at (x, y) in a `direction` (up, down, left, right) by `amount` lines (default 5).", "inputSchema": {"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}, "direction": {"type": "string", "enum": ["up", "down", "left", "right"]}, "amount": {"type": "integer", "minimum": 1, "maximum": 50}}, "required": ["x", "y", "direction"], "additionalProperties": false}, "annotations": acts}),
        json!({"name": "type", "title": "Type text", "description": "Type `text` into the focused control as keystrokes (Unicode, newlines become Return). Click the control first.", "inputSchema": {"type": "object", "properties": {"text": {"type": "string", "minLength": 1}}, "required": ["text"], "additionalProperties": false}, "annotations": acts}),
        json!({"name": "key", "title": "Press keys", "description": "Press a key or shortcut such as \"return\", \"escape\", \"tab\", \"space\", \"delete\", \"up\", \"cmd+c\", \"cmd+shift+s\", \"ctrl+alt+t\", \"f5\". Letters, digits and punctuation name themselves. `repeat` presses it several times.", "inputSchema": {"type": "object", "properties": {"keys": {"type": "string", "minLength": 1}, "repeat": {"type": "integer", "minimum": 1, "maximum": 50}}, "required": ["keys"], "additionalProperties": false}, "annotations": acts}),
        json!({"name": "wait", "title": "Wait", "description": "Pause for `seconds` (at most 10) while something loads, then take a screenshot.", "inputSchema": {"type": "object", "properties": {"seconds": {"type": "number", "minimum": 0, "maximum": 10}}, "required": ["seconds"], "additionalProperties": false}, "annotations": read_only}),
        json!({"name": "open", "title": "Open app or URL", "description": "Bring an application to the front by name (`app`, e.g. \"Safari\", launching it if needed) or open a URL or file path (`url`) with its default handler.", "inputSchema": {"type": "object", "properties": {"app": {"type": "string"}, "url": {"type": "string"}}, "additionalProperties": false}, "annotations": acts}),
    ]
}

/// `Ok(result)` for the model; `Err(text)` is an `isError` result.
fn call_tool(name: &str, args: &Value) -> std::result::Result<Value, String> {
    let num = |key: &str| args.get(key).and_then(|v| v.as_f64());
    match name {
        "screenshot" => platform::screenshot(args.get("display").and_then(|d| d.as_u64()).unwrap_or(1) as u32),
        "cursor_position" => platform::cursor_position(),
        "click" => {
            let (x, y) = (num("x").ok_or("x is required")?, num("y").ok_or("y is required")?);
            let button = args.get("button").and_then(|b| b.as_str()).unwrap_or("left");
            let count = args.get("count").and_then(|c| c.as_u64()).unwrap_or(1).clamp(1, 3) as u32;
            let modifiers: Vec<String> = args.get("modifiers").and_then(|m| m.as_array()).map(|a| a.iter().filter_map(|m| m.as_str().map(str::to_string)).collect()).unwrap_or_default();
            platform::click(x, y, button, count, &modifiers)
        }
        "move_mouse" => platform::move_mouse(num("x").ok_or("x is required")?, num("y").ok_or("y is required")?),
        "drag" => platform::drag(num("from_x").ok_or("from_x is required")?, num("from_y").ok_or("from_y is required")?, num("to_x").ok_or("to_x is required")?, num("to_y").ok_or("to_y is required")?),
        "scroll" => {
            let direction = args.get("direction").and_then(|d| d.as_str()).unwrap_or("down");
            let amount = args.get("amount").and_then(|a| a.as_u64()).unwrap_or(5).clamp(1, 50) as i32;
            platform::scroll(num("x").ok_or("x is required")?, num("y").ok_or("y is required")?, direction, amount)
        }
        "type" => platform::type_text(args.get("text").and_then(|t| t.as_str()).ok_or("text is required")?),
        "key" => {
            let keys = args.get("keys").and_then(|k| k.as_str()).ok_or("keys is required")?;
            let repeat = args.get("repeat").and_then(|r| r.as_u64()).unwrap_or(1).clamp(1, 50) as u32;
            platform::press_keys(keys, repeat)
        }
        "wait" => {
            let seconds = num("seconds").unwrap_or(1.0).clamp(0.0, 10.0);
            std::thread::sleep(std::time::Duration::from_secs_f64(seconds));
            Ok(text_result(format!("waited {seconds:.1}s")))
        }
        "open" => {
            let app = args.get("app").and_then(|a| a.as_str()).map(str::trim).filter(|a| !a.is_empty());
            let url = args.get("url").and_then(|u| u.as_str()).map(str::trim).filter(|u| !u.is_empty());
            platform::open(app, url)
        }
        other => Err(format!("unknown tool `{other}`")),
    }
}

fn text_result(text: impl Into<String>) -> Value {
    json!({"content": [{"type": "text", "text": text.into()}]})
}

/// The `key` argument as modifier names and the key itself.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn parse_keys(spec: &str) -> std::result::Result<(Vec<String>, String), String> {
    let parts: Vec<&str> = spec.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Err("keys is empty".into());
    }
    if spec.trim() == "+" {
        return Ok((Vec::new(), "+".into()));
    }
    let (key, mods) = parts.split_last().expect("non-empty");
    let mut modifiers = Vec::new();
    for m in mods {
        let normalized = match m.to_ascii_lowercase().as_str() {
            "cmd" | "command" | "meta" | "super" => "cmd",
            "shift" => "shift",
            "alt" | "option" | "opt" => "alt",
            "ctrl" | "control" => "ctrl",
            other => return Err(format!("unknown modifier `{other}` (use cmd, shift, alt, ctrl)")),
        };
        modifiers.push(normalized.to_string());
    }
    Ok((modifiers, key.to_string()))
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{parse_keys, text_result};
    use core_graphics::display::{CGDisplay, CGPoint};
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton, ScrollEventUnit};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use serde_json::{json, Value};
    use std::thread::sleep;
    use std::time::Duration;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
        fn CGPreflightPostEventAccess() -> bool;
        fn CGRequestPostEventAccess() -> bool;
    }

    const CLICK_STATE_FIELD: u32 = 1; // kCGMouseEventClickState
    const HOLDER: &str = "Context (or the terminal running ctx)";

    fn source() -> Result<CGEventSource, String> {
        CGEventSource::new(CGEventSourceStateID::HIDSystemState).map_err(|_| "could not create an event source".to_string())
    }

    fn need_input_access() -> Result<(), String> {
        if unsafe { CGPreflightPostEventAccess() } {
            return Ok(());
        }
        unsafe { CGRequestPostEventAccess() };
        Err(format!("macOS has not granted Accessibility permission to {HOLDER}. Ask the user to allow it in System Settings → Privacy & Security → Accessibility (the system just prompted), then try again."))
    }

    fn need_screen_access() -> Result<(), String> {
        if unsafe { CGPreflightScreenCaptureAccess() } {
            return Ok(());
        }
        unsafe { CGRequestScreenCaptureAccess() };
        Err(format!("macOS has not granted Screen Recording permission to {HOLDER}. Ask the user to allow it in System Settings → Privacy & Security → Screen Recording (the system just prompted; the app may need to be relaunched), then try again."))
    }

    pub fn permissions() -> (bool, bool) {
        unsafe { (CGPreflightScreenCaptureAccess(), CGPreflightPostEventAccess()) }
    }

    fn main_bounds() -> (f64, f64) {
        let b = CGDisplay::main().bounds();
        (b.size.width, b.size.height)
    }

    fn post(event: CGEvent) {
        event.post(CGEventTapLocation::HID);
    }

    fn mouse_event(kind: CGEventType, point: CGPoint, button: CGMouseButton) -> Result<CGEvent, String> {
        CGEvent::new_mouse_event(source()?, kind, point, button).map_err(|_| "could not create a mouse event".to_string())
    }

    pub fn screenshot(display: u32) -> Result<Value, String> {
        need_screen_access()?;
        let dir = std::env::temp_dir().join("ctx-computer-use");
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let path = dir.join(format!("shot-{}.png", std::process::id()));
        let mut cmd = std::process::Command::new("screencapture");
        cmd.arg("-x").arg("-t").arg("png");
        if display > 1 {
            cmd.arg("-D").arg(display.to_string());
        }
        let status = cmd.arg(&path).status().map_err(|e| format!("screencapture failed to start: {e}"))?;
        if !status.success() {
            return Err(format!("screencapture exited with {status}"));
        }
        // Scale the Retina capture to points so the model's coordinates match.
        let (w, h) = main_bounds();
        let (w, h) = if display > 1 { display_bounds(display).unwrap_or((w, h)) } else { (w, h) };
        let _ = std::process::Command::new("sips").arg("-z").arg(format!("{}", h.round() as i64)).arg(format!("{}", w.round() as i64)).arg(&path).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
        let bytes = std::fs::read(&path).map_err(|e| format!("cannot read the screenshot: {e}"))?;
        let _ = std::fs::remove_file(&path);
        use base64::Engine as _;
        let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let cursor = cursor_point().map(|p| format!(" The pointer is at ({:.0}, {:.0}).", p.x, p.y)).unwrap_or_default();
        Ok(json!({"content": [
            {"type": "image", "data": data, "mimeType": "image/png"},
            {"type": "text", "text": format!("Screenshot of display {display}: {} × {} points, origin top-left; use these units for click, drag and scroll coordinates.{cursor}", w.round(), h.round())}
        ]}))
    }

    fn display_bounds(display: u32) -> Option<(f64, f64)> {
        let ids = CGDisplay::active_displays().ok()?;
        let id = *ids.get((display - 1) as usize)?;
        let b = CGDisplay::new(id).bounds();
        Some((b.size.width, b.size.height))
    }

    fn cursor_point() -> Option<CGPoint> {
        CGEvent::new(source().ok()?).ok().map(|e| e.location())
    }

    pub fn cursor_position() -> Result<Value, String> {
        let p = cursor_point().ok_or("could not read the pointer position")?;
        Ok(text_result(format!("The pointer is at ({:.0}, {:.0}).", p.x, p.y)))
    }

    fn flags_for(modifiers: &[String]) -> CGEventFlags {
        let mut flags = CGEventFlags::empty();
        for m in modifiers {
            flags |= match m.as_str() {
                "cmd" => CGEventFlags::CGEventFlagCommand,
                "shift" => CGEventFlags::CGEventFlagShift,
                "alt" => CGEventFlags::CGEventFlagAlternate,
                "ctrl" => CGEventFlags::CGEventFlagControl,
                _ => CGEventFlags::empty(),
            };
        }
        flags
    }

    pub fn move_mouse(x: f64, y: f64) -> Result<Value, String> {
        need_input_access()?;
        post(mouse_event(CGEventType::MouseMoved, CGPoint::new(x, y), CGMouseButton::Left)?);
        sleep(Duration::from_millis(40));
        Ok(text_result(format!("moved the pointer to ({x:.0}, {y:.0})")))
    }

    pub fn click(x: f64, y: f64, button: &str, count: u32, modifiers: &[String]) -> Result<Value, String> {
        need_input_access()?;
        let point = CGPoint::new(x, y);
        let (down, up, btn) = match button {
            "right" => (CGEventType::RightMouseDown, CGEventType::RightMouseUp, CGMouseButton::Right),
            "middle" => (CGEventType::OtherMouseDown, CGEventType::OtherMouseUp, CGMouseButton::Center),
            _ => (CGEventType::LeftMouseDown, CGEventType::LeftMouseUp, CGMouseButton::Left),
        };
        let flags = flags_for(modifiers);
        post(mouse_event(CGEventType::MouseMoved, point, btn)?);
        sleep(Duration::from_millis(60));
        for n in 1..=count {
            let press = mouse_event(down, point, btn)?;
            press.set_integer_value_field(CLICK_STATE_FIELD, n as i64);
            press.set_flags(flags);
            post(press);
            sleep(Duration::from_millis(30));
            let release = mouse_event(up, point, btn)?;
            release.set_integer_value_field(CLICK_STATE_FIELD, n as i64);
            release.set_flags(flags);
            post(release);
            sleep(Duration::from_millis(if count > 1 { 90 } else { 40 }));
        }
        let what = match count {
            1 => format!("{button}-clicked"),
            2 => format!("double-{button}-clicked"),
            _ => format!("triple-{button}-clicked"),
        };
        let mods = if modifiers.is_empty() { String::new() } else { format!(" with {}", modifiers.join("+")) };
        Ok(text_result(format!("{what} at ({x:.0}, {y:.0}){mods}; take a screenshot to see the result")))
    }

    pub fn drag(from_x: f64, from_y: f64, to_x: f64, to_y: f64) -> Result<Value, String> {
        need_input_access()?;
        let start = CGPoint::new(from_x, from_y);
        post(mouse_event(CGEventType::MouseMoved, start, CGMouseButton::Left)?);
        sleep(Duration::from_millis(60));
        post(mouse_event(CGEventType::LeftMouseDown, start, CGMouseButton::Left)?);
        sleep(Duration::from_millis(80));
        let steps = 12;
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let p = CGPoint::new(from_x + (to_x - from_x) * t, from_y + (to_y - from_y) * t);
            post(mouse_event(CGEventType::LeftMouseDragged, p, CGMouseButton::Left)?);
            sleep(Duration::from_millis(16));
        }
        sleep(Duration::from_millis(60));
        post(mouse_event(CGEventType::LeftMouseUp, CGPoint::new(to_x, to_y), CGMouseButton::Left)?);
        Ok(text_result(format!("dragged from ({from_x:.0}, {from_y:.0}) to ({to_x:.0}, {to_y:.0})")))
    }

    pub fn scroll(x: f64, y: f64, direction: &str, amount: i32) -> Result<Value, String> {
        need_input_access()?;
        post(mouse_event(CGEventType::MouseMoved, CGPoint::new(x, y), CGMouseButton::Left)?);
        sleep(Duration::from_millis(40));
        let (vertical, horizontal) = match direction {
            "up" => (amount, 0),
            "down" => (-amount, 0),
            "left" => (0, amount),
            "right" => (0, -amount),
            other => return Err(format!("unknown direction `{other}`")),
        };
        let event = CGEvent::new_scroll_event(source()?, ScrollEventUnit::LINE, 2, vertical, horizontal, 0).map_err(|_| "could not create a scroll event".to_string())?;
        post(event);
        sleep(Duration::from_millis(80));
        Ok(text_result(format!("scrolled {direction} by {amount} lines at ({x:.0}, {y:.0})")))
    }

    pub fn type_text(text: &str) -> Result<Value, String> {
        need_input_access()?;
        let src = source()?;
        let mut typed = 0usize;
        for segment in text.split_inclusive('\n') {
            let (body, newline) = match segment.strip_suffix('\n') {
                Some(b) => (b, true),
                None => (segment, false),
            };
            // Short chunks: long strings on one event get dropped by some apps.
            let chars: Vec<char> = body.chars().collect();
            for chunk in chars.chunks(20) {
                let s: String = chunk.iter().collect();
                let down = CGEvent::new_keyboard_event(src.clone(), 0, true).map_err(|_| "could not create a key event".to_string())?;
                down.set_string(&s);
                post(down);
                let up = CGEvent::new_keyboard_event(src.clone(), 0, false).map_err(|_| "could not create a key event".to_string())?;
                post(up);
                typed += chunk.len();
                sleep(Duration::from_millis(25));
            }
            if newline {
                press_keycode(&src, KEY_RETURN, CGEventFlags::empty())?;
                typed += 1;
            }
        }
        Ok(text_result(format!("typed {typed} characters")))
    }

    pub fn press_keys(spec: &str, repeat: u32) -> Result<Value, String> {
        need_input_access()?;
        let (modifiers, key) = parse_keys(spec)?;
        let (code, needs_shift) = keycode(&key).ok_or_else(|| format!("unknown key `{key}`"))?;
        let mut flags = flags_for(&modifiers);
        if needs_shift {
            flags |= CGEventFlags::CGEventFlagShift;
        }
        let src = source()?;
        for _ in 0..repeat {
            press_keycode(&src, code, flags)?;
            sleep(Duration::from_millis(40));
        }
        Ok(text_result(format!("pressed {spec}{}", if repeat > 1 { format!(" ×{repeat}") } else { String::new() })))
    }

    fn press_keycode(src: &CGEventSource, code: u16, flags: CGEventFlags) -> Result<(), String> {
        let down = CGEvent::new_keyboard_event(src.clone(), code, true).map_err(|_| "could not create a key event".to_string())?;
        down.set_flags(flags);
        post(down);
        sleep(Duration::from_millis(20));
        let up = CGEvent::new_keyboard_event(src.clone(), code, false).map_err(|_| "could not create a key event".to_string())?;
        up.set_flags(flags);
        post(up);
        Ok(())
    }

    pub fn open(app: Option<&str>, url: Option<&str>) -> Result<Value, String> {
        let mut cmd = std::process::Command::new("open");
        let what = match (app, url) {
            (Some(app), Some(url)) => {
                cmd.arg("-a").arg(app).arg(url);
                format!("{url} in {app}")
            }
            (Some(app), None) => {
                cmd.arg("-a").arg(app);
                app.to_string()
            }
            (None, Some(url)) => {
                cmd.arg(url);
                url.to_string()
            }
            (None, None) => return Err("give `app`, `url` or both".into()),
        };
        let output = cmd.output().map_err(|e| format!("`open` failed to start: {e}"))?;
        if !output.status.success() {
            return Err(format!("could not open {what}: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
        sleep(Duration::from_millis(700));
        Ok(text_result(format!("opened {what}; take a screenshot to see it")))
    }

    const KEY_RETURN: u16 = 36;

    /// US-layout virtual key codes; `true` when the character needs Shift.
    fn keycode(key: &str) -> Option<(u16, bool)> {
        let lower = key.to_ascii_lowercase();
        let named = match lower.as_str() {
            "return" | "enter" => 36,
            "tab" => 48,
            "space" | " " => 49,
            "delete" | "backspace" => 51,
            "forwarddelete" | "forward_delete" | "del" => 117,
            "escape" | "esc" => 53,
            "left" | "arrowleft" => 123,
            "right" | "arrowright" => 124,
            "down" | "arrowdown" => 125,
            "up" | "arrowup" => 126,
            "home" => 115,
            "end" => 119,
            "pageup" | "page_up" => 116,
            "pagedown" | "page_down" => 121,
            "capslock" => 57,
            "f1" => 122,
            "f2" => 120,
            "f3" => 99,
            "f4" => 118,
            "f5" => 96,
            "f6" => 97,
            "f7" => 98,
            "f8" => 100,
            "f9" => 101,
            "f10" => 109,
            "f11" => 103,
            "f12" => 111,
            _ => 0,
        };
        if named != 0 {
            return Some((named, false));
        }
        let mut chars = key.chars();
        let c = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        let plain = |c: char| -> Option<u16> {
            Some(match c {
                'a' => 0, 's' => 1, 'd' => 2, 'f' => 3, 'h' => 4, 'g' => 5, 'z' => 6, 'x' => 7, 'c' => 8, 'v' => 9, 'b' => 11, 'q' => 12, 'w' => 13, 'e' => 14, 'r' => 15, 'y' => 16, 't' => 17,
                '1' => 18, '2' => 19, '3' => 20, '4' => 21, '6' => 22, '5' => 23, '=' => 24, '9' => 25, '7' => 26, '-' => 27, '8' => 28, '0' => 29, ']' => 30, 'o' => 31, 'u' => 32, '[' => 33, 'i' => 34, 'p' => 35,
                'l' => 37, 'j' => 38, '\'' => 39, 'k' => 40, ';' => 41, '\\' => 42, ',' => 43, '/' => 44, 'n' => 45, 'm' => 46, '.' => 47, '`' => 50,
                _ => return None,
            })
        };
        if let Some(code) = plain(c.to_ascii_lowercase()) {
            return Some((code, c.is_ascii_uppercase()));
        }
        let shifted = match c {
            '!' => '1', '@' => '2', '#' => '3', '$' => '4', '%' => '5', '^' => '6', '&' => '7', '*' => '8', '(' => '9', ')' => '0', '_' => '-', '+' => '=', '{' => '[', '}' => ']', '|' => '\\', ':' => ';', '"' => '\'', '<' => ',', '>' => '.', '?' => '/', '~' => '`',
            _ => return None,
        };
        plain(shifted).map(|code| (code, true))
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use serde_json::Value;
    const UNSUPPORTED: &str = "Computer Use is available on macOS only";

    pub fn permissions() -> (bool, bool) {
        (false, false)
    }

    pub fn screenshot(_display: u32) -> Result<Value, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn cursor_position() -> Result<Value, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn click(_x: f64, _y: f64, _button: &str, _count: u32, _modifiers: &[String]) -> Result<Value, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn move_mouse(_x: f64, _y: f64) -> Result<Value, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn drag(_a: f64, _b: f64, _c: f64, _d: f64) -> Result<Value, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn scroll(_x: f64, _y: f64, _direction: &str, _amount: i32) -> Result<Value, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn type_text(_text: &str) -> Result<Value, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn press_keys(_spec: &str, _repeat: u32) -> Result<Value, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn open(_app: Option<&str>, _url: Option<&str>) -> Result<Value, String> {
        Err(UNSUPPORTED.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_specs_split_into_modifiers_and_key() {
        assert_eq!(parse_keys("cmd+shift+s").unwrap(), (vec!["cmd".to_string(), "shift".to_string()], "s".to_string()));
        assert_eq!(parse_keys("return").unwrap(), (vec![], "return".to_string()));
        assert_eq!(parse_keys("Option+Left").unwrap(), (vec!["alt".to_string()], "Left".to_string()));
        assert!(parse_keys("hyper+x").is_err());
    }

    #[test]
    fn tool_list_is_well_formed() {
        let tools = tools();
        assert!(tools.iter().any(|t| t["name"] == "screenshot" && t["annotations"]["readOnlyHint"] == true));
        assert!(tools.iter().any(|t| t["name"] == "click" && t["annotations"]["readOnlyHint"] == false));
        for t in &tools {
            assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
        }
        assert_eq!(initialize_result(&json!({"protocolVersion": "2024-11-05"}))["protocolVersion"], "2024-11-05");
        assert_eq!(initialize_result(&json!({"protocolVersion": "1999"}))["protocolVersion"], "2025-06-18");
    }
}
