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
use crate::state::{
    PairingDecision, PairingRequest, SessionCommand, Shared, TrustedDevice, Viewer,
};
use crate::tls::{self, TlsWriter};
use crate::{adb, discovery};

/// Binds the port and serves viewers on a background thread.
pub fn start(shared: Arc<Shared>) {
    let port = shared.settings().port;
    let identity = match tls::load_identity() {
        Ok(id) => Arc::new(id),
        Err(e) => {
            let msg = format!("cannot set up encryption: {e:#}");
            log::error!("{msg}");
            shared.update_status(|s| s.server_error = Some(msg));
            return;
        }
    };
    shared.update_status(|s| s.pairing_code = identity.pairing_code());
    discovery::spawn(shared.clone(), port);
    adb::spawn(shared.clone(), port);
    if shared.settings().manage_virtual_display {
        backend::virtual_display_idle();
    }

    thread::Builder::new()
        .name("server".into())
        .spawn(move || serve_forever(shared, identity, port))
        .expect("spawn server thread");
}

/// Listens on all interfaces, or only on localhost (USB) when WiFi is off,
/// re-binding when that setting changes.
fn serve_forever(shared: Arc<Shared>, identity: Arc<tls::Identity>, port: u16) {
    let mut bound: Option<(bool, TcpListener)> = None;
    loop {
        let wifi = shared.settings().wifi;
        if bound.as_ref().map(|(w, _)| *w) != Some(wifi) {
            bound = None;
            let addr = if wifi { "0.0.0.0" } else { "127.0.0.1" };
            match TcpListener::bind((addr, port)).and_then(|l| l.set_nonblocking(true).map(|_| l)) {
                Ok(l) => {
                    shared.update_status(|s| s.server_error = None);
                    if wifi {
                        log::info!("waiting for the tablet on port {port} (USB and WiFi)");
                    } else {
                        log::info!("waiting for the tablet over USB only (WiFi port closed)");
                    }
                    bound = Some((wifi, l));
                }
                Err(e) => {
                    let msg = if e.kind() == io::ErrorKind::AddrInUse {
                        format!(
                            "TCP port {port} is busy. Extendo is probably already running \
                             (close the other window), or pick another port."
                        )
                    } else {
                        format!("cannot listen on TCP port {port}: {e}")
                    };
                    // Log once, not on every retry.
                    if shared.status().server_error.as_deref() != Some(msg.as_str()) {
                        log::error!("{msg}");
                    }
                    shared.update_status(|s| s.server_error = Some(msg));
                    thread::sleep(Duration::from_secs(3));
                    continue;
                }
            }
        }
        let (_, listener) = bound.as_ref().unwrap();
        let stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(e) => {
                log::warn!("accept failed: {e}");
                continue;
            }
        };
        let _ = stream.set_nonblocking(false);
        let peer = stream
            .peer_addr()
            .map(|a| a.to_string())
            .unwrap_or_default();
        log::info!("connection from {peer}");
        // One viewer at a time.
        match serve(stream, &shared, &identity) {
            Ok(()) => log::info!("viewer {peer} disconnected"),
            Err(e) => log::warn!("viewer {peer}: {e:#}"),
        }
        *shared.session.lock().unwrap() = None;
        shared.update_status(|s| s.viewer = None);
    }
}

/// Writer shared by the video loop, cursor, clipboard and file threads.
#[derive(Clone)]
pub struct Out {
    writer: Arc<Mutex<BufWriter<TlsWriter>>>,
    /// The viewer can't parse anything before the server hello.
    ready: Arc<AtomicBool>,
}

impl Out {
    pub fn send(
        &self,
        f: impl FnOnce(&mut BufWriter<TlsWriter>) -> io::Result<()>,
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

fn serve(stream: TcpStream, shared: &Arc<Shared>, identity: &tls::Identity) -> Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let (tls_reader, mut tls_writer) = tls::accept(stream.try_clone()?, identity.config.clone())
        .context("encryption handshake")?;
    let mut reader = BufReader::new(tls_reader);
    let hello = protocol::read_client_hello(&mut reader).context("handshake")?;
    let name = if hello.name.is_empty() {
        "Android device".to_string()
    } else {
        hello.name.clone()
    };
    let usb = stream
        .peer_addr()
        .map(|a| a.ip().is_loopback())
        .unwrap_or(false);
    authorize(shared, &hello, &name, usb, &stream, &mut tls_writer)?;
    stream.set_read_timeout(None)?;
    let settings = shared.settings();
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
        manage_virtual_display: settings.manage_virtual_display,
    };
    if settings.tablet_size != Some((req.width, req.height)) {
        let mut s = shared.settings.lock().unwrap();
        s.tablet_size = Some((req.width, req.height));
        s.save();
    }
    let mut capture = backend::start(&req)?;
    let frames = capture.frames.clone();
    let input = capture.input.clone();

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
        writer: Arc::new(Mutex::new(BufWriter::with_capacity(64 * 1024, tls_writer))),
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

/// USB is always allowed. Over WiFi the tablet must be trusted, or the
/// user allows it in the window while both screens show the same code.
fn authorize(
    shared: &Shared,
    hello: &protocol::ClientHello,
    name: &str,
    usb: bool,
    stream: &TcpStream,
    w: &mut TlsWriter,
) -> Result<()> {
    let trusted = shared
        .settings()
        .trusted_devices
        .iter()
        .any(|d| d.id == hello.device_id);
    if usb || trusted {
        protocol::write_auth(w, protocol::AUTH_OK)?;
        return Ok(());
    }
    let address = stream
        .peer_addr()
        .map(|a| a.ip().to_string())
        .unwrap_or_default();
    if !shared.has_gui.load(Ordering::Relaxed) {
        protocol::write_auth(w, protocol::AUTH_DENIED)?;
        anyhow::bail!("{name} ({address}) isn't paired; open the Extendo window to allow it");
    }
    log::info!("{name} ({address}) wants to connect; waiting for you to allow it");
    protocol::write_auth(w, protocol::AUTH_PENDING)?;
    let (tx, rx) = mpsc::channel();
    *shared.pairing_reply.lock().unwrap() = Some(tx);
    shared.update_status(|s| {
        s.pairing = Some(PairingRequest {
            id: hello.device_id.clone(),
            name: name.to_string(),
            address: address.clone(),
        })
    });
    let decision = rx
        .recv_timeout(Duration::from_secs(120))
        .unwrap_or(PairingDecision::Deny);
    shared.update_status(|s| s.pairing = None);
    *shared.pairing_reply.lock().unwrap() = None;
    match decision {
        PairingDecision::Deny => {
            let _ = protocol::write_auth(w, protocol::AUTH_DENIED);
            anyhow::bail!("{name} ({address}) was not allowed");
        }
        PairingDecision::AlwaysAllow => {
            let mut s = shared.settings.lock().unwrap();
            if !s.trusted_devices.iter().any(|d| d.id == hello.device_id) {
                s.trusted_devices.push(TrustedDevice {
                    id: hello.device_id.clone(),
                    name: name.to_string(),
                });
                s.save();
            }
            log::info!("{name} is now trusted");
        }
        PairingDecision::AllowOnce => log::info!("{name} allowed for this connection"),
    }
    protocol::write_auth(w, protocol::AUTH_OK)?;
    Ok(())
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
