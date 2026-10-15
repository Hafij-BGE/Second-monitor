//! Windows backend: DXGI Desktop Duplication for capture, SendInput for touch.
//!
//! Windows has no public API to create a monitor, so extend mode streams an
//! existing non-primary monitor. Install a virtual display driver (for
//! example https://github.com/VirtualDrivers/Virtual-Display-Driver) to get one
//! without a physical screen.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use anyhow::{anyhow, bail, Context, Result};
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication,
    IDXGIResource, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_NOT_FOUND, DXGI_ERROR_WAIT_TIMEOUT,
    DXGI_OUTDUPL_FRAME_INFO,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL,
    MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use super::vdd;
use super::{Capture, CaptureRequest, Frame, FrameSlot, InputSink, Mode};
use crate::protocol::{MouseButton, MouseEvent, Touch, TouchAction};

struct OutputInfo {
    adapter: IDXGIAdapter1,
    output: IDXGIOutput1,
    name: String,
    rect: RECT,
}

impl OutputInfo {
    fn is_primary(&self) -> bool {
        self.rect.left == 0 && self.rect.top == 0
    }
    fn width(&self) -> u32 {
        (self.rect.right - self.rect.left) as u32
    }
    fn height(&self) -> u32 {
        (self.rect.bottom - self.rect.top) as u32
    }
}

fn enumerate_outputs() -> Result<Vec<OutputInfo>> {
    let mut out = Vec::new();
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
        for a in 0.. {
            let adapter = match factory.EnumAdapters1(a) {
                Ok(adapter) => adapter,
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(e) => return Err(e.into()),
            };
            for o in 0.. {
                let output = match adapter.EnumOutputs(o) {
                    Ok(output) => output,
                    Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                    Err(e) => return Err(e.into()),
                };
                let desc = output.GetDesc()?;
                if !desc.AttachedToDesktop.as_bool() {
                    continue;
                }
                let len = desc.DeviceName.iter().position(|&c| c == 0).unwrap_or(32);
                out.push(OutputInfo {
                    adapter: adapter.clone(),
                    output: output.cast()?,
                    name: String::from_utf16_lossy(&desc.DeviceName[..len]),
                    rect: desc.DesktopCoordinates,
                });
            }
        }
    }
    Ok(out)
}

pub fn list_monitors() -> Result<Vec<String>> {
    Ok(enumerate_outputs()?
        .iter()
        .enumerate()
        .map(|(i, o)| {
            format!(
                "{i}: {} {}x{} at ({}, {}){}",
                o.name,
                o.width(),
                o.height(),
                o.rect.left,
                o.rect.top,
                if o.is_primary() { " primary" } else { "" }
            )
        })
        .collect())
}

fn find_output(device: &str) -> Option<OutputInfo> {
    enumerate_outputs()
        .ok()?
        .into_iter()
        .find(|o| o.name == device)
}

fn pick_output(req: &CaptureRequest) -> Result<OutputInfo> {
    let mut outputs = enumerate_outputs()?;
    if outputs.is_empty() {
        bail!("no monitors found");
    }
    if let Some(m) = &req.monitor {
        let index: usize = m
            .parse()
            .context("--monitor must be an index from --list")?;
        if index >= outputs.len() {
            bail!("monitor {index} does not exist; see --list");
        }
        return Ok(outputs.swap_remove(index));
    }
    let index = match req.mode {
        Mode::Extend => match outputs.iter().rposition(|o| !o.is_primary()) {
            Some(i) => i,
            None => {
                log::warn!(
                    "only one monitor, so mirroring it. To extend, click \"Set up extend\" \
                     in Extendo (installs Virtual Display Driver)"
                );
                0
            }
        },
        Mode::Mirror => outputs.iter().position(|o| o.is_primary()).unwrap_or(0),
    };
    Ok(outputs.swap_remove(index))
}

pub fn start(req: &CaptureRequest) -> Result<Capture> {
    // Extend: give the tablet its own virtual monitor (Virtual Display
    // Driver), switched on for this connection only.
    let mut virtual_on = false;
    let mut chosen: Option<OutputInfo> = None;
    if req.mode == Mode::Extend
        && req.monitor.is_none()
        && req.manage_virtual_display
        && vdd::installed()
    {
        if vdd::output_name().is_none() {
            log::info!(
                "adding a virtual display for the tablet ({}x{})",
                req.width,
                req.height
            );
            if let Err(e) = vdd::turn_on(req.width, req.height, || vdd::output_name().is_some()) {
                log::warn!("{e:#}");
            }
        }
        if let Some(device) = vdd::output_name() {
            vdd::set_resolution(&device, req.width, req.height);
            chosen = find_output(&device);
            virtual_on = true;
        }
    }
    let target = match chosen {
        Some(t) => t,
        None => pick_output(req)?,
    };
    let name = target.name.clone();
    log::info!("capturing {name} ({}x{})", target.width(), target.height());

    let slot = Arc::new(FrameSlot::default());
    let rect = Arc::new(Mutex::new(target.rect));
    let running = Arc::new(AtomicBool::new(true));
    let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
    {
        let slot = slot.clone();
        let rect = rect.clone();
        let running = running.clone();
        let monitor = req.monitor.clone();
        let mode = req.mode;
        let device = name.clone();
        thread::Builder::new().name("dxgi".into()).spawn(move || {
            // COM objects are not Send: re-enumerate inside the capture thread.
            let result = (|| -> Result<()> {
                let req = CaptureRequest {
                    mode,
                    monitor,
                    width: 0,
                    height: 0,
                    cursor_overlay: false,
                    manage_virtual_display: false,
                };
                // Same monitor as chosen above, looked up again by name.
                let target = match find_output(&device) {
                    Some(t) => t,
                    None => pick_output(&req)?,
                };
                let mut dup = Duplicator::new(&target)?;
                let _ = ready_tx.send(Ok(()));
                dup.run(&slot, &rect, &running)
            })();
            if let Err(e) = result {
                let msg = format!("{e:#}");
                let _ = ready_tx.send(Err(anyhow!(msg.clone())));
                slot.fail(msg);
            }
        })?;
    }
    ready_rx
        .recv()
        .map_err(|_| anyhow!("capture thread exited"))?
        .with_context(|| format!("cannot capture {name}"))?;

    let stop_flag = running.clone();
    Ok(Capture {
        frames: slot,
        input: Arc::new(Mutex::new(Box::new(MouseInput {
            rect,
            pressed: false,
            carry: (0.0, 0.0),
            scroll_carry: (0.0, 0.0),
        }))),
        // Desktop duplication gives the pointer shape too, but for now it is
        // drawn into frames.
        cursor: None,
        stop: Some(Box::new(move || {
            stop_flag.store(false, Ordering::Relaxed);
            if virtual_on {
                // Remove the virtual monitor once capture has let go of it.
                thread::spawn(|| {
                    thread::sleep(std::time::Duration::from_millis(500));
                    match vdd::set_display_count(0) {
                        Ok(()) => log::info!("virtual display removed"),
                        Err(e) => log::warn!("couldn't remove the virtual display: {e:#}"),
                    }
                });
            }
        })),
    })
}

struct Duplicator {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    output: IDXGIOutput1,
    dup: IDXGIOutputDuplication,
    staging: Option<(ID3D11Texture2D, u32, u32)>,
    cursor: (i32, i32, bool),
    origin: (i32, i32),
}

impl Duplicator {
    fn new(target: &OutputInfo) -> Result<Self> {
        unsafe {
            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(
                &target.adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .context("D3D11CreateDevice")?;
            let device = device.ok_or_else(|| anyhow!("no D3D11 device"))?;
            let context = context.ok_or_else(|| anyhow!("no D3D11 context"))?;
            let dup = target
                .output
                .DuplicateOutput(&device)
                .context("DuplicateOutput (another app may already be duplicating this screen)")?;
            Ok(Self {
                device,
                context,
                output: target.output.clone(),
                dup,
                staging: None,
                cursor: (0, 0, false),
                origin: (target.rect.left, target.rect.top),
            })
        }
    }

    fn run(&mut self, slot: &FrameSlot, rect: &Mutex<RECT>, running: &AtomicBool) -> Result<()> {
        while running.load(Ordering::Relaxed) {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource: Option<IDXGIResource> = None;
            let acquired = unsafe { self.dup.AcquireNextFrame(100, &mut info, &mut resource) };
            match acquired {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => continue,
                Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                    // Mode change, UAC prompt, lock screen...: recreate.
                    log::info!("desktop duplication lost, recreating");
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    self.dup = unsafe { self.output.DuplicateOutput(&self.device)? };
                    let desc = unsafe { self.output.GetDesc()? };
                    *rect.lock().unwrap() = desc.DesktopCoordinates;
                    self.origin = (desc.DesktopCoordinates.left, desc.DesktopCoordinates.top);
                    continue;
                }
                Err(e) => return Err(e.into()),
            }

            if info.LastMouseUpdateTime != 0 {
                let p = info.PointerPosition;
                self.cursor = (p.Position.x, p.Position.y, p.Visible.as_bool());
            }

            let frame = if info.LastPresentTime != 0 || info.LastMouseUpdateTime != 0 {
                resource.as_ref().map(|r| self.copy_frame(r)).transpose()
            } else {
                Ok(None)
            };
            unsafe {
                let _ = self.dup.ReleaseFrame();
            }
            if let Some(mut frame) = frame? {
                if self.cursor.2 {
                    draw_cursor(&mut frame, self.cursor.0, self.cursor.1);
                }
                slot.put(frame);
            }
        }
        Ok(())
    }

    fn copy_frame(&mut self, resource: &IDXGIResource) -> Result<Frame> {
        unsafe {
            let texture: ID3D11Texture2D = resource.cast()?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut desc);
            let needs_new =
                !matches!(&self.staging, Some((_, w, h)) if *w == desc.Width && *h == desc.Height);
            if needs_new {
                let mut staging_desc = desc;
                staging_desc.Usage = D3D11_USAGE_STAGING;
                staging_desc.BindFlags = 0;
                staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                staging_desc.MiscFlags = 0;
                staging_desc.MipLevels = 1;
                staging_desc.ArraySize = 1;
                let mut staging = None;
                self.device
                    .CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
                let staging = staging.ok_or_else(|| anyhow!("no staging texture"))?;
                self.staging = Some((staging, desc.Width, desc.Height));
            }
            let (staging, w, h) = self.staging.as_ref().unwrap();
            let (w, h) = (*w as usize, *h as usize);
            self.context.CopyResource(staging, &texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let pitch = mapped.RowPitch as usize;
            let src = std::slice::from_raw_parts(mapped.pData as *const u8, pitch * h);
            let mut bgra = Vec::with_capacity(w * h * 4);
            for row in 0..h {
                bgra.extend_from_slice(&src[row * pitch..row * pitch + w * 4]);
            }
            self.context.Unmap(staging, 0);
            Ok(Frame {
                width: w as u32,
                height: h as u32,
                bgra,
            })
        }
    }
}

/// Desktop duplication excludes the mouse pointer, so draw a simple arrow.
fn draw_cursor(frame: &mut Frame, x: i32, y: i32) {
    const SHAPE: [&str; 16] = [
        "X...........",
        "XX..........",
        "XOX.........",
        "XOOX........",
        "XOOOX.......",
        "XOOOOX......",
        "XOOOOOX.....",
        "XOOOOOOX....",
        "XOOOOOOOX...",
        "XOOOOOOOOX..",
        "XOOOOOXXXXX.",
        "XOOXOOX.....",
        "XOX.XOOX....",
        "XX..XOOX....",
        "X....XOOX...",
        ".....XXX....",
    ];
    let (fw, fh) = (frame.width as i32, frame.height as i32);
    for (dy, line) in SHAPE.iter().enumerate() {
        for (dx, c) in line.bytes().enumerate() {
            let (px, py) = (x + dx as i32, y + dy as i32);
            if px < 0 || py < 0 || px >= fw || py >= fh {
                continue;
            }
            let color = match c {
                b'X' => [0, 0, 0, 255],
                b'O' => [255, 255, 255, 255],
                _ => continue,
            };
            let i = ((py * fw + px) * 4) as usize;
            frame.bgra[i..i + 4].copy_from_slice(&color);
        }
    }
}

/// Maps touches on the phone to the mouse on the streamed monitor (only the
/// first finger), and trackpad/keyboard events to SendInput.
struct MouseInput {
    rect: Arc<Mutex<RECT>>,
    pressed: bool,
    /// Sub-pixel remainder of relative motion and scrolling.
    carry: (f32, f32),
    scroll_carry: (f32, f32),
}

impl MouseInput {
    fn send_mouse(&self, dx: i32, dy: i32, data: i32, flags: MOUSE_EVENT_FLAGS) {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data as u32,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    }

    /// Absolute move to (x, y) in 0..1 of the streamed monitor.
    fn move_to(&self, x: f32, y: f32, extra: MOUSE_EVENT_FLAGS) {
        let r = *self.rect.lock().unwrap();
        let px = r.left as f32 + x * (r.right - r.left - 1) as f32;
        let py = r.top as f32 + y * (r.bottom - r.top - 1) as f32;
        let (vx, vy, vw, vh) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN) as f32,
                GetSystemMetrics(SM_YVIRTUALSCREEN) as f32,
                GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2) as f32,
                GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2) as f32,
            )
        };
        self.send_mouse(
            ((px - vx) * 65535.0 / (vw - 1.0)) as i32,
            ((py - vy) * 65535.0 / (vh - 1.0)) as i32,
            0,
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK | extra,
        );
    }
}

impl InputSink for MouseInput {
    fn touch(&mut self, t: Touch) {
        if t.id != 0 {
            return;
        }
        let button = match t.action {
            TouchAction::Down if !self.pressed => {
                self.pressed = true;
                MOUSEEVENTF_LEFTDOWN
            }
            TouchAction::Up | TouchAction::Cancel if self.pressed => {
                self.pressed = false;
                MOUSEEVENTF_LEFTUP
            }
            _ => Default::default(),
        };
        self.move_to(t.x, t.y, button);
    }

    fn mouse(&mut self, event: MouseEvent) {
        match event {
            MouseEvent::MoveTo { x, y } => self.move_to(x, y, Default::default()),
            MouseEvent::MoveBy { dx, dy } => {
                let (fx, fy) = (dx + self.carry.0, dy + self.carry.1);
                let (ix, iy) = (fx.trunc(), fy.trunc());
                self.carry = (fx - ix, fy - iy);
                if ix != 0.0 || iy != 0.0 {
                    self.send_mouse(ix as i32, iy as i32, 0, MOUSEEVENTF_MOVE);
                }
            }
            MouseEvent::Button { button, pressed } => {
                let flags = match (button, pressed) {
                    (MouseButton::Left, true) => MOUSEEVENTF_LEFTDOWN,
                    (MouseButton::Left, false) => MOUSEEVENTF_LEFTUP,
                    (MouseButton::Right, true) => MOUSEEVENTF_RIGHTDOWN,
                    (MouseButton::Right, false) => MOUSEEVENTF_RIGHTUP,
                    (MouseButton::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
                    (MouseButton::Middle, false) => MOUSEEVENTF_MIDDLEUP,
                };
                self.send_mouse(0, 0, 0, flags);
            }
            MouseEvent::Scroll { dx, dy, .. } => {
                // One wheel notch (120) per ~40 px of finger travel.
                const PX_PER_NOTCH: f32 = 40.0;
                let sx = dx * 120.0 / PX_PER_NOTCH + self.scroll_carry.0;
                let sy = dy * 120.0 / PX_PER_NOTCH + self.scroll_carry.1;
                let (ix, iy) = (sx.trunc(), sy.trunc());
                self.scroll_carry = (sx - ix, sy - iy);
                if iy != 0.0 {
                    // Windows wheel is positive away from the user.
                    self.send_mouse(0, 0, -(iy as i32), MOUSEEVENTF_WHEEL);
                }
                if ix != 0.0 {
                    self.send_mouse(0, 0, ix as i32, MOUSEEVENTF_HWHEEL);
                }
            }
        }
    }

    fn key(&mut self, keysym: u32, pressed: bool) {
        let up = if pressed {
            Default::default()
        } else {
            KEYEVENTF_KEYUP
        };
        let input = match keysym_to_vk(keysym) {
            Some(vk) => KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: up,
                time: 0,
                dwExtraInfo: 0,
            },
            None => {
                let cp = if keysym >= 0x0100_0000 {
                    keysym - 0x0100_0000
                } else {
                    keysym
                };
                let Some(ch) = char::from_u32(cp) else { return };
                let mut utf16 = [0u16; 2];
                let unit = ch.encode_utf16(&mut utf16)[0];
                KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: unit,
                    dwFlags: KEYEVENTF_UNICODE | up,
                    time: 0,
                    dwExtraInfo: 0,
                }
            }
        };
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: input },
        };
        unsafe {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    }
}

/// Virtual-key code for X11 keysyms that aren't plain text.
fn keysym_to_vk(keysym: u32) -> Option<u16> {
    Some(match keysym {
        0xff08 => 0x08,                                     // BackSpace
        0xff09 => 0x09,                                     // Tab
        0xff0d => 0x0d,                                     // Return
        0xff1b => 0x1b,                                     // Escape
        0xffff => 0x2e,                                     // Delete
        0xff50 => 0x24,                                     // Home
        0xff51 => 0x25,                                     // Left
        0xff52 => 0x26,                                     // Up
        0xff53 => 0x27,                                     // Right
        0xff54 => 0x28,                                     // Down
        0xff55 => 0x21,                                     // Page Up
        0xff56 => 0x22,                                     // Page Down
        0xff57 => 0x23,                                     // End
        0xff63 => 0x2d,                                     // Insert
        0xffe1 | 0xffe2 => 0x10,                            // Shift
        0xffe3 | 0xffe4 => 0x11,                            // Control
        0xffe9 | 0xffea => 0x12,                            // Alt
        0xffeb | 0xffec => 0x5b,                            // Super -> Windows key
        0xffbe..=0xffc9 => 0x70 + (keysym - 0xffbe) as u16, // F1-F12
        // Letters and digits as virtual keys, so Ctrl+C etc. work.
        0x61..=0x7a => (keysym - 0x20) as u16,
        0x41..=0x5a | 0x30..=0x39 => keysym as u16,
        _ => return None,
    })
}
