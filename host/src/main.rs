mod adb;
mod backend;
mod discovery;
mod encoder;
mod protocol;
mod scale;

use std::io::{BufReader, BufWriter, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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
    };
    let capture = backend::start(&req)?;
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

    let mut out = BufWriter::new(stream.try_clone()?);
    let mut encoder: Option<H264Encoder> = None;
    let mut last: Option<Frame> = None;
    let started = Instant::now();
    let frame_interval = Duration::from_secs_f64(1.0 / args.fps.max(1) as f64);
    let mut next_due = Instant::now();
    let mut sent_hello = false;

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

        let rebuild = encoder.as_ref().map_or(true, |e| e.needs_rebuild(&frame));
        if rebuild {
            let e = H264Encoder::new(
                frame.width,
                frame.height,
                args.fps,
                args.bitrate * 1_000_000,
            )?;
            let (w, h) = e.size();
            if sent_hello {
                protocol::write_resize(&mut out, w as u16, h as u16)?;
            } else {
                protocol::write_server_hello(&mut out, w as u16, h as u16)?;
                sent_hello = true;
            }
            log::info!("streaming {w}x{h}");
            encoder = Some(e);
        }
        let enc = encoder.as_mut().unwrap();
        let data = enc.encode(&frame, force || rebuild)?;
        if !data.is_empty() {
            let pts = started.elapsed().as_micros() as u64;
            protocol::write_video(&mut out, pts, &data)?;
            out.flush()?;
        }
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
