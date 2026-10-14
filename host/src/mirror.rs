//! Extendo Mirror: shows and controls a phone on the PC using scrcpy, over a
//! USB cable or WiFi (adb over TCP).

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use crate::adb;
use crate::state::{AdbDevice, MirrorSettings, Shared};

/// Port used for adb over WiFi.
const ADB_WIFI_PORT: u16 = 5555;

fn sessions() -> &'static Mutex<HashMap<String, Child>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, Child>>> = OnceLock::new();
    SESSIONS.get_or_init(Default::default)
}

/// Finds a scrcpy that actually runs (a stale build can be missing its
/// libraries) and records it in the status. Runs in the background.
pub fn detect(shared: Arc<Shared>) {
    thread::spawn(move || {
        let found = find_scrcpy();
        match &found {
            Some((path, version)) => log::info!("Extendo Mirror: {version} at {}", path.display()),
            None => log::warn!(
                "Extendo Mirror unavailable: scrcpy not found (Linux: sudo apt install scrcpy)"
            ),
        }
        shared.update_status(|s| s.scrcpy = found);
    });
}

fn find_scrcpy() -> Option<(PathBuf, String)> {
    let exe = if cfg!(windows) {
        "scrcpy.exe"
    } else {
        "scrcpy"
    };
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("SMON_SCRCPY") {
        candidates.push(p.into());
    }
    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            candidates.push(dir.join(exe));
            candidates.push(dir.join("scrcpy").join(exe));
        }
    }
    // Distribution packages first: hand-built copies in /usr/local often break
    // after ffmpeg upgrades.
    candidates.push(PathBuf::from("/usr/bin").join(exe));
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|d| d.join(exe)));
    }
    candidates.dedup();
    candidates
        .into_iter()
        .filter(|p| p.is_file())
        .find_map(|p| {
            let out = quiet(Command::new(&p)).arg("--version").output().ok()?;
            if !out.status.success() {
                log::debug!("ignoring broken scrcpy at {}", p.display());
                return None;
            }
            let version = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or("scrcpy")
                // "scrcpy 3.3.4 <https://...>" -> "scrcpy 3.3.4"
                .split('<')
                .next()
                .unwrap_or("scrcpy")
                .trim()
                .to_string();
            Some((p, version))
        })
}

fn quiet(mut cmd: Command) -> Command {
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn args_for(device: &AdbDevice, m: &MirrorSettings, record_dir: &std::path::Path) -> Vec<String> {
    let mut a = vec![
        "-s".to_string(),
        device.serial.clone(),
        format!("--window-title=Extendo Mirror · {}", device.model),
        format!("--video-bit-rate={}M", m.bitrate_mbps.max(1)),
        format!("--max-fps={}", m.max_fps.max(1)),
        // Alt or Super, so shortcuts don't clash with apps on the phone.
        "--shortcut-mod=lalt,lsuper".into(),
    ];
    if m.max_size > 0 {
        a.push(format!("--max-size={}", m.max_size));
    }
    if !m.audio {
        a.push("--no-audio".into());
    }
    if m.screen_off {
        a.push("--turn-screen-off".into());
    }
    if m.stay_awake {
        a.push("--stay-awake".into());
    }
    if m.show_touches {
        a.push("--show-touches".into());
    }
    if m.always_on_top {
        a.push("--always-on-top".into());
    }
    if m.fullscreen {
        a.push("--fullscreen".into());
    }
    if m.physical_keyboard {
        a.push("--keyboard=uhid".into());
    }
    if m.power_off_on_close {
        a.push("--power-off-on-close".into());
    }
    if m.camera {
        a.push("--video-source=camera".into());
        // Camera mode can't show touches or turn the screen off.
        a.retain(|x| x != "--turn-screen-off" && x != "--show-touches" && x != "--stay-awake");
    }
    if m.record {
        let _ = std::fs::create_dir_all(record_dir);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let name = format!("Extendo Mirror {} {stamp}.mp4", device.model);
        a.push(format!("--record={}", record_dir.join(name).display()));
    }
    a
}

/// Opens a mirror window for `device`.
pub fn start(shared: Arc<Shared>, device: AdbDevice) {
    let Some((scrcpy, _)) = shared.status().scrcpy.clone() else {
        log::warn!("Extendo Mirror needs scrcpy");
        return;
    };
    if sessions().lock().unwrap().contains_key(&device.serial) {
        return;
    }
    let settings = shared.settings();
    let args = args_for(
        &device,
        &settings.mirror,
        &settings.save_dir.join("Recordings"),
    );
    let mut cmd = quiet(Command::new(&scrcpy));
    cmd.args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // scrcpy must use the same adb, or two adb servers fight over the phone.
    if let Some(adb) = shared.status().adb_path.clone() {
        cmd.env("ADB", adb);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("cannot start scrcpy: {e}");
            return;
        }
    };
    log::info!("mirroring {} ({})", device.model, device.serial);
    let serial = device.serial.clone();
    for stream in [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        // Show scrcpy's errors in the activity log.
        thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                let line = line.trim();
                if line.starts_with("ERROR") || line.starts_with("WARN") {
                    log::warn!("mirror: {line}");
                } else if !line.is_empty() {
                    log::debug!("mirror: {line}");
                }
            }
        });
    }
    sessions().lock().unwrap().insert(serial.clone(), child);
    shared.update_status(|s| s.mirroring.push(serial.clone()));

    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(500));
        let mut map = sessions().lock().unwrap();
        let finished = match map.get_mut(&serial) {
            Some(child) => !matches!(child.try_wait(), Ok(None)),
            None => true,
        };
        if finished {
            map.remove(&serial);
            drop(map);
            log::info!("mirror window for {serial} closed");
            shared.update_status(|s| s.mirroring.retain(|x| x != &serial));
            break;
        }
    });
}

pub fn stop(serial: &str) {
    if let Some(child) = sessions().lock().unwrap().get_mut(serial) {
        let _ = child.kill();
    }
}

/// Switches a USB-connected phone to adb over WiFi and connects to it, so
/// the cable can be unplugged.
pub fn enable_wifi(shared: Arc<Shared>, device: AdbDevice) {
    let Some(adb) = shared.status().adb_path.clone() else {
        return;
    };
    thread::spawn(move || {
        let busy = |msg: &str| {
            let (msg, serial) = (msg.to_string(), device.serial.clone());
            shared.update_status(move |s| {
                s.mirror_busy.retain(|(x, _)| x != &serial);
                if !msg.is_empty() {
                    s.mirror_busy.push((serial, msg));
                }
            });
        };
        busy("Finding the phone's WiFi address…");
        let ip = adb::run(
            &adb,
            &[
                "-s",
                &device.serial,
                "shell",
                "ip",
                "-f",
                "inet",
                "addr",
                "show",
                "wlan0",
            ],
        )
        .ok()
        .and_then(|out| {
            out.split_whitespace()
                .skip_while(|w| *w != "inet")
                .nth(1)
                .and_then(|cidr| cidr.split('/').next())
                .map(str::to_string)
        });
        let Some(ip) = ip else {
            log::warn!("{}: no WiFi address. Is the phone on WiFi?", device.model);
            busy("");
            return;
        };
        busy("Turning on WiFi debugging…");
        if let Err(e) = adb::run(
            &adb,
            &["-s", &device.serial, "tcpip", &ADB_WIFI_PORT.to_string()],
        ) {
            log::warn!("{}: cannot enable WiFi debugging: {e}", device.model);
            busy("");
            return;
        }
        // adbd restarts in TCP mode.
        thread::sleep(Duration::from_secs(2));
        let address = format!("{ip}:{ADB_WIFI_PORT}");
        busy("Connecting over WiFi…");
        connect_blocking(&shared, &adb, &address);
        busy("");
    });
}

/// Connects to a phone at `address` (ip or ip:port) over WiFi.
pub fn connect(shared: Arc<Shared>, address: String) {
    let Some(adb) = shared.status().adb_path.clone() else {
        return;
    };
    let address = if address.contains(':') {
        address
    } else {
        format!("{address}:{ADB_WIFI_PORT}")
    };
    thread::spawn(move || connect_blocking(&shared, &adb, &address));
}

fn connect_blocking(shared: &Shared, adb: &PathBuf, address: &str) {
    match adb::run(adb, &["connect", address]) {
        // adb prints "failed to connect" with exit status 0 on older versions.
        Ok(out) if out.contains("connected to") => {
            log::info!("phone connected over WiFi at {address}");
            let mut s = shared.settings.lock().unwrap();
            if !s.wifi_phones.iter().any(|p| p == address) {
                s.wifi_phones.push(address.to_string());
                s.save();
            }
        }
        Ok(out) | Err(out) => log::warn!("cannot connect to {address}: {out}"),
    }
    shared.changed();
}

pub fn disconnect(shared: Arc<Shared>, address: String, forget: bool) {
    let Some(adb) = shared.status().adb_path.clone() else {
        return;
    };
    stop(&address);
    thread::spawn(move || {
        let _ = adb::run(&adb, &["disconnect", &address]);
        if forget {
            let mut s = shared.settings.lock().unwrap();
            s.wifi_phones.retain(|p| p != &address);
            s.save();
        }
        shared.changed();
    });
}

/// Re-connects remembered WiFi phones at startup.
pub fn reconnect_saved(shared: Arc<Shared>) {
    let phones = shared.settings().wifi_phones;
    if phones.is_empty() {
        return;
    }
    thread::spawn(move || {
        // Wait for the adb thread to find adb.
        thread::sleep(Duration::from_secs(2));
        let Some(adb) = shared.status().adb_path.clone() else {
            return;
        };
        for address in phones {
            let _ = adb::run(&adb, &["connect", &address]);
        }
        shared.changed();
    });
}

/// Keyboard shortcuts in the mirror window, shown in the PC app.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Alt+O", "Turn the phone screen off (mirroring keeps going)"),
    ("Alt+Shift+O", "Turn the phone screen back on"),
    ("Alt+P", "Power button (lock / wake)"),
    ("Alt+H  ·  middle-click", "Home"),
    ("Alt+B  ·  right-click", "Back"),
    ("Alt+S", "Recent apps"),
    ("Alt+M", "Menu"),
    ("Alt+↑ / Alt+↓", "Volume up / down"),
    ("Alt+N", "Open notifications"),
    ("Alt+Shift+N", "Close notifications"),
    ("Alt+R", "Rotate the phone screen"),
    ("Alt+← / Alt+→", "Rotate only the mirror window"),
    ("Alt+Z / Alt+Shift+Z", "Freeze / unfreeze the picture"),
    ("Ctrl+drag", "Pinch to zoom and rotate"),
    ("Right-click (screen off)", "Wake the phone"),
    ("Alt+F", "Fullscreen"),
    ("Alt+G", "Pixel-perfect size (1:1)"),
    ("Alt+W", "Fit the window, remove black bars"),
    ("Alt+C / Alt+X", "Copy / cut on the phone"),
    ("Alt+V", "Paste the PC clipboard on the phone"),
    ("Alt+Shift+V", "Type the PC clipboard as text"),
    ("Alt+I", "Show frame rate in the terminal"),
    (
        "Drag an .apk onto the window",
        "Install the app on the phone",
    ),
    (
        "Drag any file onto the window",
        "Copy it to the phone's Download folder",
    ),
];
