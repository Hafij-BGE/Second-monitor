//! Screen capture + input injection, one implementation per OS.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use anyhow::Result;

use crate::protocol::{MouseEvent, Touch};

#[cfg(target_os = "linux")]
mod mutter;
#[cfg(target_os = "linux")]
use mutter as platform;

#[cfg(windows)]
mod dxgi;
#[cfg(windows)]
mod parsec;
#[cfg(windows)]
mod vdd;
#[cfg(windows)]
use dxgi as platform;

#[cfg(not(any(target_os = "linux", windows)))]
compile_error!("Extendo supports Linux (GNOME) and Windows hosts only");

/// A captured frame, tightly packed BGRA (stride = width * 4).
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    /// Add a new virtual monitor (Linux: GNOME virtual monitor; Windows: one
    /// from a virtual display driver, else a non-primary monitor).
    Extend,
    /// Show an existing monitor.
    Mirror,
}

pub struct CaptureRequest {
    pub mode: Mode,
    /// Linux: connector name (e.g. "eDP-1"). Windows: monitor index from `--list`.
    pub monitor: Option<String>,
    /// Preferred size; only honoured where the backend can create monitors.
    #[cfg_attr(windows, allow(dead_code))]
    pub width: u32,
    #[cfg_attr(windows, allow(dead_code))]
    pub height: u32,
    /// Send the pointer separately (see [`CursorUpdate`]) instead of drawing
    /// it into frames. Backends that can't do this ignore it.
    #[cfg_attr(windows, allow(dead_code))]
    pub cursor_overlay: bool,
    /// Windows: switch the virtual display on only while streaming.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub manage_virtual_display: bool,
}

/// Pointer state for viewers that draw the cursor themselves, so moving the
/// mouse needs no video encoding.
#[derive(Clone, Debug)]
pub struct CursorUpdate {
    /// Hotspot position as a fraction of the frame (0.0..=1.0).
    pub x: f32,
    pub y: f32,
    pub visible: bool,
    /// Frame width the image is sized for, so the viewer can scale it.
    pub source_width: u32,
    /// New cursor image, only when the shape changed.
    pub image: Option<CursorImage>,
}

#[derive(Clone, Debug)]
pub struct CursorImage {
    pub width: u32,
    pub height: u32,
    pub hot_x: u32,
    pub hot_y: u32,
    /// Premultiplied RGBA, tightly packed.
    pub rgba: Vec<u8>,
}

pub trait InputSink: Send {
    fn touch(&mut self, touch: Touch);
    fn mouse(&mut self, event: MouseEvent);
    /// X11 keysym (text uses 0x0100_0000 + Unicode code point).
    fn key(&mut self, keysym: u32, pressed: bool);
}

/// A running capture session. Dropping it stops capture.
pub struct Capture {
    pub frames: Arc<FrameSlot>,
    pub input: Arc<Mutex<Box<dyn InputSink>>>,
    /// Set when the backend honoured `cursor_overlay`.
    pub cursor: Option<std::sync::mpsc::Receiver<CursorUpdate>>,
    stop: Option<Box<dyn FnOnce() + Send>>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

pub fn start(req: &CaptureRequest) -> Result<Capture> {
    platform::start(req)
}

pub fn list_monitors() -> Result<Vec<String>> {
    platform::list_monitors()
}

/// Windows only: whether a virtual display driver for "extend" is
/// installed. `None` where extend works without it (Linux/GNOME).
pub fn virtual_display_installed() -> Option<bool> {
    #[cfg(windows)]
    {
        Some(parsec::installed() || vdd::installed())
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Windows only: true when extend uses Virtual Display Driver, whose screen
/// can also stay on between connections. Parsec's exists only while
/// Extendo runs.
pub fn virtual_display_can_stay_on() -> bool {
    #[cfg(windows)]
    {
        !parsec::installed() && vdd::installed()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Windows only: the tablet size the virtual display lacked on the last
/// connection, so the tablet letterboxed.
pub fn virtual_display_missing_size() -> Option<(u32, u32)> {
    #[cfg(windows)]
    {
        parsec::missing_size()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Windows only: turns the managed virtual display off while no tablet is
/// connected, so windows don't land on a screen nobody can see.
pub fn virtual_display_idle() {
    #[cfg(windows)]
    if vdd::installed() && vdd::display_count() > 0 {
        match vdd::set_display_count(0) {
            Ok(()) => log::info!("virtual display off until a tablet connects"),
            Err(e) => log::debug!("virtual display: {e:#}"),
        }
    }
}

/// Windows only: installs Parsec Virtual Display Driver and adds the
/// tablet's `size` to its sizes (asks for admin).
pub fn install_virtual_display(size: Option<(u32, u32)>) {
    #[cfg(windows)]
    parsec::install(size);
    #[cfg(not(windows))]
    let _ = size;
}

/// Single-slot mailbox: the producer overwrites, the consumer always gets the
/// newest frame. Old frames are dropped instead of queuing up latency.
#[derive(Default)]
pub struct FrameSlot {
    state: Mutex<SlotState>,
    ready: Condvar,
}

#[derive(Default)]
struct SlotState {
    frame: Option<Frame>,
    error: Option<String>,
}

impl FrameSlot {
    pub fn put(&self, frame: Frame) {
        self.state.lock().unwrap().frame = Some(frame);
        self.ready.notify_one();
    }

    pub fn fail(&self, error: String) {
        self.state.lock().unwrap().error = Some(error);
        self.ready.notify_one();
    }

    /// Waits up to `timeout` for a new frame. `Err` means capture died.
    pub fn take(&self, timeout: Duration) -> Result<Option<Frame>, String> {
        let guard = self.state.lock().unwrap();
        let (mut guard, _) = self
            .ready
            .wait_timeout_while(guard, timeout, |s| s.frame.is_none() && s.error.is_none())
            .unwrap();
        if let Some(e) = guard.error.take() {
            return Err(e);
        }
        Ok(guard.frame.take())
    }
}
