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
        }
    }
}

fn default_save_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Second Monitor")
}

fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("second-monitor").join("settings.json"))
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
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(self) {
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

#[derive(Debug, Clone, Default)]
pub struct Status {
    /// Error from binding the port, if any.
    pub server_error: Option<String>,
    pub adb_path: Option<PathBuf>,
    pub usb_devices: Vec<String>,
    pub viewer: Option<Viewer>,
    pub transfers: Vec<Transfer>,
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
