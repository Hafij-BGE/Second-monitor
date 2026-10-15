//! State shared between the server threads and the GUI.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::backend::Mode;
use crate::protocol;

/// User settings, saved to the config directory as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub port: u16,
    pub mode: Mode,
    /// Linux connector name or Windows monitor index; `None` = automatic.
    pub monitor: Option<String>,
    /// Defaults used when the device leaves quality on "Auto".
    pub fps: u32,
    pub bitrate_mbps: u32,
    /// Divides the device resolution to make things bigger.
    pub scale: f32,
    pub usb: bool,
    pub wifi: bool,
    pub clipboard: bool,
    pub save_dir: PathBuf,
    pub dark_theme: bool,
    pub mirror: MirrorSettings,
    /// Phones added for WiFi mirroring (ip:port).
    pub wifi_phones: Vec<String>,
    /// Last address typed for phone storage over WiFi.
    pub storage_address: String,
    /// Tablets allowed to connect over WiFi without asking.
    pub trusted_devices: Vec<TrustedDevice>,
    /// Settings as on disk before command-line options changed them for
    /// this run; those options are never saved.
    #[serde(skip)]
    pub on_disk: Option<Box<Settings>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrustedDevice {
    pub id: String,
    pub name: String,
}

/// Options for Extendo Mirror (scrcpy).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MirrorSettings {
    /// Longest side in pixels; 0 = full resolution.
    pub max_size: u32,
    pub bitrate_mbps: u32,
    pub max_fps: u32,
    pub audio: bool,
    /// Phone screen off while mirroring keeps working.
    pub screen_off: bool,
    pub stay_awake: bool,
    pub show_touches: bool,
    pub always_on_top: bool,
    pub fullscreen: bool,
    /// Simulate a physical keyboard (better for games and layouts).
    pub physical_keyboard: bool,
    pub power_off_on_close: bool,
    pub record: bool,
    pub camera: bool,
    /// Back / Home / Recents bar drawn under the phone in the mirror window.
    pub navbar: bool,
}

impl Default for MirrorSettings {
    fn default() -> Self {
        Self {
            max_size: 0,
            bitrate_mbps: 8,
            max_fps: 60,
            audio: true,
            screen_off: false,
            stay_awake: true,
            show_touches: false,
            always_on_top: false,
            fullscreen: false,
            physical_keyboard: false,
            power_off_on_close: false,
            record: false,
            camera: false,
            navbar: true,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            port: protocol::DEFAULT_PORT,
            mode: Mode::Extend,
            monitor: None,
            fps: 60,
            bitrate_mbps: 12,
            scale: 1.0,
            usb: true,
            wifi: true,
            clipboard: true,
            save_dir: default_save_dir(),
            dark_theme: true,
            mirror: MirrorSettings::default(),
            wifi_phones: Vec::new(),
            storage_address: String::new(),
            trusted_devices: Vec::new(),
            on_disk: None,
        }
    }
}

fn default_save_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Extendo")
}

fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("extendo").join("settings.json"))
}

impl Settings {
    pub fn load() -> Self {
        settings_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = settings_path() else { return };
        // Keep the saved values of anything overridden on the command line.
        let mut out = self.clone();
        if let Some(base) = &self.on_disk {
            out.port = base.port;
            out.mode = base.mode;
            out.monitor = base.monitor.clone();
            out.fps = base.fps;
            out.bitrate_mbps = base.bitrate_mbps;
            out.scale = base.scale;
            out.usb = base.usb;
            out.wifi = base.wifi;
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(&out) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    log::warn!("cannot save settings to {}: {e}", path.display());
                }
            }
            Err(e) => log::warn!("cannot encode settings: {e}"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Viewer {
    pub name: String,
    pub address: String,
    pub usb: bool,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub mbps: f32,
    pub encode_ms: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TransferState {
    Running,
    Done,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Transfer {
    pub id: u32,
    pub name: String,
    /// true = device to PC.
    pub incoming: bool,
    pub size: u64,
    pub done: u64,
    pub state: TransferState,
    /// Where an incoming file was saved.
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdbDevice {
    pub serial: String,
    pub model: String,
    pub wifi: bool,
    /// adb is listening on the network (port 5555), even if on USB now.
    pub wifi_debugging: bool,
}

/// A phone running the Extendo storage server, found on WiFi.
#[derive(Debug, Clone, PartialEq)]
pub struct StoragePhone {
    pub name: String,
    pub address: String,
    pub port: u16,
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    /// Error from binding the port, if any.
    pub server_error: Option<String>,
    pub adb_path: Option<PathBuf>,
    /// All devices adb sees, USB and WiFi.
    pub usb_devices: Vec<AdbDevice>,
    /// scrcpy used by Extendo Mirror, with its version line.
    pub scrcpy: Option<(PathBuf, String)>,
    /// Serials with a mirror window open.
    pub mirroring: Vec<String>,
    /// Busy message per serial (e.g. "Switching to WiFi…").
    pub mirror_busy: Vec<(String, String)>,
    pub storage_phones: Vec<StoragePhone>,
    pub storage_searching: bool,
    /// A tablet waiting for the user to allow it.
    pub pairing: Option<PairingRequest>,
    /// Code shown while pairing; the tablet shows the same one.
    pub pairing_code: String,
    pub viewer: Option<Viewer>,
    pub transfers: Vec<Transfer>,
}

#[derive(Debug, Clone)]
pub struct PairingRequest {
    pub id: String,
    pub name: String,
    pub address: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingDecision {
    Deny,
    AllowOnce,
    AlwaysAllow,
}

/// Commands from the GUI to the connected viewer's session.
pub enum SessionCommand {
    SendFiles(Vec<PathBuf>),
    Disconnect,
}

#[derive(Default)]
pub struct Shared {
    pub settings: Mutex<Settings>,
    pub status: Mutex<Status>,
    pub log: Mutex<VecDeque<String>>,
    pub session: Mutex<Option<Sender<SessionCommand>>>,
    /// Answer channel for the pairing request in `Status::pairing`.
    pub pairing_reply: Mutex<Option<Sender<PairingDecision>>>,
    /// Set when a window can show pairing requests.
    pub has_gui: std::sync::atomic::AtomicBool,
    repaint: Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl Shared {
    pub fn new(settings: Settings) -> Arc<Self> {
        Arc::new(Self {
            settings: Mutex::new(settings),
            ..Default::default()
        })
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    pub fn status(&self) -> MutexGuard<'_, Status> {
        self.status.lock().unwrap()
    }

    /// Lets the GUI redraw when background threads change state.
    pub fn set_repaint(&self, f: impl Fn() + Send + 'static) {
        *self.repaint.lock().unwrap() = Some(Box::new(f));
    }

    pub fn changed(&self) {
        if let Some(f) = self.repaint.lock().unwrap().as_ref() {
            f();
        }
    }

    pub fn update_status(&self, f: impl FnOnce(&mut Status)) {
        f(&mut self.status());
        self.changed();
    }

    pub fn update_transfer(&self, id: u32, incoming: bool, f: impl FnOnce(&mut Transfer)) {
        self.update_status(|s| {
            if let Some(t) = s
                .transfers
                .iter_mut()
                .find(|t| t.id == id && t.incoming == incoming)
            {
                f(t);
            }
        });
    }

    /// Sends a command to the connected viewer. Returns false if none.
    pub fn command(&self, cmd: SessionCommand) -> bool {
        match self.session.lock().unwrap().as_ref() {
            Some(tx) => tx.send(cmd).is_ok(),
            None => false,
        }
    }

    /// Answers the pending pairing request.
    pub fn answer_pairing(&self, decision: PairingDecision) {
        if let Some(tx) = self.pairing_reply.lock().unwrap().take() {
            let _ = tx.send(decision);
        }
        self.update_status(|s| s.pairing = None);
    }

    pub fn push_log(&self, line: String) {
        let mut log = self.log.lock().unwrap();
        if log.len() >= 300 {
            log.pop_front();
        }
        log.push_back(line);
        drop(log);
        self.changed();
    }
}
