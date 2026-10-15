//! USB mode: keeps `adb reverse tcp:PORT tcp:PORT` set up for every attached
//! device, so the app can reach the host at 127.0.0.1 over the USB cable.

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::state::{AdbDevice, Shared};

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
                let serials: Vec<String> = devices.into_iter().map(|d| d.serial).collect();
                reversed.retain(|s| serials.contains(s));
                // If the adb server restarted (another tool can do that), the
                // tunnels are gone: check and set them up again.
                let spec = format!("tcp:{port}");
                reversed.retain(|s| {
                    run(&adb, &["-s", s, "reverse", "--list"])
                        .map(|list| list.contains(&spec))
                        .unwrap_or(false)
                });
                for serial in serials {
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

/// Devices adb can talk to, over USB or WiFi (`adb connect`).
pub fn list_devices(adb: &PathBuf) -> Vec<AdbDevice> {
    let Ok(out) = command(adb).args(["devices", "-l"]).output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut devices = Vec::new();
    for line in text.lines().skip(1) {
        let mut parts = line.split_whitespace();
        if let (Some(serial), Some(state)) = (parts.next(), parts.next()) {
            match state {
                "device" => {
                    let model = parts
                        .find_map(|p| p.strip_prefix("model:"))
                        .map(|m| m.replace('_', " "))
                        .unwrap_or_else(|| serial.to_string());
                    // `adb connect` devices are named ip:port; wireless
                    // debugging ones contain "_adb-tls-connect".
                    let wifi = serial.contains(':') || serial.contains("adb-tls");
                    let wifi_debugging = wifi || tcp_port_open(adb, serial);
                    devices.push(AdbDevice {
                        serial: serial.to_string(),
                        model,
                        wifi,
                        wifi_debugging,
                    });
                }
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

/// True when `adb tcpip` left adbd listening on the network.
fn tcp_port_open(adb: &PathBuf, serial: &str) -> bool {
    run(
        adb,
        &["-s", serial, "shell", "getprop", "service.adb.tcp.port"],
    )
    .map(|p| p.trim().parse::<i32>().is_ok_and(|n| n > 0))
    .unwrap_or(false)
}

/// Runs adb and returns stdout, or stderr as the error.
pub fn run(adb: &PathBuf, args: &[&str]) -> Result<String, String> {
    let out = command(adb)
        .args(args)
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() {
        Ok(stdout)
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { stdout } else { err })
    }
}

pub fn command(adb: &PathBuf) -> Command {
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

pub fn find_adb() -> Option<PathBuf> {
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
