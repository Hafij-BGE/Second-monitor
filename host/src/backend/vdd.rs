//! Real "extend" on Windows through Virtual Display Driver (VDD,
//! github.com/VirtualDrivers/Virtual-Display-Driver, MIT, signed via
//! SignPath). Windows has no API to create a monitor; VDD adds virtual ones
//! that it reads from `C:\VirtualDisplayDriver\vdd_settings.xml`.
//!
//! The driver keeps one monitor; Extendo attaches it to the desktop while a
//! tablet is connected and detaches it afterwards. Changing the driver's
//! monitor count instead reloads the driver each time (and it treats 0 as
//! 1), and a few quick reloads crash it until Windows disables the device.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GENERIC_WRITE, POINTL};
use windows::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, EnumDisplayDevicesW, EnumDisplaySettingsW, CDS_NORESET, CDS_TYPE,
    CDS_UPDATEREGISTRY, DEVMODEW, DEVMODE_FIELD_FLAGS, DISPLAY_DEVICEW,
    DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY, DM_PELSHEIGHT,
    DM_PELSWIDTH, DM_POSITION, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_MODE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, WriteFile, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_NONE, OPEN_EXISTING,
};

const PIPE: &str = r"\\.\pipe\MTTVirtualDisplayPipe";
const SETTINGS: &str = r"C:\VirtualDisplayDriver\vdd_settings.xml";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn text(w: &[u16]) -> String {
    let len = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..len])
}

/// True when the driver is installed (its settings file exists).
pub fn installed() -> bool {
    PathBuf::from(SETTINGS).is_file()
}

/// Sends one text command to the driver.
fn command(cmd: &str) -> Result<()> {
    let name = wide(PIPE);
    unsafe {
        let pipe = CreateFileW(
            PCWSTR(name.as_ptr()),
            GENERIC_WRITE.0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
        .map_err(|e| {
            anyhow!(
                "virtual display driver isn't running ({e}); restart Windows or click \
                 \"Set up extend\" again"
            )
        })?;
        // The driver reads UTF-16LE.
        let bytes: Vec<u8> = cmd.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let result = WriteFile(pipe, Some(&bytes), None, None);
        let _ = CloseHandle(pipe);
        result.map_err(|e| anyhow!("virtual display driver: {e}"))?;
    }
    Ok(())
}

/// The driver's monitor as `(\\.\DISPLAYn, attached to the desktop)`.
fn device() -> Option<(String, bool)> {
    for i in 0.. {
        let mut dd = DISPLAY_DEVICEW {
            cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        if !unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut dd, 0) }.as_bool() {
            return None;
        }
        let what = text(&dd.DeviceString);
        if what.contains("Virtual Display") || what.contains("MttVDD") {
            let attached = dd.StateFlags.0 & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP.0 != 0;
            return Some((text(&dd.DeviceName), attached));
        }
    }
    None
}

/// `\\.\DISPLAYn` of the driver's virtual monitor while it is on.
pub fn output_name() -> Option<String> {
    device().filter(|(_, attached)| *attached).map(|(name, _)| name)
}

fn wait_for(timeout: Duration, ready: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(300));
        if ready() {
            return true;
        }
    }
    false
}

#[derive(PartialEq)]
enum Resolution {
    Present,
    Added,
    Unavailable,
}

/// Adds `width`x`height` to the driver's resolution list if missing, so the
/// virtual monitor can match the tablet. Needs write access to the settings
/// folder (the setup grants it).
fn ensure_resolution(width: u32, height: u32) -> Resolution {
    let Ok(xml) = std::fs::read_to_string(SETTINGS) else {
        return Resolution::Unavailable;
    };
    let compact: String = xml.split_whitespace().collect();
    if compact.contains(&format!("<width>{width}</width><height>{height}</height>")) {
        return Resolution::Present;
    }
    let Some(at) = xml.find("</resolutions>") else {
        return Resolution::Unavailable;
    };
    let entry = format!(
        "    <resolution>\n            <width>{width}</width>\n            <height>{height}</height>\n            <refresh_rate>60</refresh_rate>\n        </resolution>\n    "
    );
    let updated = format!("{}{}{}", &xml[..at], entry, &xml[at..]);
    match std::fs::write(SETTINGS, updated) {
        Ok(()) => {
            log::info!("added {width}x{height} to the virtual display's resolutions");
            Resolution::Added
        }
        Err(e) => {
            log::warn!("can't add {width}x{height} to {SETTINGS} ({e}); using the closest size");
            Resolution::Unavailable
        }
    }
}

/// `width`x`height` from `device`'s mode list, 60 Hz if offered.
fn find_mode(device: &str, width: u32, height: u32) -> Option<DEVMODEW> {
    let name = wide(device);
    let mut found = None;
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
        if mode.dmPelsWidth == width && mode.dmPelsHeight == height {
            if mode.dmDisplayFrequency == 60 {
                return Some(mode);
            }
            found.get_or_insert(mode);
        }
    }
    found
}

/// Stores `mode` for `device`, then applies all pending display changes.
fn apply(device: &str, mode: &DEVMODEW) -> Result<()> {
    let name = wide(device);
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

/// Sets `device` (e.g. `\\.\DISPLAY3`) to `width`x`height` if it offers it.
pub fn set_resolution(device: &str, width: u32, height: u32) {
    let Some(mut mode) = find_mode(device, width, height) else {
        log::info!("virtual display has no {width}x{height} mode; the tablet will letterbox");
        return;
    };
    mode.dmFields = DEVMODE_FIELD_FLAGS(
        mode.dmFields.0 | DM_PELSWIDTH.0 | DM_PELSHEIGHT.0 | DM_DISPLAYFREQUENCY.0,
    );
    if let Err(e) = apply(device, &mode) {
        log::debug!("{e:#}");
    }
}

/// Attaches the virtual monitor to the right of the desktop, sized for the
/// tablet, and waits until Windows shows it. `has_second_monitor` re-checks
/// the monitor list.
pub fn turn_on(width: u32, height: u32, has_second_monitor: impl Fn() -> bool) -> Result<()> {
    if ensure_resolution(width, height) == Resolution::Added {
        // The driver reads its resolutions only when it starts.
        command("RELOAD_DRIVER")?;
        std::thread::sleep(Duration::from_millis(1500));
        wait_for(Duration::from_secs(10), || device().is_some());
    }
    if device().is_none() {
        // Older Extendo versions set the driver's monitor count to 0.
        command("SETDISPLAYCOUNT 1")?;
        if !wait_for(Duration::from_secs(10), || device().is_some()) {
            bail!("the virtual display didn't appear");
        }
    }
    let (name, attached) = device().ok_or_else(|| anyhow!("the virtual display went away"))?;
    if !attached {
        let mut mode = find_mode(&name, width, height)
            .or_else(|| {
                log::info!(
                    "virtual display has no {width}x{height} mode; the tablet will letterbox"
                );
                find_mode(&name, 1920, 1080)
            })
            .ok_or_else(|| anyhow!("the virtual display offers no usable size"))?;
        let x = desktop_right_edge();
        unsafe { mode.Anonymous1.Anonymous2.dmPosition = POINTL { x, y: 0 } };
        mode.dmFields = DEVMODE_FIELD_FLAGS(
            mode.dmFields.0
                | DM_POSITION.0
                | DM_PELSWIDTH.0
                | DM_PELSHEIGHT.0
                | DM_DISPLAYFREQUENCY.0,
        );
        apply(&name, &mode)?;
    }
    if wait_for(Duration::from_secs(8), has_second_monitor) {
        Ok(())
    } else {
        Err(anyhow!("the virtual display didn't appear"))
    }
}

/// Detaches the virtual monitor from the desktop, if it is attached.
pub fn turn_off() -> Result<()> {
    let Some((name, true)) = device() else {
        return Ok(());
    };
    let mode = DEVMODEW {
        dmSize: std::mem::size_of::<DEVMODEW>() as u16,
        dmFields: DEVMODE_FIELD_FLAGS(DM_POSITION.0 | DM_PELSWIDTH.0 | DM_PELSHEIGHT.0),
        ..Default::default()
    };
    apply(&name, &mode)
}

/// Setup script. The winget package is only the portable "VDD Control"
/// zip: it unpacks the signed driver but doesn't install it. So after winget
/// the script re-runs itself elevated with `-Package` to create the settings
/// folder and add the `Root\MttVDD` device with the bundled devcon.
const SETUP_SCRIPT: &str = r#"param([string]$Package)
$ErrorActionPreference = 'Stop'
$releases = 'https://github.com/VirtualDrivers/Virtual-Display-Driver/releases'
if ($Package) {
    try {
        $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'ARM64' } else { 'x86' }
        $driver = Join-Path $Package "SignedDrivers\$arch\VDD"
        $dir = 'C:\VirtualDisplayDriver'
        New-Item -ItemType Directory -Force $dir | Out-Null
        if (-not (Test-Path "$dir\vdd_settings.xml")) {
            Copy-Item "$driver\vdd_settings.xml" $dir
        }
        # Extendo runs unelevated and adds the tablet's resolution here.
        icacls $dir /grant '*S-1-5-32-545:(OI)(CI)M' | Out-Null
        $device = Get-PnpDevice -ErrorAction SilentlyContinue |
            Where-Object { $_.HardwareID -contains 'Root\MttVDD' }
        if ($device) {
            pnputil /add-driver "$driver\MttVDD.inf" /install
        } else {
            & (Join-Path $Package 'Dependencies\devcon.exe') install "$driver\MttVDD.inf" 'Root\MttVDD'
        }
        if ($LASTEXITCODE -gt 1) { throw "driver setup failed (exit code $LASTEXITCODE)" }
        Write-Host ''
        if ($LASTEXITCODE -eq 1) { Write-Host 'Done. Restart Windows, then connect your tablet.' }
        else { Write-Host 'Done. Reconnect your tablet in Extendo.' }
    } catch {
        Write-Host ''
        Write-Host "Failed: $_"
        Write-Host "Install it by hand from $releases"
    }
    Read-Host 'Press Enter to close'
    exit
}
try {
    Write-Host 'Installing Virtual Display Driver (MIT, signed) for Extendo...'
    winget install --id=VirtualDrivers.Virtual-Display-Driver -e --accept-source-agreements --accept-package-agreements
    $pkg = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages", "$env:ProgramFiles\WinGet\Packages" `
        -Directory -Filter 'VirtualDrivers.Virtual-Display-Driver_*' -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if (-not $pkg) { throw 'winget did not download the driver' }
    Write-Host 'Windows will now ask for admin approval to add the driver.'
    Start-Process powershell -Verb RunAs -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass',
        '-File', "`"$PSCommandPath`"", '-Package', "`"$($pkg.FullName)`""
} catch {
    Write-Host ''
    Write-Host "Failed: $_"
    Write-Host "Install it by hand from $releases"
    Read-Host 'Press Enter to close'
}
"#;

/// Opens a console that downloads the driver with winget and installs it
/// (asks for admin).
pub fn install() {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let path = std::env::temp_dir().join("extendo-setup-extend.ps1");
    if let Err(e) = std::fs::write(&path, SETUP_SCRIPT) {
        log::warn!("cannot write {}: {e}", path.display());
        return;
    }
    if let Err(e) = std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&path)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
    {
        log::warn!("cannot start the driver setup: {e}");
    }
}
