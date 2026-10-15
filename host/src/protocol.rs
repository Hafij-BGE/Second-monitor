//! Wire protocol shared with the Android viewer. See `docs/protocol.md`.
//!
//! All integers are big-endian. After the handshake every message is
//! `u8 type | u32 payload_len | payload`.

use std::io::{self, Read, Write};

pub const MAGIC: &[u8; 4] = b"SMON";
/// v2 added hello flags and cursor messages; v3 quality, device name,
/// mouse/keyboard input, clipboard and files; v4 runs over TLS and adds a
/// device id and pairing.
pub const VERSION: u8 = 4;
const MIN_VERSION: u8 = 4;

/// Sent by the host after the client hello (v4).
pub const AUTH_OK: u8 = 0;
/// The PC is asking the user; another auth byte follows.
pub const AUTH_PENDING: u8 = 1;
pub const AUTH_DENIED: u8 = 2;
pub const DEFAULT_PORT: u16 = 52020;
pub const DISCOVERY_PORT: u16 = 52021;
pub const DISCOVERY_QUERY: &[u8] = b"SMON_DISCOVER";
pub const CODEC_H264: u8 = 1;

// Host -> client
pub const MSG_VIDEO: u8 = 1;
pub const MSG_RESIZE: u8 = 2;
pub const MSG_CURSOR: u8 = 3;

// Client -> host
pub const MSG_TOUCH: u8 = 10;
pub const MSG_KEYFRAME: u8 = 11;
pub const MSG_MOUSE: u8 = 12;
pub const MSG_KEY: u8 = 13;

// Both directions
pub const MSG_CLIPBOARD: u8 = 20;
pub const MSG_FILE_BEGIN: u8 = 30;
pub const MSG_FILE_DATA: u8 = 31;
pub const MSG_FILE_END: u8 = 32;

/// Client hello flag: the viewer draws the pointer itself (MSG_CURSOR).
pub const FLAG_CURSOR_OVERLAY: u8 = 1;
/// Client hello flag: sync clipboard text both ways.
pub const FLAG_CLIPBOARD: u8 = 2;

/// Largest client message we accept; file chunks are the biggest.
const MAX_CLIENT_PAYLOAD: u32 = 1024 * 1024;
/// File data is sent in chunks this big so video keeps flowing.
pub const FILE_CHUNK: usize = 128 * 1024;

#[derive(Debug, Clone)]
pub struct ClientHello {
    pub version: u8,
    pub width: u16,
    pub height: u16,
    pub dpi: u16,
    pub flags: u8,
    /// 0 = host default.
    pub fps: u8,
    /// 0 = host default.
    pub bitrate_kbps: u32,
    pub name: String,
    /// Random id the app generated once, hex encoded (v4).
    pub device_id: String,
}

pub fn read_client_hello(r: &mut impl Read) -> io::Result<ClientHello> {
    let mut buf = [0u8; 11];
    r.read_exact(&mut buf)?;
    if &buf[0..4] != MAGIC {
        return Err(invalid("bad magic"));
    }
    let version = buf[4];
    if !(MIN_VERSION..=VERSION).contains(&version) {
        return Err(invalid(format!("unsupported protocol version {version}")));
    }
    let mut hello = ClientHello {
        version,
        width: u16::from_be_bytes([buf[5], buf[6]]),
        height: u16::from_be_bytes([buf[7], buf[8]]),
        dpi: u16::from_be_bytes([buf[9], buf[10]]),
        flags: 0,
        fps: 0,
        bitrate_kbps: 0,
        name: String::new(),
        device_id: String::new(),
    };
    if version >= 2 {
        hello.flags = read_u8(r)?;
    }
    if version >= 3 {
        hello.fps = read_u8(r)?;
        let mut b = [0u8; 4];
        r.read_exact(&mut b)?;
        hello.bitrate_kbps = u32::from_be_bytes(b);
        let len = read_u8(r)? as usize;
        let mut name = vec![0u8; len];
        r.read_exact(&mut name)?;
        hello.name = String::from_utf8_lossy(&name).into_owned();
    }
    if version >= 4 {
        let mut id = [0u8; 16];
        r.read_exact(&mut id)?;
        hello.device_id = id.iter().map(|b| format!("{b:02x}")).collect();
    }
    Ok(hello)
}

/// `version` echoes the client's, so older viewers still accept the reply.
pub fn write_server_hello(
    w: &mut impl Write,
    version: u8,
    width: u16,
    height: u16,
) -> io::Result<()> {
    let mut buf = Vec::with_capacity(10);
    buf.extend_from_slice(MAGIC);
    buf.push(version);
    buf.extend_from_slice(&width.to_be_bytes());
    buf.extend_from_slice(&height.to_be_bytes());
    buf.push(CODEC_H264);
    w.write_all(&buf)
}

pub fn write_auth(w: &mut impl Write, result: u8) -> io::Result<()> {
    w.write_all(&[result])?;
    w.flush()
}

fn write_message(w: &mut impl Write, kind: u8, payload: &[&[u8]]) -> io::Result<()> {
    let len: usize = payload.iter().map(|p| p.len()).sum();
    let mut header = [0u8; 5];
    header[0] = kind;
    header[1..5].copy_from_slice(&(len as u32).to_be_bytes());
    w.write_all(&header)?;
    for p in payload {
        w.write_all(p)?;
    }
    Ok(())
}

pub fn write_video(w: &mut impl Write, pts_us: u64, data: &[u8]) -> io::Result<()> {
    write_message(w, MSG_VIDEO, &[&pts_us.to_be_bytes(), data])
}

pub fn write_resize(w: &mut impl Write, width: u16, height: u16) -> io::Result<()> {
    write_message(
        w,
        MSG_RESIZE,
        &[&width.to_be_bytes(), &height.to_be_bytes()],
    )
}

pub fn write_cursor(w: &mut impl Write, c: &crate::backend::CursorUpdate) -> io::Result<()> {
    let mut payload = Vec::with_capacity(19);
    let flags = (c.visible as u8) | ((c.image.is_some() as u8) << 1);
    payload.push(flags);
    payload.extend_from_slice(&c.x.to_be_bytes());
    payload.extend_from_slice(&c.y.to_be_bytes());
    payload.extend_from_slice(&(c.source_width.min(u16::MAX as u32) as u16).to_be_bytes());
    if let Some(img) = &c.image {
        for v in [img.width, img.height, img.hot_x, img.hot_y] {
            payload.extend_from_slice(&(v.min(u16::MAX as u32) as u16).to_be_bytes());
        }
        payload.extend_from_slice(&img.rgba);
    }
    write_message(w, MSG_CURSOR, &[&payload])
}

pub fn write_clipboard(w: &mut impl Write, text: &str) -> io::Result<()> {
    write_message(w, MSG_CLIPBOARD, &[text.as_bytes()])
}

pub fn write_file_begin(w: &mut impl Write, id: u32, size: u64, name: &str) -> io::Result<()> {
    let name = name.as_bytes();
    let name = &name[..name.len().min(u16::MAX as usize)];
    write_message(
        w,
        MSG_FILE_BEGIN,
        &[
            &id.to_be_bytes(),
            &size.to_be_bytes(),
            &(name.len() as u16).to_be_bytes(),
            name,
        ],
    )
}

pub fn write_file_data(w: &mut impl Write, id: u32, data: &[u8]) -> io::Result<()> {
    write_message(w, MSG_FILE_DATA, &[&id.to_be_bytes(), data])
}

pub fn write_file_end(w: &mut impl Write, id: u32, ok: bool) -> io::Result<()> {
    write_message(w, MSG_FILE_END, &[&id.to_be_bytes(), &[(!ok) as u8]])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchAction {
    Down,
    Move,
    Up,
    Cancel,
}

/// A touch point in normalized video coordinates (0.0..=1.0).
#[derive(Debug, Clone, Copy)]
pub struct Touch {
    pub action: TouchAction,
    pub id: u8,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Trackpad-mode input from the device.
#[derive(Debug, Clone, Copy)]
pub enum MouseEvent {
    /// Absolute position, 0.0..=1.0 of the video (stylus hover).
    MoveTo {
        x: f32,
        y: f32,
    },
    /// Relative motion in host pixels.
    MoveBy {
        dx: f32,
        dy: f32,
    },
    Button {
        button: MouseButton,
        pressed: bool,
    },
    /// Smooth scroll in host pixels. `finish` ends a scroll gesture.
    Scroll {
        dx: f32,
        dy: f32,
        #[cfg_attr(windows, allow(dead_code))]
        finish: bool,
    },
}

pub enum ClientMessage {
    Touch(Touch),
    KeyframeRequest,
    Mouse(MouseEvent),
    /// X11 keysym; text uses Unicode keysyms (0x0100_0000 + code point).
    Key {
        keysym: u32,
        pressed: bool,
    },
    Clipboard(String),
    FileBegin {
        id: u32,
        size: u64,
        name: String,
    },
    FileData {
        id: u32,
        data: Vec<u8>,
    },
    FileEnd {
        id: u32,
        ok: bool,
    },
    Unknown(u8),
}

pub fn read_client_message(r: &mut impl Read) -> io::Result<ClientMessage> {
    let mut header = [0u8; 5];
    r.read_exact(&mut header)?;
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]);
    if len > MAX_CLIENT_PAYLOAD {
        return Err(invalid("message too large"));
    }
    let mut p = vec![0u8; len as usize];
    r.read_exact(&mut p)?;
    let f32_at = |i: usize| f32::from_be_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]]);
    let u32_at = |i: usize| u32::from_be_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]]);
    Ok(match header[0] {
        MSG_TOUCH if p.len() >= 10 => ClientMessage::Touch(Touch {
            action: match p[0] {
                0 => TouchAction::Down,
                1 => TouchAction::Move,
                2 => TouchAction::Up,
                _ => TouchAction::Cancel,
            },
            id: p[1],
            x: f32_at(2).clamp(0.0, 1.0),
            y: f32_at(6).clamp(0.0, 1.0),
        }),
        MSG_KEYFRAME => ClientMessage::KeyframeRequest,
        MSG_MOUSE if !p.is_empty() => match (p[0], p.len()) {
            (0, 9..) => ClientMessage::Mouse(MouseEvent::MoveTo {
                x: f32_at(1).clamp(0.0, 1.0),
                y: f32_at(5).clamp(0.0, 1.0),
            }),
            (1, 9..) => ClientMessage::Mouse(MouseEvent::MoveBy {
                dx: f32_at(1),
                dy: f32_at(5),
            }),
            (2, 3..) => ClientMessage::Mouse(MouseEvent::Button {
                button: match p[1] {
                    1 => MouseButton::Right,
                    2 => MouseButton::Middle,
                    _ => MouseButton::Left,
                },
                pressed: p[2] != 0,
            }),
            (3, 10..) => ClientMessage::Mouse(MouseEvent::Scroll {
                dx: f32_at(1),
                dy: f32_at(5),
                finish: p[9] != 0,
            }),
            _ => ClientMessage::Unknown(MSG_MOUSE),
        },
        MSG_KEY if p.len() >= 5 => ClientMessage::Key {
            keysym: u32_at(0),
            pressed: p[4] != 0,
        },
        MSG_CLIPBOARD => ClientMessage::Clipboard(String::from_utf8_lossy(&p).into_owned()),
        MSG_FILE_BEGIN if p.len() >= 14 => {
            let name_len = u16::from_be_bytes([p[12], p[13]]) as usize;
            let name_end = (14 + name_len).min(p.len());
            ClientMessage::FileBegin {
                id: u32_at(0),
                size: u64::from_be_bytes(p[4..12].try_into().unwrap()),
                name: String::from_utf8_lossy(&p[14..name_end]).into_owned(),
            }
        }
        MSG_FILE_DATA if p.len() >= 4 => ClientMessage::FileData {
            id: u32_at(0),
            data: p[4..].to_vec(),
        },
        MSG_FILE_END if p.len() >= 5 => ClientMessage::FileEnd {
            id: u32_at(0),
            ok: p[4] == 0,
        },
        other => ClientMessage::Unknown(other),
    })
}

fn read_u8(r: &mut impl Read) -> io::Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}
