//! Keyboard shortcuts that work while another app has focus.
//!
//! Windows: Ctrl+Alt+E switches Extend/Mirror, Ctrl+Alt+X disconnects
//! (RegisterHotKey). Linux: Wayland apps can't grab keys, so the desktop runs
//! `extendo --toggle-mode` / `extendo --disconnect`; on GNOME the window can
//! add those shortcuts to Settings > Keyboard.

use std::sync::{Arc, OnceLock};

use crate::backend::Mode;
use crate::state::{SessionCommand, Shared};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    ToggleMode,
    Disconnect,
}

/// Command-line flag and shortcut name for each action.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const ACTIONS: [(Action, &str, &str, &str); 2] = [
    (
        Action::ToggleMode,
        "--toggle-mode",
        "Extendo: switch Extend / Mirror",
        "<Control><Alt>e",
    ),
    (
        Action::Disconnect,
        "--disconnect",
        "Extendo: disconnect the tablet",
        "<Control><Alt>x",
    ),
];

fn shared() -> &'static OnceLock<Arc<Shared>> {
    static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
    &SHARED
}

/// Starts listening for shortcuts (Windows) and lets the running copy act
/// on `extendo --toggle-mode` and friends.
pub fn init(s: Arc<Shared>) {
    let _ = shared().set(s.clone());
    #[cfg(windows)]
    if s.settings().hotkeys {
        windows_keys::start();
    }
}

pub fn run(action: Action) {
    let Some(shared) = shared().get() else { return };
    match action {
        Action::ToggleMode => {
            let mut next = Mode::Extend;
            shared.update_settings(|s| {
                s.mode = match s.mode {
                    Mode::Extend => Mode::Mirror,
                    Mode::Mirror => Mode::Extend,
                };
                next = s.mode;
            });
            let name = match next {
                Mode::Extend => "Extend",
                Mode::Mirror => "Mirror",
            };
            // The tablet reconnects by itself and gets the new mode.
            if shared.command(SessionCommand::Disconnect) {
                log::info!("shortcut: switching the tablet to {name}");
            } else {
                log::info!("shortcut: the next connection uses {name}");
            }
        }
        Action::Disconnect => {
            if shared.command(SessionCommand::Disconnect) {
                log::info!("shortcut: tablet disconnected");
            }
        }
    }
}

/// Turns the Windows shortcuts on or off right away.
#[cfg(windows)]
pub fn set_enabled(on: bool) {
    if on {
        windows_keys::start();
    } else {
        windows_keys::stop();
    }
}

#[cfg(windows)]
mod windows_keys {
    use std::sync::atomic::{AtomicU32, Ordering};

    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetMessageW, PostThreadMessageW, MSG, WM_HOTKEY, WM_QUIT,
    };

    use super::{run, Action};

    /// Thread running the message loop, 0 when off.
    static THREAD: AtomicU32 = AtomicU32::new(0);

    pub fn start() {
        if THREAD.load(Ordering::Relaxed) != 0 {
            return;
        }
        let _ = std::thread::Builder::new()
            .name("hotkeys".into())
            .spawn(|| unsafe {
                THREAD.store(GetCurrentThreadId(), Ordering::Relaxed);
                let mods = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
                for (id, key) in [(1, b'E'), (2, b'X')] {
                    if let Err(e) = RegisterHotKey(None, id, mods, key as u32) {
                        log::warn!(
                            "shortcut Ctrl+Alt+{} is taken by another app: {e}",
                            key as char
                        );
                    }
                }
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    if msg.message == WM_HOTKEY {
                        match msg.wParam.0 {
                            1 => run(Action::ToggleMode),
                            2 => run(Action::Disconnect),
                            _ => {}
                        }
                    }
                }
                let _ = UnregisterHotKey(None, 1);
                let _ = UnregisterHotKey(None, 2);
                THREAD.store(0, Ordering::Relaxed);
            });
    }

    pub fn stop() {
        let id = THREAD.load(Ordering::Relaxed);
        if id != 0 {
            unsafe {
                let _ = PostThreadMessageW(id, WM_QUIT, Default::default(), Default::default());
            }
        }
    }
}

/// GNOME custom shortcuts (Settings > Keyboard > Custom Shortcuts).
#[cfg(target_os = "linux")]
pub mod gnome {
    use std::process::Command;

    use anyhow::{bail, Context, Result};

    use super::ACTIONS;

    const LIST_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
    const ITEM_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
    const BASE: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings";

    fn path(i: usize) -> String {
        format!("{BASE}/extendo-{i}/")
    }

    fn gsettings(args: &[&str]) -> Result<String> {
        let out = Command::new("gsettings")
            .args(args)
            .output()
            .context("gsettings not found")?;
        if !out.status.success() {
            bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Paths in the custom shortcut list.
    fn list() -> Result<Vec<String>> {
        let raw = gsettings(&["get", LIST_SCHEMA, "custom-keybindings"])?;
        let inner = raw.trim_start_matches("@as").trim();
        let inner = inner.trim_start_matches('[').trim_end_matches(']');
        Ok(inner
            .split(',')
            .map(|p| p.trim().trim_matches('\'').to_string())
            .filter(|p| !p.is_empty())
            .collect())
    }

    fn write_list(paths: &[String]) -> Result<()> {
        let value = format!(
            "[{}]",
            paths
                .iter()
                .map(|p| format!("'{p}'"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        gsettings(&["set", LIST_SCHEMA, "custom-keybindings", &value]).map(|_| ())
    }

    /// A GVariant string literal.
    fn quoted(s: &str) -> String {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }

    /// GNOME's shortcut settings exist on this desktop.
    pub fn available() -> bool {
        list().is_ok()
    }

    pub fn installed() -> bool {
        list().is_ok_and(|l| l.contains(&path(0)))
    }

    pub fn install() -> Result<()> {
        let exe = crate::background::launch_path()?;
        let mut paths = list()?;
        for (i, (_, flag, name, keys)) in ACTIONS.iter().enumerate() {
            let p = path(i);
            let item = format!("{ITEM_SCHEMA}:{p}");
            // The command line is parsed by GLib: quote the path.
            let command = format!(
                "'{}' {flag}",
                exe.display().to_string().replace('\'', "'\\''")
            );
            gsettings(&["set", &item, "name", &quoted(name)])?;
            gsettings(&["set", &item, "command", &quoted(&command)])?;
            gsettings(&["set", &item, "binding", &quoted(keys)])?;
            if !paths.contains(&p) {
                paths.push(p);
            }
        }
        write_list(&paths)?;
        log::info!("added Ctrl+Alt+E and Ctrl+Alt+X to GNOME's keyboard shortcuts");
        Ok(())
    }

    pub fn remove() -> Result<()> {
        let mut paths = list()?;
        for i in 0..ACTIONS.len() {
            let p = path(i);
            paths.retain(|x| *x != p);
            let _ = gsettings(&["reset-recursively", &format!("{ITEM_SCHEMA}:{p}")]);
        }
        write_list(&paths)?;
        log::info!("removed Extendo's keyboard shortcuts from GNOME");
        Ok(())
    }
}
