//! Opening folders, files and phone storage with the user's apps.

use std::process::{Command, Stdio};

/// Opens a folder path or an `ftp://` URL in the default file manager.
///
/// `xdg-open ftp://...` would pick the browser's FTP handler, and most
/// browsers dropped FTP, so ask for the app that opens folders instead.
pub fn open_in_file_manager(target: &str) {
    #[cfg(windows)]
    {
        spawn(Command::new("explorer").arg(target));
    }
    #[cfg(not(windows))]
    {
        if let Some(app) = default_file_manager() {
            if which("gtk-launch") && spawn(Command::new("gtk-launch").arg(&app).arg(target)) {
                return;
            }
            if which("gio") {
                if let Some(path) = desktop_file_path(&app) {
                    if spawn(Command::new("gio").arg("launch").arg(path).arg(target)) {
                        return;
                    }
                }
            }
        }
        for fm in [
            "nautilus",
            "nemo",
            "caja",
            "dolphin",
            "thunar",
            "pcmanfm-qt",
            "pcmanfm",
        ] {
            if which(fm) && spawn(Command::new(fm).arg(target)) {
                return;
            }
        }
        spawn(Command::new("xdg-open").arg(target));
    }
}

/// Opens a file with its default app.
pub fn open_file(path: &std::path::Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    spawn(Command::new(program).arg(path));
}

/// Starts the program and reaps it in the background (no zombies).
fn spawn(cmd: &mut Command) -> bool {
    match cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(e) => {
            log::debug!("cannot start {:?}: {e}", cmd.get_program());
            false
        }
    }
}

#[cfg(not(windows))]
fn default_file_manager() -> Option<String> {
    let out = Command::new("xdg-mime")
        .args(["query", "default", "inode/directory"])
        .output()
        .ok()?;
    let app = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!app.is_empty()).then_some(app)
}

#[cfg(not(windows))]
fn desktop_file_path(id: &str) -> Option<std::path::PathBuf> {
    let mut dirs = vec![dirs::data_dir()?];
    let system =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(system.split(':').map(std::path::PathBuf::from));
    dirs.into_iter()
        .map(|d| d.join("applications").join(id))
        .find(|p| p.is_file())
}

#[cfg(not(windows))]
fn which(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
        .unwrap_or(false)
}
