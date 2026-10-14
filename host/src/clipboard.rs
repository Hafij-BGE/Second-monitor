//! Clipboard text sync with the device.
//!
//! One thread owns the system clipboard: it polls for local changes and
//! applies text received from the device, remembering the last value so a
//! change is never echoed back.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

/// Handle to the clipboard thread. Dropping it stops the thread.
pub struct ClipboardSync {
    to_host: Sender<String>,
}

impl ClipboardSync {
    /// `on_local_change` is called with new text copied on this PC.
    pub fn start(on_local_change: impl Fn(String) + Send + 'static) -> Option<Self> {
        let (tx, rx) = mpsc::channel::<String>();
        let (ready_tx, ready_rx) = mpsc::channel();
        thread::Builder::new()
            .name("clipboard".into())
            .spawn(move || {
                let clipboard = match arboard::Clipboard::new() {
                    Ok(c) => {
                        let _ = ready_tx.send(true);
                        c
                    }
                    Err(e) => {
                        log::warn!("clipboard sync unavailable: {e}");
                        let _ = ready_tx.send(false);
                        return;
                    }
                };
                run(clipboard, rx, on_local_change);
            })
            .ok()?;
        ready_rx
            .recv()
            .ok()
            .filter(|ok| *ok)
            .map(|_| Self { to_host: tx })
    }

    /// Puts text from the device on this PC's clipboard.
    pub fn set(&self, text: String) {
        let _ = self.to_host.send(text);
    }
}

fn run(mut clipboard: arboard::Clipboard, rx: Receiver<String>, on_change: impl Fn(String)) {
    // Don't push whatever is already on the clipboard at connect time.
    let mut last = clipboard.get_text().unwrap_or_default();
    loop {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(text) => {
                if text != last {
                    if let Err(e) = clipboard.set_text(text.clone()) {
                        log::debug!("cannot set clipboard: {e}");
                    }
                    last = text;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Ok(text) = clipboard.get_text() {
                    if text != last && !text.is_empty() {
                        last = text.clone();
                        on_change(text);
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}
