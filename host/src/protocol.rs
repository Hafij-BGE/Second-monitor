//! Wire protocol shared with the Android viewer. See `docs/protocol.md`.
//!
//! All integers are big-endian. After the handshake every message is
//! `u8 type | u32 payload_len | payload`.

use std::io::{self, Read, Write};

pub const MAGIC: &[u8; 4] = b"SMON";
/// v2 adds a flags byte to the client hello and cursor messages.
pub const VERSION: u8 = 2;
const MIN_VERSION: u8 = 1;
pub const DEFAULT_PORT: u16 = 52020;
pub const DISCOVERY_PORT: u16 = 52021;
pub const DISCOVERY_QUERY: &[u8] = b"SMON_DISCOVER";
pub const CODEC_H264: u8 = 1;

// Host -> client
pub const MSG_VIDEO: u8 = 1;
pub const MSG_RESIZE: u8 = 2;
pub const MSG_CURSOR: u8 = 3;

/// Client hello flag: the viewer draws the pointer itself (MSG_CURSOR).
pub const FLAG_CURSOR_OVERLAY: u8 = 1;

// Client -> host
pub const MSG_TOUCH: u8 = 10;
pub const MSG_KEYFRAME: u8 = 11;

/// Largest client message we accept; guards against garbage lengths.
const MAX_CLIENT_PAYLOAD: u32 = 64 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct ClientHello {
    pub version: u8,
    pub width: u16,
    pub height: u16,
    pub dpi: u16,
    pub flags: u8,
}

pub fn read_client_hello(r: &mut impl Read) -> io::Result<ClientHello> {
    let mut buf = [0u8; 11];
    r.read_exact(&mut buf)?;
    if &buf[0..4] != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad magic"));
    }
    let version = buf[4];
    if !(MIN_VERSION..=VERSION).contains(&version) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported protocol version {version}"),
        ));
    }
    let mut flags = 0;
    if version >= 2 {
        let mut b = [0u8; 1];
        r.read_exact(&mut b)?;
        flags = b[0];
    }
    Ok(ClientHello {
        version,
        width: u16::from_be_bytes([buf[5], buf[6]]),
        height: u16::from_be_bytes([buf[7], buf[8]]),
        dpi: u16::from_be_bytes([buf[9], buf[10]]),
        flags,
    })
}

/// `version` echoes the client's, so v1 viewers still accept the reply.
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

pub fn write_video(w: &mut impl Write, pts_us: u64, data: &[u8]) -> io::Result<()> {
    let len = (8 + data.len()) as u32;
    let mut header = [0u8; 13];
    header[0] = MSG_VIDEO;
    header[1..5].copy_from_slice(&len.to_be_bytes());
    header[5..13].copy_from_slice(&pts_us.to_be_bytes());
    w.write_all(&header)?;
    w.write_all(data)
}

pub fn write_resize(w: &mut impl Write, width: u16, height: u16) -> io::Result<()> {
    let mut buf = [0u8; 9];
    buf[0] = MSG_RESIZE;
    buf[1..5].copy_from_slice(&4u32.to_be_bytes());
    buf[5..7].copy_from_slice(&width.to_be_bytes());
    buf[7..9].copy_from_slice(&height.to_be_bytes());
    w.write_all(&buf)
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
    let mut header = [0u8; 5];
    header[0] = MSG_CURSOR;
    header[1..5].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    w.write_all(&header)?;
    w.write_all(&payload)
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

pub enum ClientMessage {
    Touch(Touch),
    KeyframeRequest,
    Unknown(u8),
}

pub fn read_client_message(r: &mut impl Read) -> io::Result<ClientMessage> {
    let mut header = [0u8; 5];
    r.read_exact(&mut header)?;
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]);
    if len > MAX_CLIENT_PAYLOAD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too large",
        ));
    }
    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload)?;
    match header[0] {
        MSG_TOUCH if payload.len() >= 10 => {
            let action = match payload[0] {
                0 => TouchAction::Down,
                1 => TouchAction::Move,
                2 => TouchAction::Up,
                _ => TouchAction::Cancel,
            };
            let x = f32::from_be_bytes([payload[2], payload[3], payload[4], payload[5]]);
            let y = f32::from_be_bytes([payload[6], payload[7], payload[8], payload[9]]);
            Ok(ClientMessage::Touch(Touch {
                action,
                id: payload[1],
                x: x.clamp(0.0, 1.0),
                y: y.clamp(0.0, 1.0),
            }))
        }
        MSG_KEYFRAME => Ok(ClientMessage::KeyframeRequest),
        other => Ok(ClientMessage::Unknown(other)),
    }
}
