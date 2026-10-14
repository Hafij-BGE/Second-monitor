//! Screen capture + input injection, one implementation per OS.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use anyhow::Result;

use crate::protocol::Touch;

#[cfg(target_os = "linux")]
mod mutter;
#[cfg(target_os = "linux")]
use mutter as platform;

#[cfg(windows)]
mod dxgi;
#[cfg(windows)]
use dxgi as platform;

#[cfg(not(any(target_os = "linux", windows)))]
compile_error!("second-monitor supports Linux (GNOME) and Windows hosts only");

/// A captured frame, tightly packed BGRA (stride = width * 4).
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Mode {
    /// Add a new virtual monitor (Linux: GNOME virtual monitor; Windows: a
    /// non-primary monitor such as one from Virtual Display Driver).
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
}

pub trait InputSink: Send {
    fn touch(&mut self, touch: Touch);
}

/// A running capture session. Dropping it stops capture.
pub struct Capture {
    pub frames: Arc<FrameSlot>,
    pub input: Arc<Mutex<Box<dyn InputSink>>>,
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
