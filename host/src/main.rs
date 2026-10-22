// No console window for the GUI on Windows (--headless re-attaches one).
#![cfg_attr(windows, windows_subsystem = "windows")]

mod adb;
mod backend;
mod background;
mod clipboard;
mod desktop;
mod discovery;
mod encoder;
mod files;
mod gui;
mod logging;
mod mirror;
mod protocol;
mod scale;
mod server;
mod state;
mod storage;
mod tls;

use anyhow::Result;
use clap::Parser;

use backend::Mode;
use state::{Settings, Shared};

/// Use an Android phone or tablet as a second monitor over USB or WiFi.
///
/// Opens a window by default. Options given here override the saved
/// settings for this run.
#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    /// Run without a window (logs to the terminal).
    #[arg(long)]
    headless: bool,

    /// Start with the window closed; Extendo keeps running in the tray.
    #[arg(long, conflicts_with = "headless")]
    background: bool,

    /// List monitors and exit.
    #[arg(long)]
    list: bool,

    /// TCP port the Android app connects to.
    #[arg(long)]
    port: Option<u16>,

    /// extend: add a virtual monitor. mirror: show an existing monitor.
    #[arg(long, value_enum)]
    mode: Option<Mode>,

    /// Monitor to capture: connector name on Linux (e.g. eDP-1), index on Windows. See --list.
    #[arg(long)]
    monitor: Option<String>,

    /// Default frames per second.
    #[arg(long)]
    fps: Option<u32>,

    /// Default video bitrate in Mbit/s.
    #[arg(long)]
    bitrate: Option<u32>,

    /// Divide the device resolution by this (1.5 makes text bigger).
    #[arg(long)]
    scale: Option<f32>,

    /// Don't set up USB (adb reverse).
    #[arg(long)]
    no_usb: bool,

    /// Don't accept WiFi connections.
    #[arg(long)]
    no_discovery: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    platform_init(args.headless || args.list);
    logging::init();

    if args.list {
        for m in backend::list_monitors()? {
            println!("{m}");
        }
        return Ok(());
    }

    // One copy runs at a time; starting Extendo again opens its window.
    if !args.headless {
        if let background::Instance::AlreadyRunning = background::claim(!args.background) {
            log::info!("Extendo is already running; opening its window");
            return Ok(());
        }
    }

    let disk = Settings::load();
    let mut settings = disk.clone();
    if let Some(v) = args.port {
        settings.port = v;
    }
    if let Some(v) = args.mode {
        settings.mode = v;
    }
    if args.monitor.is_some() {
        settings.monitor = args.monitor.clone();
    }
    if let Some(v) = args.fps {
        settings.fps = v;
    }
    if let Some(v) = args.bitrate {
        settings.bitrate_mbps = v;
    }
    if let Some(v) = args.scale {
        settings.scale = v;
    }
    if args.no_usb {
        settings.usb = false;
    }
    if args.no_discovery {
        settings.wifi = false;
    }
    let overridden = args.port.is_some()
        || args.mode.is_some()
        || args.monitor.is_some()
        || args.fps.is_some()
        || args.bitrate.is_some()
        || args.scale.is_some()
        || args.no_usb
        || args.no_discovery;
    if overridden {
        settings.on_disk = Some(Box::new(disk));
    }

    let shared = Shared::new(settings);
    logging::attach(shared.clone());
    server::start(shared.clone());
    mirror::detect(shared.clone());
    mirror::reconnect_saved(shared.clone());

    if !args.headless {
        return run_windowed(&shared, !args.background);
    }
    if let Some(err) = shared.status().server_error.clone() {
        anyhow::bail!(err);
    }
    loop {
        std::thread::park();
    }
}

/// Shows the window, and keeps serving tablets after it closes when
/// "Keep running in the background" is on.
fn run_windowed(shared: &std::sync::Arc<Shared>, mut show: bool) -> Result<()> {
    // Pairing requests can open the window, so WiFi tablets can still ask.
    shared
        .has_gui
        .store(true, std::sync::atomic::Ordering::Relaxed);
    background::start_tray();
    loop {
        if show {
            if let Err(e) = gui::run(shared.clone()) {
                log::error!("cannot open the window ({e})");
            }
            // Requests the closed window didn't get to are stale.
            let _ = background::take();
            if background::quit_requested() || !shared.settings().run_in_background {
                break;
            }
            if let Some(err) = shared.status().server_error.clone() {
                background::stop_tray();
                anyhow::bail!(err);
            }
            log::info!("window closed; Extendo keeps running in the background");
        }
        match background::wait() {
            background::Request::Show => show = true,
            background::Request::Quit => break,
        }
    }
    background::stop_tray();
    Ok(())
}

#[cfg(windows)]
fn platform_init(console: bool) {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    unsafe {
        if console {
            // Show output when started from a terminal.
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
        // Real pixel coordinates for capture and input on scaled displays.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

#[cfg(not(windows))]
fn platform_init(_console: bool) {}
