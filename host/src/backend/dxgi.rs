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
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use super::{Capture, CaptureRequest, Frame, FrameSlot, InputSink, Mode};
use crate::protocol::{Touch, TouchAction};

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
                    "only one monitor found, so mirroring it. Install Virtual Display Driver \
                     (github.com/VirtualDrivers/Virtual-Display-Driver) to extend instead"
                );
                0
            }
        },
        Mode::Mirror => outputs.iter().position(|o| o.is_primary()).unwrap_or(0),
    };
    Ok(outputs.swap_remove(index))
}

pub fn start(req: &CaptureRequest) -> Result<Capture> {
    let target = pick_output(req)?;
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
        thread::Builder::new().name("dxgi".into()).spawn(move || {
            // COM objects are not Send: re-enumerate inside the capture thread.
            let result = (|| -> Result<()> {
                let req = CaptureRequest {
                    mode,
                    monitor,
                    width: 0,
                    height: 0,
                    cursor_overlay: false,
                };
                let target = pick_output(&req)?;
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
        }))),
        // Desktop duplication gives the pointer shape too, but for now it is
        // drawn into frames.
        cursor: None,
        stop: Some(Box::new(move || stop_flag.store(false, Ordering::Relaxed))),
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

/// Maps touches on the phone to the mouse on the streamed monitor. Only the
/// first finger is used.
struct MouseInput {
    rect: Arc<Mutex<RECT>>,
    pressed: bool,
}

impl InputSink for MouseInput {
    fn touch(&mut self, t: Touch) {
        if t.id != 0 {
            return;
        }
        let r = *self.rect.lock().unwrap();
        let px = r.left as f32 + t.x * (r.right - r.left - 1) as f32;
        let py = r.top as f32 + t.y * (r.bottom - r.top - 1) as f32;
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
        unsafe {
            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN) as f32;
            let vy = GetSystemMetrics(SM_YVIRTUALSCREEN) as f32;
            let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2) as f32;
            let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2) as f32;
            let input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: ((px - vx) * 65535.0 / (vw - 1.0)) as i32,
                        dy: ((py - vy) * 65535.0 / (vh - 1.0)) as i32,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_MOVE
                            | MOUSEEVENTF_ABSOLUTE
                            | MOUSEEVENTF_VIRTUALDESK
                            | button,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    }
}
