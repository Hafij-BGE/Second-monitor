//! Accepts viewers and runs one streaming session at a time.

use std::io::{self, BufReader, BufWriter, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::backend::{self, CaptureRequest, CursorUpdate, Frame};
use crate::clipboard::ClipboardSync;
use crate::encoder::H264Encoder;
use crate::files;
use crate::protocol::{self, ClientMessage};
use crate::scale;
use crate::state::{SessionCommand, Shared, Viewer};
use crate::{adb, discovery};

/// Binds the port and serves viewers on a background thread.
pub fn start(shared: Arc<Shared>) {
    let port = shared.settings().port;
    let listener = match TcpListener::bind(("0.0.0.0", port)) {
        Ok(l) => l,
        Err(e) => {
            let msg = if e.kind() == io::ErrorKind::AddrInUse {
                format!(
                    "TCP port {port} is busy. Extendo is probably already running \
                     (close the other window), or pick another port."
                )
            } else {
                format!("cannot listen on TCP port {port}: {e}")
            };
            log::error!("{msg}");
            shared.update_status(|s| s.server_error = Some(msg));
            return;
        }
    };
    log::info!("waiting for the Android app on TCP port {port}");
    discovery::spawn(shared.clone(), port);
    adb::spawn(shared.clone(), port);

    thread::Builder::new()
        .name("server".into())
        .spawn(move || {
            // One viewer at a time; the next one waits in the accept backlog.
            for conn in listener.incoming() {
                let stream = match conn {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!("accept failed: {e}");
                        continue;
                    }
                };
                let local = stream
                    .peer_addr()
                    .map(|a| a.ip().is_loopback())
                    .unwrap_or(false);
                if !local && !shared.settings().wifi {
                    log::info!("refused a WiFi connection (WiFi is turned off in Options)");
                    continue;
                }
                let peer = stream
                    .peer_addr()
                    .map(|a| a.to_string())
                    .unwrap_or_default();
                log::info!("viewer connected from {peer}");
                match serve(stream, &shared) {
                    Ok(()) => log::info!("viewer {peer} disconnected"),
                    Err(e) => log::warn!("viewer {peer}: {e:#}"),
                }
                *shared.session.lock().unwrap() = None;
                shared.update_status(|s| s.viewer = None);
            }
        })
        .expect("spawn server thread");
}

/// Writer shared by the video loop, cursor, clipboard and file threads.
#[derive(Clone)]
pub struct Out {
    writer: Arc<Mutex<BufWriter<TcpStream>>>,
    /// The viewer can't parse anything before the server hello.
    ready: Arc<AtomicBool>,
}

impl Out {
    pub fn send(
        &self,
        f: impl FnOnce(&mut BufWriter<TcpStream>) -> io::Result<()>,
    ) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.ready.load(Ordering::Acquire) {
            if Instant::now() > deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "stream not started",
                ));
            }
            thread::sleep(Duration::from_millis(20));
        }
        let mut w = self.writer.lock().unwrap();
        f(&mut w)?;
        w.flush()
    }

    fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
}

fn serve(stream: TcpStream, shared: &Arc<Shared>) -> Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let hello = protocol::read_client_hello(&mut reader).context("handshake")?;
    stream.set_read_timeout(None)?;
    let settings = shared.settings();
    let name = if hello.name.is_empty() {
        "Android device".to_string()
    } else {
        hello.name.clone()
    };
    log::info!(
        "{name}: screen {}x{} @ {} dpi, protocol v{}",
        hello.width,
        hello.height,
        hello.dpi,
        hello.version
    );

    let fps = if hello.fps > 0 {
        hello.fps as u32
    } else {
        settings.fps
    }
    .clamp(5, 120);
    let bitrate = if hello.bitrate_kbps > 0 {
        hello.bitrate_kbps * 1000
    } else {
        settings.bitrate_mbps * 1_000_000
    }
    .clamp(500_000, 200_000_000);

    let scale = settings.scale.clamp(0.25, 4.0);
    let req = CaptureRequest {
        mode: settings.mode,
        monitor: settings.monitor.clone(),
        width: (((hello.width as f32 / scale) as u32).max(320)) & !1,
        height: (((hello.height as f32 / scale) as u32).max(240)) & !1,
        cursor_overlay: hello.flags & protocol::FLAG_CURSOR_OVERLAY != 0,
    };
    let mut capture = backend::start(&req)?;
    let frames = capture.frames.clone();
    let input = capture.input.clone();

    let usb = stream
        .peer_addr()
        .map(|a| a.ip().is_loopback())
        .unwrap_or(false);
    shared.update_status(|s| {
        s.viewer = Some(Viewer {
            name: name.clone(),
            address: stream
                .peer_addr()
                .map(|a| a.ip().to_string())
                .unwrap_or_default(),
            usb,
            ..Default::default()
        })
    });

    let out = Out {
        writer: Arc::new(Mutex::new(BufWriter::new(stream.try_clone()?))),
        ready: Arc::new(AtomicBool::new(false)),
    };
    let closed = Arc::new(AtomicBool::new(false));
    let keyframe = Arc::new(AtomicBool::new(true));

    // Clipboard: PC -> device from the polling thread; device -> PC below.
    let clipboard = if settings.clipboard && hello.flags & protocol::FLAG_CLIPBOARD != 0 {
        let out = out.clone();
        ClipboardSync::start(move |text| {
            log::debug!("clipboard -> device ({} chars)", text.len());
            let _ = out.send(|w| protocol::write_clipboard(w, &text));
        })
    } else {
        None
    };

    // Reader: input, keyframe requests, clipboard and files from the device.
    {
        let keyframe = keyframe.clone();
        let closed = closed.clone();
        let input = input.clone();
        let mut incoming = files::Incoming::new(shared.clone(), settings.save_dir.clone());
        thread::Builder::new()
            .name("client-reader".into())
            .spawn(move || {
                loop {
                    match protocol::read_client_message(&mut reader) {
                        Ok(ClientMessage::Touch(t)) => input.lock().unwrap().touch(t),
                        Ok(ClientMessage::Mouse(m)) => input.lock().unwrap().mouse(m),
                        Ok(ClientMessage::Key { keysym, pressed }) => {
                            input.lock().unwrap().key(keysym, pressed)
                        }
                        Ok(ClientMessage::KeyframeRequest) => {
                            keyframe.store(true, Ordering::Relaxed)
                        }
                        Ok(ClientMessage::Clipboard(text)) => {
                            if let Some(c) = &clipboard {
                                c.set(text);
                            }
                        }
                        Ok(ClientMessage::FileBegin { id, size, name }) => {
                            incoming.begin(id, size, &name)
                        }
                        Ok(ClientMessage::FileData { id, data }) => incoming.data(id, &data),
                        Ok(ClientMessage::FileEnd { id, ok }) => incoming.end(id, ok),
                        Ok(ClientMessage::Unknown(t)) => log::debug!("ignoring message type {t}"),
                        Err(_) => break,
                    }
                }
                closed.store(true, Ordering::Relaxed);
            })?;
    }

    // Commands from the GUI.
    {
        let (tx, rx) = mpsc::channel();
        *shared.session.lock().unwrap() = Some(tx);
        let shared = shared.clone();
        let out = out.clone();
        let stream = stream.try_clone()?;
        thread::Builder::new()
            .name("session-commands".into())
            .spawn(move || {
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        SessionCommand::SendFiles(paths) => {
                            files::send(shared.clone(), out.clone(), paths)
                        }
                        SessionCommand::Disconnect => {
                            let _ = stream.shutdown(Shutdown::Both);
                        }
                    }
                }
            })?;
    }

    if let Some(cursor_rx) = capture.cursor.take() {
        log::info!("pointer sent separately (drawn by the device)");
        let out = out.clone();
        let closed = closed.clone();
        thread::Builder::new()
            .name("cursor".into())
            .spawn(move || send_cursor_updates(cursor_rx, out, closed))?;
    }

    let result = stream_video(
        &frames, &out, &hello, fps, bitrate, &keyframe, &closed, shared,
    );
    let _ = stream.shutdown(Shutdown::Both);
    closed.store(true, Ordering::Relaxed);
    drop(capture);
    result
}

#[allow(clippy::too_many_arguments)]
fn stream_video(
    frames: &backend::FrameSlot,
    out: &Out,
    hello: &protocol::ClientHello,
    fps: u32,
    bitrate: u32,
    keyframe: &AtomicBool,
    closed: &AtomicBool,
    shared: &Shared,
) -> Result<()> {
    let mut encoder: Option<H264Encoder> = None;
    let mut last: Option<Frame> = None;
    let started = Instant::now();
    let frame_interval = Duration::from_secs_f64(1.0 / fps as f64);
    let mut next_due = Instant::now();
    let mut stats = Stats::default();

    while !closed.load(Ordering::Relaxed) {
        let force = keyframe.swap(false, Ordering::Relaxed);
        let fresh = frames
            .take(Duration::from_millis(250))
            .map_err(anyhow::Error::msg)?;
        let frame = match (fresh, force) {
            (Some(f), _) => f,
            // Nothing changed on screen, but the viewer wants a keyframe.
            (None, true) => match last.take() {
                Some(f) => f,
                None => {
                    keyframe.store(true, Ordering::Relaxed);
                    continue;
                }
            },
            (None, false) => {
                stats.idle(shared);
                continue;
            }
        };
        let t_frame = Instant::now();
        // Never send more pixels than the viewer's decoder accepts.
        let (fw, fh) = scale::fit_size(
            frame.width,
            frame.height,
            hello.width as u32,
            hello.height as u32,
        );
        let frame = if (fw, fh) != (frame.width, frame.height) {
            scale::resize(&frame, fw, fh)
        } else {
            frame
        };
        // GNOME re-sends the whole virtual monitor when only the pointer
        // moved. The pointer travels separately, so skip identical frames.
        if !force && last.as_ref().is_some_and(|l| same_pixels(l, &frame)) {
            continue;
        }

        let rebuild = encoder.as_ref().is_none_or(|e| e.needs_rebuild(&frame));
        if rebuild {
            let e = H264Encoder::new(frame.width, frame.height, fps, bitrate)?;
            let (w, h) = e.size();
            {
                let mut writer = out.writer.lock().unwrap();
                if out.is_ready() {
                    protocol::write_resize(&mut *writer, w as u16, h as u16)?;
                } else {
                    protocol::write_server_hello(&mut *writer, hello.version, w as u16, h as u16)?;
                    out.ready.store(true, Ordering::Release);
                }
            }
            log::info!(
                "streaming {w}x{h} at up to {fps} fps, {} Mbit/s",
                bitrate / 1_000_000
            );
            shared.update_status(|s| {
                if let Some(v) = &mut s.viewer {
                    v.width = w;
                    v.height = h;
                }
            });
            encoder = Some(e);
        }
        let t_encode = Instant::now();
        let enc = encoder.as_mut().unwrap();
        let data = enc.encode(&frame, force || rebuild)?;
        let t_send = Instant::now();
        if !data.is_empty() {
            let pts = started.elapsed().as_micros() as u64;
            out.send(|w| protocol::write_video(w, pts, &data))?;
        }
        stats.add(t_frame, t_encode, t_send, data.len(), shared);
        last = Some(frame);

        // Pace to the frame rate; newer frames overwrite older ones meanwhile.
        next_due += frame_interval;
        let now = Instant::now();
        if next_due > now {
            thread::sleep(next_due - now);
        } else {
            next_due = now;
        }
    }
    Ok(())
}

fn same_pixels(a: &Frame, b: &Frame) -> bool {
    use rayon::prelude::*;
    a.width == b.width
        && a.height == b.height
        && a.bgra
            .par_chunks(1 << 18)
            .zip(b.bgra.par_chunks(1 << 18))
            .all(|(x, y)| x == y)
}

/// Forwards pointer updates as soon as they arrive, merging any backlog so a
/// slow link never queues up stale positions.
fn send_cursor_updates(rx: mpsc::Receiver<CursorUpdate>, out: Out, closed: Arc<AtomicBool>) {
    use mpsc::RecvTimeoutError;
    // Keep a newer position but never lose an image that wasn't sent yet.
    fn merge(pending: &mut Option<CursorUpdate>, newer: CursorUpdate) {
        let image = newer
            .image
            .clone()
            .or_else(|| pending.take().and_then(|p| p.image));
        *pending = Some(CursorUpdate { image, ..newer });
    }
    let mut pending: Option<CursorUpdate> = None;
    while !closed.load(Ordering::Relaxed) {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(newer) => {
                merge(&mut pending, newer);
                while let Ok(newer) = rx.try_recv() {
                    merge(&mut pending, newer);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if !out.is_ready() {
            continue;
        }
        if let Some(update) = pending.take() {
            if out.send(|w| protocol::write_cursor(w, &update)).is_err() {
                break;
            }
        }
    }
}

/// Per-stage timings: shown in the GUI and, with RUST_LOG=debug, logged.
#[derive(Default)]
struct Stats {
    since: Option<Instant>,
    frames: u32,
    bytes: usize,
    scale: Duration,
    encode: Duration,
    send: Duration,
}

impl Stats {
    fn add(
        &mut self,
        frame: Instant,
        encode: Instant,
        send: Instant,
        bytes: usize,
        shared: &Shared,
    ) {
        let now = Instant::now();
        self.since.get_or_insert(frame);
        self.frames += 1;
        self.bytes += bytes;
        self.scale += encode - frame;
        self.encode += send - encode;
        self.send += now - send;
        self.report(shared);
    }

    /// Called while nothing changes on screen, so the GUI drops to 0 fps.
    fn idle(&mut self, shared: &Shared) {
        self.since.get_or_insert_with(Instant::now);
        self.report(shared);
    }

    fn report(&mut self, shared: &Shared) {
        let Some(since) = self.since else { return };
        let elapsed = since.elapsed();
        if elapsed < Duration::from_secs(1) {
            return;
        }
        let n = self.frames.max(1) as f64;
        let fps = self.frames as f64 / elapsed.as_secs_f64();
        let mbps = self.bytes as f64 * 8.0 / elapsed.as_secs_f64() / 1e6;
        let encode_ms = self.encode.as_secs_f64() * 1e3 / n;
        log::debug!(
            "{fps:.1} fps, {mbps:.1} Mbit/s, per frame: scale {:.1} ms, encode {encode_ms:.1} ms, send {:.1} ms",
            self.scale.as_secs_f64() * 1e3 / n,
            self.send.as_secs_f64() * 1e3 / n,
        );
        shared.update_status(|s| {
            if let Some(v) = &mut s.viewer {
                v.fps = fps as f32;
                v.mbps = mbps as f32;
                if self.frames > 0 {
                    v.encode_ms = encode_ms as f32;
                }
            }
        });
        *self = Stats::default();
    }
}
