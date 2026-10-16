//! Real "extend" on Windows through Parsec Virtual Display Driver (signed by
//! Parsec), driven the way github.com/nomi-san/parsec-vdd (core/parsec-vdd.h)
//! documents it. Displays are plugged in and out with IOCTLs, without
//! reloading the driver, and the driver unplugs every display about a second
//! after the keep-alive pings stop, so a crashed or killed Extendo leaves no
//! screen behind.
//!
//! The driver offers a fixed list of sizes plus up to five custom ones from
//! `HKLM\SOFTWARE\Parsec\vdd\0..4` (`w`, `h`, `hz`); the setup adds the
//! tablet's size there (needs admin).

use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_List_SizeW,
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CR_SUCCESS,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, POINTL};
use windows::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, EnumDisplayDevicesW, EnumDisplaySettingsW, CDS_NORESET, CDS_TYPE,
    CDS_UPDATEREGISTRY, DEVMODEW, DEVMODE_FIELD_FLAGS, DISPLAY_DEVICEW,
    DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY, DM_PELSHEIGHT,
    DM_PELSWIDTH, DM_POSITION, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_MODE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_NO_BUFFERING, FILE_FLAG_OVERLAPPED,
    FILE_FLAG_WRITE_THROUGH, FILE_SHARE_MODE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Threading::CreateEventW;
use windows::Win32::System::IO::{
    CancelIoEx, DeviceIoControl, GetOverlappedResult, GetOverlappedResultEx, OVERLAPPED,
};
use windows::Win32::UI::WindowsAndMessaging::EDD_GET_DEVICE_INTERFACE_NAME;

/// Device interface of the driver's adapter.
const ADAPTER: GUID = GUID::from_u128(0x00b41627_04c4_429e_a26e_0265cf50c8fa);
const IOCTL_ADD: u32 = 0x0022_e004;
const IOCTL_REMOVE: u32 = 0x0022_a008;
const IOCTL_UPDATE: u32 = 0x0022_a00c;
/// The driver drops all displays ~1 s after the last ping.
const PING_EVERY: Duration = Duration::from_millis(100);
/// Monitor ID the driver's displays report.
const MONITOR_ID: &str = "PSCCDD0";

fn text(w: &[u16]) -> String {
    let len = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..len])
}

/// Path of the driver's device interface, NUL-terminated, while the driver
/// is installed and running.
fn interface_path() -> Option<Vec<u16>> {
    let mut len = 0u32;
    let rc = unsafe {
        CM_Get_Device_Interface_List_SizeW(
            &mut len,
            &ADAPTER,
            PCWSTR::null(),
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    };
    if rc != CR_SUCCESS || len <= 1 {
        return None;
    }
    let mut list = vec![0u16; len as usize];
    let rc = unsafe {
        CM_Get_Device_Interface_ListW(
            &ADAPTER,
            PCWSTR::null(),
            &mut list,
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    };
    if rc != CR_SUCCESS {
        return None;
    }
    // NUL-separated paths; take the first.
    let end = list.iter().position(|&c| c == 0)?;
    if end == 0 {
        return None;
    }
    list.truncate(end + 1);
    Some(list)
}

/// True when the driver is installed and running.
pub fn installed() -> bool {
    interface_path().is_some()
}

/// Open handle to the driver.
struct Device(HANDLE);

// A plain kernel handle, usable from any thread.
unsafe impl Send for Device {}
unsafe impl Sync for Device {}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

impl Device {
    fn open() -> Result<Self> {
        let path = interface_path()
            .ok_or_else(|| anyhow!("Parsec Virtual Display Driver isn't installed or running"))?;
        let handle = unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0),
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL
                    | FILE_FLAG_NO_BUFFERING
                    | FILE_FLAG_OVERLAPPED
                    | FILE_FLAG_WRITE_THROUGH,
                None,
            )
        }
        .map_err(|e| anyhow!("cannot open Parsec Virtual Display Driver: {e}"))?;
        Ok(Self(handle))
    }

    /// Sends one request and returns the driver's 4-byte answer. Like the
    /// reference code, it waits on the OVERLAPPED whatever DeviceIoControl
    /// returns (the driver may report failure yet still complete it), and
    /// cancels before the buffers go out of scope.
    fn ioctl(&self, code: u32, input: [u8; 32], timeout_ms: u32) -> Option<u32> {
        unsafe {
            let event = CreateEventW(None, true, false, PCWSTR::null()).ok()?;
            let mut overlapped = OVERLAPPED {
                hEvent: event,
                ..Default::default()
            };
            let ov: *mut OVERLAPPED = &mut overlapped;
            let mut out = 0u32;
            let mut transferred = 0u32;
            let _ = DeviceIoControl(
                self.0,
                code,
                Some(input.as_ptr().cast()),
                input.len() as u32,
                Some((&mut out as *mut u32).cast()),
                4,
                None,
                Some(ov),
            );
            let done = GetOverlappedResultEx(self.0, ov, &mut transferred, timeout_ms, false).is_ok();
            if !done {
                let _ = CancelIoEx(self.0, Some(ov as *const _));
                let _ = GetOverlappedResult(self.0, ov, &mut transferred, true);
            }
            let _ = CloseHandle(event);
            done.then_some(out)
        }
    }

    fn ping(&self) {
        self.ioctl(IOCTL_UPDATE, [0; 32], 1000);
    }
}

/// The open driver, shared by every display Extendo added. A thread pings it
/// while any of them exists; the last one dropped closes it.
struct Session {
    device: Device,
}

static SESSION: Mutex<Option<Weak<Session>>> = Mutex::new(None);

fn session() -> Result<Arc<Session>> {
    let mut slot = SESSION.lock().unwrap();
    if let Some(session) = slot.as_ref().and_then(Weak::upgrade) {
        return Ok(session);
    }
    let session = Arc::new(Session {
        device: Device::open()?,
    });
    let weak = Arc::downgrade(&session);
    thread::Builder::new()
        .name("parsec-vdd".into())
        .spawn(move || {
            while let Some(session) = weak.upgrade() {
                session.device.ping();
                drop(session);
                thread::sleep(PING_EVERY);
            }
        })?;
    *slot = Some(Arc::downgrade(&session));
    Ok(session)
}

/// Size last asked for that the driver doesn't offer, so the GUI can offer
/// to add it. `None` once a display got its exact size.
static MISSING_SIZE: Mutex<Option<(u32, u32)>> = Mutex::new(None);

pub fn missing_size() -> Option<(u32, u32)> {
    *MISSING_SIZE.lock().unwrap()
}

/// A virtual monitor; unplugged when dropped.
pub struct Display {
    session: Arc<Session>,
    index: u32,
    name: String,
}

impl Display {
    /// `\\.\DISPLAYn`.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        // 16-bit big-endian index at the start of the buffer.
        let mut input = [0u8; 32];
        input[0] = (self.index >> 8) as u8;
        input[1] = self.index as u8;
        let removed = self.session.device.ioctl(IOCTL_REMOVE, input, 1000);
        self.session.device.ping();
        match removed {
            Some(_) => log::info!("virtual display removed"),
            None => log::warn!("Parsec Virtual Display Driver didn't confirm removing the display"),
        }
    }
}

/// The driver's monitors: (`\\.\DISPLAYn`, interface path, attached to the
/// desktop).
fn monitors() -> Vec<(String, String, bool)> {
    let mut found = Vec::new();
    for i in 0.. {
        let mut source = DISPLAY_DEVICEW {
            cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        if !unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut source, 0) }.as_bool() {
            break;
        }
        for j in 0.. {
            let mut monitor = DISPLAY_DEVICEW {
                cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            let more = unsafe {
                EnumDisplayDevicesW(
                    PCWSTR(source.DeviceName.as_ptr()),
                    j,
                    &mut monitor,
                    EDD_GET_DEVICE_INTERFACE_NAME,
                )
            };
            if !more.as_bool() {
                break;
            }
            let path = text(&monitor.DeviceID).to_ascii_uppercase();
            if path.contains(MONITOR_ID) {
                let attached = source.StateFlags.0 & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP.0 != 0;
                found.push((text(&source.DeviceName), path, attached));
            }
        }
    }
    found
}

/// The monitor for driver index `index`: its interface path carries
/// `UID<256 + index>`. Falls back to any of the driver's monitors that
/// wasn't there `before`.
fn find_monitor(index: u32, before: &[(String, String, bool)]) -> Option<(String, bool)> {
    let now = monitors();
    let uid = format!("UID{}#", 256 + index);
    now.iter()
        .find(|(_, path, _)| path.contains(&uid))
        .or_else(|| {
            now.iter()
                .find(|(_, path, _)| !before.iter().any(|(_, old, _)| old == path))
        })
        .map(|(name, _, attached)| (name.clone(), *attached))
}

fn poll<T>(timeout: Duration, mut check: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(v) = check() {
            return Some(v);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn modes(device: &str) -> Vec<DEVMODEW> {
    let name: Vec<u16> = device.encode_utf16().chain(std::iter::once(0)).collect();
    let mut all = Vec::new();
    for i in 0.. {
        let mut mode = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        if !unsafe {
            EnumDisplaySettingsW(PCWSTR(name.as_ptr()), ENUM_DISPLAY_SETTINGS_MODE(i), &mut mode)
        }
        .as_bool()
        {
            break;
        }
        all.push(mode);
    }
    all
}

/// `width`x`height` at 60 Hz if offered, else at any rate.
fn exact_mode(modes: &[DEVMODEW], width: u32, height: u32) -> Option<DEVMODEW> {
    let same = |m: &&DEVMODEW| m.dmPelsWidth == width && m.dmPelsHeight == height;
    modes
        .iter()
        .filter(same)
        .find(|m| m.dmDisplayFrequency == 60)
        .or_else(|| modes.iter().find(same))
        .copied()
}

/// Largest mode that fits in `width`x`height`, 60 Hz preferred.
fn fitting_mode(modes: &[DEVMODEW], width: u32, height: u32) -> Option<DEVMODEW> {
    modes
        .iter()
        .filter(|m| m.dmPelsWidth <= width && m.dmPelsHeight <= height)
        .max_by_key(|m| {
            (
                m.dmPelsWidth as u64 * m.dmPelsHeight as u64,
                m.dmDisplayFrequency == 60,
            )
        })
        .copied()
}

/// Stores `mode` for `device`, then applies all pending display changes.
fn apply(device: &str, mode: &DEVMODEW) -> Result<()> {
    let name: Vec<u16> = device.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let rc = ChangeDisplaySettingsExW(
            PCWSTR(name.as_ptr()),
            Some(mode),
            None,
            CDS_TYPE(CDS_UPDATEREGISTRY.0 | CDS_NORESET.0),
            None,
        );
        if rc != DISP_CHANGE_SUCCESSFUL {
            bail!("changing {device} failed ({})", rc.0);
        }
        let rc = ChangeDisplaySettingsExW(PCWSTR::null(), None, None, CDS_TYPE(0), None);
        if rc != DISP_CHANGE_SUCCESSFUL {
            bail!("applying display changes failed ({})", rc.0);
        }
    }
    Ok(())
}

/// Right edge of the desktop's attached monitors, in pixels.
fn desktop_right_edge() -> i32 {
    let mut right = 0;
    for i in 0.. {
        let mut dd = DISPLAY_DEVICEW {
            cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        if !unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut dd, 0) }.as_bool() {
            break;
        }
        if dd.StateFlags.0 & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP.0 == 0 {
            continue;
        }
        let mut mode = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        let current = unsafe {
            EnumDisplaySettingsW(PCWSTR(dd.DeviceName.as_ptr()), ENUM_CURRENT_SETTINGS, &mut mode)
        };
        if current.as_bool() {
            let x = unsafe { mode.Anonymous1.Anonymous2.dmPosition.x };
            right = right.max(x + mode.dmPelsWidth as i32);
        }
    }
    right
}

/// Gives `device` the tablet's size (or the largest that fits) and, if
/// Windows didn't already, puts it on the desktop right of the others.
fn size_and_attach(device: &str, attached: bool, width: u32, height: u32) -> Result<()> {
    let all = modes(device);
    let exact = exact_mode(&all, width, height);
    *MISSING_SIZE.lock().unwrap() = if exact.is_some() {
        None
    } else {
        Some((width, height))
    };
    let Some(mut mode) = exact.or_else(|| {
        log::info!(
            "the virtual display has no {width}x{height} mode, so the tablet will letterbox; \
             click \"Add {width}×{height}\" in Extendo to add it"
        );
        fitting_mode(&all, width, height)
    }) else {
        if attached {
            return Ok(());
        }
        bail!("the virtual display offers no usable size");
    };
    mode.dmFields = DEVMODE_FIELD_FLAGS(
        mode.dmFields.0 | DM_PELSWIDTH.0 | DM_PELSHEIGHT.0 | DM_DISPLAYFREQUENCY.0,
    );
    if !attached {
        let x = desktop_right_edge();
        mode.Anonymous1.Anonymous2.dmPosition = POINTL { x, y: 0 };
        mode.dmFields = DEVMODE_FIELD_FLAGS(mode.dmFields.0 | DM_POSITION.0);
    }
    apply(device, &mode)
}

/// Plugs in a virtual monitor for a `width`x`height` tablet, sizes it and
/// waits until `visible` (the capture backend's monitor list) shows it.
pub fn add(width: u32, height: u32, visible: impl Fn(&str) -> bool) -> Result<Display> {
    let session = session()?;
    let before = monitors();
    let index = session.device.ioctl(IOCTL_ADD, [0; 32], 5000);
    session.device.ping();
    let index = match index {
        Some(i) if i < 16 => i,
        _ => bail!("Parsec Virtual Display Driver didn't add a display"),
    };
    // From here on, dropping `display` unplugs it again.
    let mut display = Display {
        session,
        index,
        name: String::new(),
    };
    let (name, _) = poll(Duration::from_secs(6), || find_monitor(index, &before))
        .ok_or_else(|| anyhow!("the virtual display didn't show up in Windows"))?;
    // Windows usually extends the desktop onto a new monitor by itself.
    let attached = poll(Duration::from_millis(1500), || {
        find_monitor(index, &before).filter(|(_, attached)| *attached)
    })
    .is_some();
    log::info!("virtual display is {name}");
    display.name = name.clone();
    if let Err(e) = size_and_attach(&name, attached, width, height) {
        log::warn!("{e:#}");
    }
    if poll(Duration::from_secs(5), || visible(&name).then_some(())).is_none() {
        // e.g. Windows was set to duplicate the screens.
        super::vdd::extend_topology();
        poll(Duration::from_secs(5), || visible(&name).then_some(()))
            .ok_or_else(|| anyhow!("Windows didn't put the virtual display on the desktop"))?;
    }
    Ok(display)
}

/// Setup script, run elevated (it re-launches itself with RunAs). Writes the
/// tablet's size into the driver's custom sizes, then downloads Parsec's
/// signed driver setup, checks it is the expected file and runs it silently.
/// With the driver already installed it only adds the size and restarts the
/// adapter, which reads its sizes when it starts.
const SETUP_SCRIPT: &str = r#"param([int]$Width = 0, [int]$Height = 0)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$url = 'https://builds.parsec.app/vdd/parsec-vdd-0.41.0.0.exe'
# MD5 Parsec's download server reports for this file (its ETag).
$md5 = 'F376960263B039EA1BBAC40CE1567ABA'
$hwid = 'Root\Parsec\VDA'
$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) {
    try {
        Write-Host 'Windows will now ask for admin approval to set up the virtual display.'
        Start-Process powershell -Verb RunAs -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass',
            '-File', "`"$PSCommandPath`"", '-Width', $Width, '-Height', $Height
    } catch {
        Write-Host ''
        Write-Host "Failed: $_"
        Read-Host 'Press Enter to close'
    }
    exit
}
try {
    $added = $false
    if ($Width -gt 0 -and $Height -gt 0) {
        $root = 'HKLM:\SOFTWARE\Parsec\vdd'
        $present = $false
        foreach ($i in 0..4) {
            $p = Get-ItemProperty "$root\$i" -ErrorAction SilentlyContinue
            if ($p -and $p.w -eq $Width -and $p.h -eq $Height) { $present = $true }
        }
        if (-not $present) {
            $slot = 0..4 | Where-Object { -not (Test-Path "$root\$_") } | Select-Object -First 1
            if ($null -eq $slot) { $slot = 4 }
            $key = "$root\$slot"
            if (-not (Test-Path $key)) { New-Item -Path $key -Force | Out-Null }
            New-ItemProperty -Path $key -Name 'w' -Value $Width -PropertyType DWord -Force | Out-Null
            New-ItemProperty -Path $key -Name 'h' -Value $Height -PropertyType DWord -Force | Out-Null
            New-ItemProperty -Path $key -Name 'hz' -Value 60 -PropertyType DWord -Force | Out-Null
            Write-Host "Added ${Width}x${Height} at 60 Hz to the virtual display's sizes."
            $added = $true
        }
    }
    $device = Get-PnpDevice -ErrorAction SilentlyContinue |
        Where-Object { $_.HardwareID -contains $hwid } | Select-Object -First 1
    if (-not $device -or $device.Status -ne 'OK') {
        Write-Host 'Downloading Parsec Virtual Display Driver 0.41 from Parsec...'
        # Only admins can change files in this folder.
        $dir = Join-Path $env:SystemRoot 'Temp\extendo-parsec-vdd'
        New-Item -ItemType Directory -Force $dir | Out-Null
        $exe = Join-Path $dir 'parsec-vdd-0.41.0.0.exe'
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest $url -OutFile $exe -UseBasicParsing
        if ((Get-FileHash $exe -Algorithm MD5).Hash -ne $md5) { throw 'the download is not the expected file' }
        $sig = Get-AuthenticodeSignature $exe
        if ($sig.Status -ne 'Valid') { throw "the download is not validly signed ($($sig.Status))" }
        Write-Host "Signed by: $($sig.SignerCertificate.Subject)"
        Write-Host 'Installing...'
        $setup = Start-Process $exe -ArgumentList '/S' -Wait -PassThru
        Remove-Item $exe -ErrorAction SilentlyContinue
        if ($setup.ExitCode -ne 0) { throw "the driver setup failed (exit code $($setup.ExitCode))" }
        Write-Host ''
        Write-Host 'Done. Reconnect your tablet in Extendo.'
    } elseif ($added) {
        pnputil /restart-device "$($device.InstanceId)" | Out-Null
        Write-Host ''
        if ($LASTEXITCODE -eq 0) { Write-Host 'Done. Reconnect your tablet in Extendo.' }
        else { Write-Host 'Done. Restart Windows to use the new size.' }
    } else {
        Write-Host ''
        Write-Host 'Already set up. Reconnect your tablet in Extendo.'
    }
} catch {
    Write-Host ''
    Write-Host "Failed: $_"
    Write-Host 'You can install the driver by hand from https://github.com/nomi-san/parsec-vdd'
}
Read-Host 'Press Enter to close'
"#;

/// Opens a console that installs the driver and adds `size` (the tablet's)
/// to its sizes. Asks for admin.
pub fn install(size: Option<(u32, u32)>) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let path = std::env::temp_dir().join("extendo-setup-extend.ps1");
    if let Err(e) = std::fs::write(&path, SETUP_SCRIPT) {
        log::warn!("cannot write {}: {e}", path.display());
        return;
    }
    let (width, height) = size.unwrap_or((0, 0));
    if let Err(e) = std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&path)
        .arg("-Width")
        .arg(width.to_string())
        .arg("-Height")
        .arg(height.to_string())
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
    {
        log::warn!("cannot start the driver setup: {e}");
    }
}
