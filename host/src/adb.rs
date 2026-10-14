//! USB mode: keeps `adb reverse tcp:PORT tcp:PORT` set up for every attached
//! device, so the app can reach the host at 127.0.0.1 over the USB cable.

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::state::Shared;

pub fn spawn(shared: Arc<Shared>, port: u16) {
    let Some(adb) = find_adb() else {
        log::warn!(
            "adb not found: USB mode disabled. Install Android platform-tools or set SMON_ADB=/path/to/adb"
        );
        return;
    };
    log::info!("USB mode: using {}", adb.display());
    shared.update_status(|s| s.adb_path = Some(adb.clone()));
    thread::Builder::new()
        .name("adb".into())
        .spawn(move || {
            let mut reversed: HashSet<String> = HashSet::new();
            loop {
                if !shared.settings().usb {
                    reversed.clear();
                    shared.update_status(|s| s.usb_devices.clear());
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
                let devices = list_devices(&adb);
                {
                    let mut status = shared.status();
                    if status.usb_devices != devices {
                        status.usb_devices = devices.clone();
                        drop(status);
                        shared.changed();
                    }
                }
                reversed.retain(|s| devices.contains(s));
                for serial in devices {
                    if reversed.contains(&serial) {
                        continue;
                    }
                    let spec = format!("tcp:{port}");
                    let ok = command(&adb)
                        .args(["-s", &serial, "reverse", &spec, &spec])
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false);
                    if ok {
                        log::info!("USB device {serial} ready (adb reverse {spec})");
                        reversed.insert(serial);
                    }
                }
                thread::sleep(Duration::from_secs(2));
            }
        })
        .expect("spawn adb thread");
}

fn list_devices(adb: &PathBuf) -> Vec<String> {
    let Ok(out) = command(adb).arg("devices").output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut devices = Vec::new();
    for line in text.lines().skip(1) {
        let mut parts = line.split_whitespace();
        if let (Some(serial), Some(state)) = (parts.next(), parts.next()) {
            match state {
                "device" => devices.push(serial.to_string()),
                "unauthorized" => {
                    log::warn!(
                        "USB device {serial}: accept the 'Allow USB debugging' prompt on the phone"
                    )
                }
                _ => {}
            }
        }
    }
    devices
}

fn command(adb: &PathBuf) -> Command {
    let mut cmd = Command::new(adb);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn find_adb() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "adb.exe" } else { "adb" };
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("SMON_ADB") {
        candidates.push(p.into());
    }
    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            candidates.push(dir.join(exe));
            candidates.push(dir.join("platform-tools").join(exe));
        }
    }
    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(sdk) = std::env::var_os(var) {
            candidates.push(PathBuf::from(sdk).join("platform-tools").join(exe));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(local)
                .join("Android/Sdk/platform-tools")
                .join(exe),
        );
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|d| d.join(exe)));
    }
    candidates.into_iter().find(|p| p.is_file())
}
