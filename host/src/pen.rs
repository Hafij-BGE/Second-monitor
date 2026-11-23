//! Pen pressure on Linux: a virtual drawing tablet made with uinput, so
//! Krita, GIMP and others see pressure and tilt from the tablet's stylus.
//!
//! The tablet covers the whole desktop, so each sample is placed inside the
//! monitor Extendo shows on the tablet (read from Mutter's DisplayConfig).
//! /dev/uinput needs the udev rule the .deb installs (uaccess).

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use zbus::blocking::Connection;
use zbus::zvariant::OwnedValue;

use crate::backend::Mode;
use crate::protocol::{Pen, PenAction};

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_ABS: u16 = 3;
const SYN_REPORT: u16 = 0;
const BTN_TOOL_PEN: u16 = 0x140;
const BTN_TOOL_RUBBER: u16 = 0x141;
const BTN_TOUCH: u16 = 0x14a;
const BTN_STYLUS: u16 = 0x14b;
const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_PRESSURE: u16 = 0x18;
const ABS_TILT_X: u16 = 0x1a;
const ABS_TILT_Y: u16 = 0x1b;
const INPUT_PROP_POINTER: i32 = 0;
const BUS_VIRTUAL: u16 = 0x06;

const UI_DEV_CREATE: libc::c_ulong = 0x5501;
const UI_DEV_DESTROY: libc::c_ulong = 0x5502;
const UI_DEV_SETUP: libc::c_ulong = 0x405c_5503;
const UI_ABS_SETUP: libc::c_ulong = 0x401c_5504;
const UI_SET_EVBIT: libc::c_ulong = 0x4004_5564;
const UI_SET_KEYBIT: libc::c_ulong = 0x4004_5565;
const UI_SET_ABSBIT: libc::c_ulong = 0x4004_5567;
const UI_SET_PROPBIT: libc::c_ulong = 0x4004_556e;

const XY_MAX: i32 = 32767;
const PRESSURE_MAX: i32 = 4095;

#[repr(C)]
struct InputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

#[repr(C)]
struct UinputSetup {
    id: InputId,
    name: [u8; 80],
    ff_effects_max: u32,
}

#[repr(C)]
#[derive(Default)]
struct AbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

#[repr(C)]
struct UinputAbsSetup {
    code: u16,
    absinfo: AbsInfo,
}

/// A rectangle in desktop (stage) coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

pub struct PenDevice {
    file: File,
    mode: Mode,
    monitor: Option<String>,
    /// (monitor shown on the tablet, whole desktop), refreshed now and then.
    layout: Option<(Rect, Rect)>,
    layout_at: Option<Instant>,
    in_range: bool,
    eraser: bool,
    touching: bool,
    barrel: bool,
}

impl PenDevice {
    pub fn open(mode: Mode, monitor: Option<String>) -> Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open("/dev/uinput")
            .context("open /dev/uinput")?;
        let fd = file.as_raw_fd();
        let set = |req: libc::c_ulong, v: i32| -> Result<()> {
            // SAFETY: plain integer ioctls on our own uinput fd.
            if unsafe { libc::ioctl(fd, req as _, v) } < 0 {
                bail!("uinput ioctl {req:#x}: {}", std::io::Error::last_os_error());
            }
            Ok(())
        };
        set(UI_SET_EVBIT, EV_KEY as i32)?;
        set(UI_SET_EVBIT, EV_ABS as i32)?;
        for key in [BTN_TOOL_PEN, BTN_TOOL_RUBBER, BTN_TOUCH, BTN_STYLUS] {
            set(UI_SET_KEYBIT, key as i32)?;
        }
        set(UI_SET_PROPBIT, INPUT_PROP_POINTER)?;
        // Resolutions: units per mm for x/y, per radian for tilt (degrees).
        for (code, min, max, res) in [
            (ABS_X, 0, XY_MAX, 100),
            (ABS_Y, 0, XY_MAX, 100),
            (ABS_PRESSURE, 0, PRESSURE_MAX, 0),
            (ABS_TILT_X, -90, 90, 57),
            (ABS_TILT_Y, -90, 90, 57),
        ] {
            set(UI_SET_ABSBIT, code as i32)?;
            let setup = UinputAbsSetup {
                code,
                absinfo: AbsInfo {
                    minimum: min,
                    maximum: max,
                    resolution: res,
                    ..Default::default()
                },
            };
            // SAFETY: the struct matches `struct uinput_abs_setup`.
            if unsafe { libc::ioctl(fd, UI_ABS_SETUP as _, &setup) } < 0 {
                bail!("uinput abs setup: {}", std::io::Error::last_os_error());
            }
        }
        let mut name = [0u8; 80];
        let label = b"Extendo Pen";
        name[..label.len()].copy_from_slice(label);
        let setup = UinputSetup {
            id: InputId {
                bustype: BUS_VIRTUAL,
                vendor: 0x1209,
                product: 0x0e70,
                version: 1,
            },
            name,
            ff_effects_max: 0,
        };
        // SAFETY: the struct matches `struct uinput_setup`.
        unsafe {
            if libc::ioctl(fd, UI_DEV_SETUP as _, &setup) < 0 {
                bail!("uinput setup: {}", std::io::Error::last_os_error());
            }
            if libc::ioctl(fd, UI_DEV_CREATE as _) < 0 {
                bail!("uinput create: {}", std::io::Error::last_os_error());
            }
        }
        log::info!("pen pressure on: virtual drawing tablet \"Extendo Pen\" ready");
        Ok(Self {
            file,
            mode,
            monitor,
            layout: None,
            layout_at: None,
            in_range: false,
            eraser: false,
            touching: false,
            barrel: false,
        })
    }

    pub fn send(&mut self, pen: Pen) {
        if self
            .layout_at
            .is_none_or(|t| t.elapsed() > Duration::from_secs(2))
        {
            self.layout_at = Some(Instant::now());
            match desktop_layout(self.mode, self.monitor.as_deref()) {
                Ok(l) => self.layout = Some(l),
                Err(e) => log::debug!("pen: can't read the monitor layout: {e:#}"),
            }
        }
        let mut ev: Vec<(u16, u16, i32)> = Vec::with_capacity(10);
        let tool = |eraser: bool| {
            if eraser {
                BTN_TOOL_RUBBER
            } else {
                BTN_TOOL_PEN
            }
        };
        if self.in_range && pen.eraser != self.eraser {
            // Switching tools: the old one leaves first.
            if self.touching {
                ev.push((EV_KEY, BTN_TOUCH, 0));
                self.touching = false;
            }
            ev.push((EV_KEY, tool(self.eraser), 0));
            self.in_range = false;
        }
        if pen.action == PenAction::Leave {
            if self.touching {
                ev.push((EV_KEY, BTN_TOUCH, 0));
                ev.push((EV_ABS, ABS_PRESSURE, 0));
            }
            if self.in_range {
                ev.push((EV_KEY, tool(self.eraser), 0));
            }
            self.in_range = false;
            self.touching = false;
            self.write(&ev);
            return;
        }
        if !self.in_range {
            ev.push((EV_KEY, tool(pen.eraser), 1));
            self.in_range = true;
            self.eraser = pen.eraser;
        }
        let (x, y) = self.place(pen.x, pen.y);
        ev.push((EV_ABS, ABS_X, x));
        ev.push((EV_ABS, ABS_Y, y));
        ev.push((EV_ABS, ABS_TILT_X, pen.tilt_x.round() as i32));
        ev.push((EV_ABS, ABS_TILT_Y, pen.tilt_y.round() as i32));
        if pen.barrel != self.barrel {
            ev.push((EV_KEY, BTN_STYLUS, pen.barrel as i32));
            self.barrel = pen.barrel;
        }
        let touching = matches!(pen.action, PenAction::Down | PenAction::Move);
        let pressure = if touching {
            ((pen.pressure * PRESSURE_MAX as f32) as i32).clamp(1, PRESSURE_MAX)
        } else {
            0
        };
        ev.push((EV_ABS, ABS_PRESSURE, pressure));
        if touching != self.touching {
            ev.push((EV_KEY, BTN_TOUCH, touching as i32));
            self.touching = touching;
        }
        self.write(&ev);
    }

    /// Video position (0..1) to tablet units across the whole desktop.
    fn place(&self, nx: f32, ny: f32) -> (i32, i32) {
        let (nx, ny) = (nx as f64, ny as f64);
        let (x, y) = match self.layout {
            Some((m, d)) if d.w > 0.0 && d.h > 0.0 => {
                ((m.x + nx * m.w - d.x) / d.w, (m.y + ny * m.h - d.y) / d.h)
            }
            _ => (nx, ny),
        };
        let unit = |v: f64| ((v.clamp(0.0, 1.0)) * XY_MAX as f64).round() as i32;
        (unit(x), unit(y))
    }

    fn write(&mut self, events: &[(u16, u16, i32)]) {
        let mut buf = Vec::with_capacity((events.len() + 1) * 24);
        for &(kind, code, value) in events.iter().chain([(EV_SYN, SYN_REPORT, 0)].iter()) {
            buf.extend_from_slice(&[0u8; 16]); // time: the kernel fills it in
            buf.extend_from_slice(&kind.to_ne_bytes());
            buf.extend_from_slice(&code.to_ne_bytes());
            buf.extend_from_slice(&value.to_ne_bytes());
        }
        if let Err(e) = self.file.write_all(&buf) {
            log::debug!("pen event dropped: {e}");
        }
    }
}

impl Drop for PenDevice {
    fn drop(&mut self) {
        if self.in_range {
            let tool = if self.eraser {
                BTN_TOOL_RUBBER
            } else {
                BTN_TOOL_PEN
            };
            self.write(&[(EV_KEY, BTN_TOUCH, 0), (EV_KEY, tool, 0)]);
        }
        // SAFETY: destroying the device we created on this fd.
        unsafe {
            libc::ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY as _);
        }
    }
}

type MonitorSpec = (String, String, String, String);
type MonitorMode = (
    String,
    i32,
    i32,
    f64,
    f64,
    Vec<f64>,
    HashMap<String, OwnedValue>,
);
type Monitor = (MonitorSpec, Vec<MonitorMode>, HashMap<String, OwnedValue>);
type LogicalMonitor = (
    i32,
    i32,
    f64,
    u32,
    bool,
    Vec<MonitorSpec>,
    HashMap<String, OwnedValue>,
);
type CurrentState = (
    u32,
    Vec<Monitor>,
    Vec<LogicalMonitor>,
    HashMap<String, OwnedValue>,
);

/// The monitor shown on the tablet and the whole desktop, in stage
/// coordinates. Extend mode shows Mutter's virtual monitor.
fn desktop_layout(mode: Mode, monitor: Option<&str>) -> Result<(Rect, Rect)> {
    let conn = Connection::session()?;
    let reply = conn.call_method(
        Some("org.gnome.Mutter.DisplayConfig"),
        "/org/gnome/Mutter/DisplayConfig",
        Some("org.gnome.Mutter.DisplayConfig"),
        "GetCurrentState",
        &(),
    )?;
    let (_, monitors, logical, props): CurrentState = reply.body().deserialize()?;
    // 1 = logical layout (sizes divided by scale), 2 = physical.
    let physical = props.get("layout-mode").and_then(|v| u32::try_from(v).ok()) == Some(2);
    let current_size = |spec: &MonitorSpec| -> Option<(f64, f64)> {
        let (_, modes, _) = monitors.iter().find(|(s, _, _)| s.0 == spec.0)?;
        let m = modes.iter().find(|m| {
            m.6.get("is-current")
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false)
        })?;
        Some((m.1 as f64, m.2 as f64))
    };
    let mut rects: Vec<(Rect, &Vec<MonitorSpec>)> = Vec::new();
    for (x, y, scale, transform, _, specs, _) in &logical {
        let Some((mut w, mut h)) = specs.first().and_then(current_size) else {
            continue;
        };
        if transform % 2 == 1 {
            std::mem::swap(&mut w, &mut h);
        }
        if !physical && *scale > 0.0 {
            w /= scale;
            h /= scale;
        }
        rects.push((
            Rect {
                x: *x as f64,
                y: *y as f64,
                w,
                h,
            },
            specs,
        ));
    }
    if rects.is_empty() {
        bail!("no monitors");
    }
    let wanted = |specs: &Vec<MonitorSpec>| -> bool {
        specs.iter().any(|(connector, _, product, _)| match mode {
            Mode::Extend => connector.starts_with("Meta-") || product == "Virtual remote monitor",
            Mode::Mirror => match monitor {
                Some(m) => connector == m,
                None => false,
            },
        })
    };
    let target = rects
        .iter()
        .rev()
        .find(|(_, specs)| wanted(specs))
        .map(|(r, _)| *r)
        .or_else(|| {
            // Mirror without a choice: the first connector, as capture does.
            let first = crate::backend::list_monitors().ok()?.into_iter().next()?;
            rects
                .iter()
                .find(|(_, specs)| specs.iter().any(|s| s.0 == first))
                .map(|(r, _)| *r)
        })
        .context("monitor shown on the tablet not found")?;
    let x0 = rects.iter().map(|(r, _)| r.x).fold(f64::MAX, f64::min);
    let y0 = rects.iter().map(|(r, _)| r.y).fold(f64::MAX, f64::min);
    let x1 = rects
        .iter()
        .map(|(r, _)| r.x + r.w)
        .fold(f64::MIN, f64::max);
    let y1 = rects
        .iter()
        .map(|(r, _)| r.y + r.h)
        .fold(f64::MIN, f64::max);
    Ok((
        target,
        Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        },
    ))
}
