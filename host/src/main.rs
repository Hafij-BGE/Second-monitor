mod adb;
mod backend;
mod discovery;
mod encoder;
mod protocol;
mod scale;

use std::io::{BufReader, BufWriter, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;

use backend::{CaptureRequest, Frame, Mode};
use encoder::H264Encoder;
use protocol::ClientMessage;

/// Use an Android phone or tablet as a second monitor over USB or WiFi.
#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    /// TCP port the Android app connects to.
    #[arg(long, default_value_t = protocol::DEFAULT_PORT)]
    port: u16,

    /// extend: add a virtual monitor. mirror: show an existing monitor.
    #[arg(long, value_enum, default_value_t = Mode::Extend)]
    mode: Mode,

    /// Monitor to capture: connector name on Linux (e.g. eDP-1), index on Windows. See --list.
    #[arg(long)]
    monitor: Option<String>,

    /// List monitors and exit.
    #[arg(long)]
    list: bool,

    /// Maximum frames per second.
    #[arg(long, default_value_t = 60)]
    fps: u32,

    /// Video bitrate in Mbit/s.
    #[arg(long, default_value_t = 12)]
    bitrate: u32,

    /// Divide the device resolution by this for the virtual monitor
    /// (e.g. 1.5 makes text bigger on high-DPI phones).
    #[arg(long, default_value_t = 1.0)]
    scale: f32,

    /// Don't set up USB (adb reverse).
    #[arg(long)]
    no_usb: bool,

    /// Don't answer WiFi discovery broadcasts.
    #[arg(long)]
    no_discovery: bool,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();
    platform_init();

    if args.list {
        for m in backend::list_monitors()? {
            println!("{m}");
        }
        return Ok(());
    }

    let listener = match TcpListener::bind(("0.0.0.0", args.port)) {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => anyhow::bail!(
            "TCP port {} is busy. Second Monitor host is probably already running \
             (close the other window), or pick another port with --port",
            args.port
        ),
        Err(e) => {
            return Err(e).with_context(|| format!("cannot listen on TCP port {}", args.port))
        }
    };
    log::info!("waiting for the Android app on TCP port {}", args.port);
    if !args.no_discovery {
        discovery::spawn(args.port);
    }
    if !args.no_usb {
        adb::spawn(args.port);
    }

    // One viewer at a time; the next one waits in the accept backlog.
    for conn in listener.incoming() {
        let stream = match conn {
            Ok(s) => s,
            Err(e) => {
                log::warn!("accept failed: {e}");
                continue;
            }
        };
        let peer = stream
            .peer_addr()
            .map(|a| a.to_string())
            .unwrap_or_default();
        log::info!("viewer connected from {peer}");
        match serve(stream, &args) {
            Ok(()) => log::info!("viewer {peer} disconnected"),
            Err(e) => log::warn!("viewer {peer}: {e:#}"),
        }
    }
    Ok(())
}

fn serve(stream: TcpStream, args: &Args) -> Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let hello = protocol::read_client_hello(&mut reader).context("handshake")?;
    stream.set_read_timeout(None)?;
    log::info!(
        "device screen {}x{} @ {} dpi",
        hello.width,
        hello.height,
        hello.dpi
    );

    let scale = args.scale.max(0.25);
    let req = CaptureRequest {
        mode: args.mode,
        monitor: args.monitor.clone(),
        width: (((hello.width as f32 / scale) as u32).max(320)) & !1,
        height: (((hello.height as f32 / scale) as u32).max(240)) & !1,
        cursor_overlay: hello.flags & protocol::FLAG_CURSOR_OVERLAY != 0,
    };
    let mut capture = backend::start(&req)?;
    let frames = capture.frames.clone();
    let input = capture.input.clone();

    // Reader: touches and keyframe requests from the device.
    let keyframe = Arc::new(AtomicBool::new(true));
    let closed = Arc::new(AtomicBool::new(false));
    {
        let keyframe = keyframe.clone();
        let closed = closed.clone();
        let input = input.clone();
        thread::Builder::new()
            .name("client-reader".into())
            .spawn(move || {
                loop {
                    match protocol::read_client_message(&mut reader) {
                        Ok(ClientMessage::Touch(t)) => input.lock().unwrap().touch(t),
                        Ok(ClientMessage::KeyframeRequest) => {
                            keyframe.store(true, Ordering::Relaxed)
                        }
                        Ok(ClientMessage::Unknown(t)) => log::debug!("ignoring message type {t}"),
                        Err(_) => break,
                    }
                }
                closed.store(true, Ordering::Relaxed);
            })?;
    }

    // Shared by the video loop and the cursor thread.
    let out = Arc::new(Mutex::new(BufWriter::new(stream.try_clone()?)));
    let hello_sent = Arc::new(AtomicBool::new(false));
    if let Some(cursor_rx) = capture.cursor.take() {
        log::info!("pointer sent separately (drawn by the device)");
        let out = out.clone();
        let hello_sent = hello_sent.clone();
        let closed = closed.clone();
        thread::Builder::new()
            .name("cursor".into())
            .spawn(move || send_cursor_updates(cursor_rx, out, hello_sent, closed))?;
    }
    let mut encoder: Option<H264Encoder> = None;
    let mut last: Option<Frame> = None;
    let started = Instant::now();
    let frame_interval = Duration::from_secs_f64(1.0 / args.fps.max(1) as f64);
    let mut next_due = Instant::now();
    let mut sent_hello = false;
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
            (None, false) => continue,
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

        let rebuild = encoder.as_ref().map_or(true, |e| e.needs_rebuild(&frame));
        if rebuild {
            let e = H264Encoder::new(
                frame.width,
                frame.height,
                args.fps,
                args.bitrate * 1_000_000,
            )?;
            let (w, h) = e.size();
            let mut out = out.lock().unwrap();
            if sent_hello {
                protocol::write_resize(&mut *out, w as u16, h as u16)?;
            } else {
                protocol::write_server_hello(&mut *out, hello.version, w as u16, h as u16)?;
                sent_hello = true;
                hello_sent.store(true, Ordering::Relaxed);
            }
            log::info!("streaming {w}x{h}");
            encoder = Some(e);
        }
        let t_encode = Instant::now();
        let enc = encoder.as_mut().unwrap();
        let data = enc.encode(&frame, force || rebuild)?;
        let t_send = Instant::now();
        if !data.is_empty() {
            let pts = started.elapsed().as_micros() as u64;
            let mut out = out.lock().unwrap();
            protocol::write_video(&mut *out, pts, &data)?;
            out.flush()?;
        }
        stats.add(t_frame, t_encode, t_send, data.len());
        last = Some(frame);

        // Pace to --fps; newer frames overwrite older ones meanwhile.
        next_due += frame_interval;
        let now = Instant::now();
        if next_due > now {
            thread::sleep(next_due - now);
        } else {
            next_due = now;
        }
    }
    drop(capture);
    Ok(())
}

/// Forwards pointer updates as soon as they arrive, merging any backlog so a
/// slow link never queues up stale positions.
fn send_cursor_updates(
    rx: std::sync::mpsc::Receiver<backend::CursorUpdate>,
    out: Arc<Mutex<BufWriter<TcpStream>>>,
    hello_sent: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
) {
    use std::sync::mpsc::RecvTimeoutError;
    // Keep a newer position but never lose an image that wasn't sent yet.
    fn merge(pending: &mut Option<backend::CursorUpdate>, newer: backend::CursorUpdate) {
        let image = newer
            .image
            .clone()
            .or_else(|| pending.take().and_then(|p| p.image));
        *pending = Some(backend::CursorUpdate { image, ..newer });
    }
    let mut pending: Option<backend::CursorUpdate> = None;
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
        flush_cursor(&mut pending, &out, &hello_sent, &closed);
    }
}

fn flush_cursor(
    pending: &mut Option<backend::CursorUpdate>,
    out: &Mutex<BufWriter<TcpStream>>,
    hello_sent: &AtomicBool,
    closed: &AtomicBool,
) {
    // The viewer can't parse cursor messages before the video hello.
    if pending.is_none() || !hello_sent.load(Ordering::Relaxed) {
        return;
    }
    let update = pending.take().unwrap();
    let mut out = out.lock().unwrap();
    if protocol::write_cursor(&mut *out, &update)
        .and_then(|_| out.flush())
        .is_err()
    {
        closed.store(true, Ordering::Relaxed);
    }
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

/// Per-stage timings, logged every few seconds at debug level.
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
    fn add(&mut self, frame: Instant, encode: Instant, send: Instant, bytes: usize) {
        let now = Instant::now();
        let since = *self.since.get_or_insert(frame);
        self.frames += 1;
        self.bytes += bytes;
        self.scale += encode - frame;
        self.encode += send - encode;
        self.send += now - send;
        let elapsed = now - since;
        if elapsed >= Duration::from_secs(3) {
            let n = self.frames.max(1);
            log::debug!(
                "{:.1} fps, {:.1} Mbit/s, per frame: scale {:.1} ms, encode {:.1} ms, send {:.1} ms",
                self.frames as f64 / elapsed.as_secs_f64(),
                self.bytes as f64 * 8.0 / elapsed.as_secs_f64() / 1e6,
                self.scale.as_secs_f64() * 1e3 / n as f64,
                self.encode.as_secs_f64() * 1e3 / n as f64,
                self.send.as_secs_f64() * 1e3 / n as f64,
            );
            *self = Stats::default();
        }
    }
}

#[cfg(windows)]
fn platform_init() {
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    // Real pixel coordinates for capture and input on scaled displays.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

#[cfg(not(windows))]
fn platform_init() {}
