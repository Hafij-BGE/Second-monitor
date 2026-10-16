//! Real "extend" on Windows through Virtual Display Driver (VDD,
//! github.com/VirtualDrivers/Virtual-Display-Driver, MIT, signed via
//! SignPath). Windows has no API to create a monitor; VDD adds virtual ones
//! that it reads from `C:\VirtualDisplayDriver\vdd_settings.xml` and that
//! can be switched on and off through its named pipe.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use windows::core::PCWSTR;
use windows::Win32::Devices::Display::{SetDisplayConfig, SDC_APPLY, SDC_TOPOLOGY_EXTEND};
use windows::Win32::Foundation::{CloseHandle, GENERIC_WRITE};
use windows::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, EnumDisplayDevicesW, EnumDisplaySettingsW, CDS_UPDATEREGISTRY,
    DEVMODEW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISP_CHANGE_SUCCESSFUL,
    ENUM_DISPLAY_SETTINGS_MODE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, WriteFile, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_NONE, OPEN_EXISTING,
};

const PIPE: &str = r"\\.\pipe\MTTVirtualDisplayPipe";
const SETTINGS: &str = r"C:\VirtualDisplayDriver\vdd_settings.xml";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
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
        .map_err(|e| anyhow!("virtual display driver isn't running: {e}"))?;
        // The driver reads UTF-16LE.
        let bytes: Vec<u8> = cmd.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let result = WriteFile(pipe, Some(&bytes), None, None);
        let _ = CloseHandle(pipe);
        result.map_err(|e| anyhow!("virtual display driver: {e}"))?;
    }
    Ok(())
}

/// Number of virtual monitors the driver is set to show.
pub fn display_count() -> u32 {
    std::fs::read_to_string(SETTINGS)
        .ok()
        .and_then(|xml| {
            let start = xml.find("<count>")? + "<count>".len();
            let end = xml[start..].find("</count>")? + start;
            xml[start..end].trim().parse().ok()
        })
        .unwrap_or(0)
}

/// `\\.\DISPLAYn` of the driver's virtual monitor while it is on.
pub fn output_name() -> Option<String> {
    let mut i = 0;
    loop {
        let mut dd = DISPLAY_DEVICEW {
            cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        if !unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut dd, 0) }.as_bool() {
            return None;
        }
        i += 1;
        let text = |w: &[u16]| {
            let len = w.iter().position(|&c| c == 0).unwrap_or(w.len());
            String::from_utf16_lossy(&w[..len])
        };
        let what = text(&dd.DeviceString);
        let attached = dd.StateFlags.0 & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP.0 != 0;
        if attached && (what.contains("Virtual Display") || what.contains("MttVDD")) {
            return Some(text(&dd.DeviceName));
        }
    }
}

pub fn set_display_count(n: u32) -> Result<()> {
    command(&format!("SETDISPLAYCOUNT {n}"))
}

/// Adds `width`x`height` to the driver's resolution list if missing, so the
/// virtual monitor can match the tablet. Needs write access to the settings
/// folder; returns false when that isn't possible.
fn ensure_resolution(width: u32, height: u32) -> bool {
    let Ok(xml) = std::fs::read_to_string(SETTINGS) else {
        return false;
    };
    let compact: String = xml.split_whitespace().collect();
    if compact.contains(&format!("<width>{width}</width><height>{height}</height>")) {
        return true;
    }
    let Some(at) = xml.find("</resolutions>") else {
        return false;
    };
    let entry = format!(
        "    <resolution>\n            <width>{width}</width>\n            <height>{height}</height>\n            <refresh_rate>60</refresh_rate>\n        </resolution>\n    "
    );
    let updated = format!("{}{}{}", &xml[..at], entry, &xml[at..]);
    match std::fs::write(SETTINGS, updated) {
        Ok(()) => {
            log::info!("added {width}x{height} to the virtual display's resolutions");
            true
        }
        Err(e) => {
            log::warn!("can't add {width}x{height} to {SETTINGS} ({e}); using the closest size");
            false
        }
    }
}

/// Makes Windows extend the desktop across all displays (not duplicate).
fn extend_topology() {
    let rc = unsafe { SetDisplayConfig(None, None, SDC_TOPOLOGY_EXTEND | SDC_APPLY) };
    if rc != 0 {
        log::debug!("SetDisplayConfig(extend) returned {rc}");
    }
}

/// Sets `device` (e.g. `\\.\DISPLAY3`) to `width`x`height` if it offers it.
pub fn set_resolution(device: &str, width: u32, height: u32) {
    let name = wide(device);
    unsafe {
        let mut mode = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        let mut i = 0;
        let mut found = false;
        while EnumDisplaySettingsW(
            PCWSTR(name.as_ptr()),
            ENUM_DISPLAY_SETTINGS_MODE(i),
            &mut mode,
        )
        .as_bool()
        {
            if mode.dmPelsWidth == width && mode.dmPelsHeight == height {
                found = true;
                break;
            }
            i += 1;
        }
        if !found {
            log::info!("virtual display has no {width}x{height} mode; the tablet will letterbox");
            return;
        }
        let rc = ChangeDisplaySettingsExW(
            PCWSTR(name.as_ptr()),
            Some(&mode),
            None,
            CDS_UPDATEREGISTRY,
            None,
        );
        if rc != DISP_CHANGE_SUCCESSFUL {
            log::debug!("ChangeDisplaySettingsExW returned {}", rc.0);
        }
    }
}

/// Turns on one virtual monitor sized for the tablet and waits until
/// Windows shows it. `has_second_monitor` re-checks the monitor list.
pub fn turn_on(width: u32, height: u32, has_second_monitor: impl Fn() -> bool) -> Result<()> {
    let reload = ensure_resolution(width, height);
    set_display_count(1)?;
    if reload {
        let _ = command("RELOAD_DRIVER");
    }
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(400));
        if has_second_monitor() {
            extend_topology();
            return Ok(());
        }
    }
    Err(anyhow!("the virtual display didn't appear"))
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
