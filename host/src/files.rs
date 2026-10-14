//! File transfer over the viewer connection, both directions.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::protocol;
use crate::server::Out;
use crate::state::{Shared, Transfer, TransferState};

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// Files arriving from the device.
pub struct Incoming {
    shared: Arc<Shared>,
    dir: PathBuf,
    open: HashMap<u32, (BufWriter<File>, PathBuf, Instant)>,
}

impl Incoming {
    pub fn new(shared: Arc<Shared>, dir: PathBuf) -> Self {
        Self {
            shared,
            dir,
            open: HashMap::new(),
        }
    }

    pub fn begin(&mut self, id: u32, size: u64, name: &str) {
        let result = std::fs::create_dir_all(&self.dir).and_then(|_| {
            let path = unique_path(&self.dir, &safe_name(name));
            File::create(&path).map(|f| (f, path))
        });
        let (file, path) = match result {
            Ok(v) => v,
            Err(e) => {
                log::warn!("cannot save {name}: {e}");
                self.add(id, name, size, TransferState::Failed(e.to_string()), None);
                return;
            }
        };
        log::info!("receiving {name} ({})", human_size(size));
        self.add(id, name, size, TransferState::Running, Some(path.clone()));
        self.open
            .insert(id, (BufWriter::new(file), path, Instant::now()));
    }

    fn add(&self, id: u32, name: &str, size: u64, state: TransferState, path: Option<PathBuf>) {
        self.shared.update_status(|s| {
            s.transfers.retain(|t| !(t.id == id && t.incoming));
            s.transfers.push(Transfer {
                id,
                name: name.to_string(),
                incoming: true,
                size,
                done: 0,
                state,
                path,
            });
        });
    }

    pub fn data(&mut self, id: u32, data: &[u8]) {
        let Some((file, _, last_update)) = self.open.get_mut(&id) else {
            return;
        };
        if let Err(e) = file.write_all(data) {
            self.fail(id, e.to_string());
            return;
        }
        let len = data.len() as u64;
        // Throttle GUI updates to ~10/s.
        let refresh = last_update.elapsed() > Duration::from_millis(100);
        if refresh {
            *last_update = Instant::now();
        }
        self.shared
            .status()
            .transfers
            .iter_mut()
            .filter(|t| t.id == id && t.incoming)
            .for_each(|t| t.done += len);
        if refresh {
            self.shared.changed();
        }
    }

    pub fn end(&mut self, id: u32, ok: bool) {
        let Some((mut file, path, _)) = self.open.remove(&id) else {
            return;
        };
        let flushed = file.flush();
        drop(file);
        if ok && flushed.is_ok() {
            log::info!("saved {}", path.display());
            self.shared
                .update_transfer(id, true, |t| t.state = TransferState::Done);
        } else {
            let _ = std::fs::remove_file(&path);
            self.shared.update_transfer(id, true, |t| {
                t.state = TransferState::Failed("cancelled".into())
            });
        }
    }

    fn fail(&mut self, id: u32, error: String) {
        if let Some((_, path, _)) = self.open.remove(&id) {
            let _ = std::fs::remove_file(path);
        }
        log::warn!("file transfer failed: {error}");
        self.shared
            .update_transfer(id, true, |t| t.state = TransferState::Failed(error));
    }
}

impl Drop for Incoming {
    fn drop(&mut self) {
        // Connection closed mid-transfer: remove partial files.
        let ids: Vec<u32> = self.open.keys().copied().collect();
        for id in ids {
            self.fail(id, "connection closed".into());
        }
    }
}

/// Sends files to the device on a background thread.
pub fn send(shared: Arc<Shared>, out: Out, paths: Vec<PathBuf>) {
    std::thread::Builder::new()
        .name("file-send".into())
        .spawn(move || {
            for path in paths {
                send_one(&shared, &out, &path);
            }
        })
        .ok();
}

fn send_one(shared: &Shared, out: &Out, path: &Path) {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let opened = File::open(path).and_then(|f| f.metadata().map(|m| (f, m.len())));
    let (mut file, size) = match opened {
        Ok(v) => v,
        Err(e) => {
            log::warn!("cannot read {}: {e}", path.display());
            return;
        }
    };
    shared.update_status(|s| {
        s.transfers.push(Transfer {
            id,
            name: name.clone(),
            incoming: false,
            size,
            done: 0,
            state: TransferState::Running,
            path: Some(path.to_path_buf()),
        })
    });
    log::info!("sending {name} ({})", human_size(size));

    let result = (|| -> std::io::Result<()> {
        out.send(|w| protocol::write_file_begin(w, id, size, &name))?;
        let mut buf = vec![0u8; protocol::FILE_CHUNK];
        let mut last_update = Instant::now();
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            // One chunk per lock, so video frames interleave.
            out.send(|w| protocol::write_file_data(w, id, &buf[..n]))?;
            shared
                .status()
                .transfers
                .iter_mut()
                .filter(|t| t.id == id && !t.incoming)
                .for_each(|t| t.done += n as u64);
            if last_update.elapsed() > Duration::from_millis(100) {
                last_update = Instant::now();
                shared.changed();
            }
        }
        out.send(|w| protocol::write_file_end(w, id, true))
    })();

    match result {
        Ok(()) => {
            log::info!("sent {name}");
            shared.update_transfer(id, false, |t| t.state = TransferState::Done);
        }
        Err(e) => {
            log::warn!("sending {name} failed: {e}");
            let _ = out.send(|w| protocol::write_file_end(w, id, false));
            shared.update_transfer(id, false, |t| {
                t.state = TransferState::Failed(e.to_string())
            });
        }
    }
}

/// Strips path separators and other characters that are unsafe in file names.
fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"|?*"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() {
        "file".into()
    } else {
        cleaned
    }
}

/// `name`, or `name (2)`, `name (3)`... if it already exists.
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap()
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}
