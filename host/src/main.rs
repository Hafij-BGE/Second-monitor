// No console window for the GUI on Windows (--headless re-attaches one).
#![cfg_attr(windows, windows_subsystem = "windows")]

mod adb;
mod backend;
mod clipboard;
mod discovery;
mod encoder;
mod files;
mod gui;
mod logging;
mod protocol;
mod scale;
mod server;
mod state;

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

    let mut settings = Settings::load();
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

    let shared = Shared::new(settings);
    logging::attach(shared.clone());
    server::start(shared.clone());

    if !args.headless {
        match gui::run(shared.clone()) {
            Ok(()) => return Ok(()),
            Err(e) => log::error!("cannot open the window ({e}); running in the background"),
        }
    }
    if let Some(err) = shared.status().server_error.clone() {
        anyhow::bail!(err);
    }
    loop {
        std::thread::park();
    }
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
