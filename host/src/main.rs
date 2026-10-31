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

use anyhow::{bail, Context, Result};

use backend::Mode;
use state::{Settings, Shared};

const HELP: &str = "\
Use an Android phone or tablet as a second monitor over USB or WiFi.

Opens a window by default. Options given here override the saved
settings for this run.

Usage: extendo [OPTIONS]

Options:
      --headless           Run without a window (logs to the terminal)
      --background         Start with the window closed; Extendo keeps running in the tray
      --list               List monitors and exit
      --port <PORT>        TCP port the Android app connects to
      --mode <MODE>        extend: add a virtual monitor. mirror: show an existing monitor
      --monitor <MONITOR>  Monitor to capture: connector name on Linux (e.g. eDP-1),
                           index on Windows. See --list
      --fps <FPS>          Default frames per second
      --bitrate <MBIT>     Default video bitrate in Mbit/s
      --scale <SCALE>      Divide the device resolution by this (1.5 makes text bigger)
      --no-usb             Don't set up USB (adb reverse)
      --no-discovery       Don't accept WiFi connections
  -h, --help               Print help
  -V, --version            Print version
";

#[derive(Debug, Default)]
struct Args {
    headless: bool,
    background: bool,
    list: bool,
    port: Option<u16>,
    mode: Option<Mode>,
    monitor: Option<String>,
    fps: Option<u32>,
    bitrate: Option<u32>,
    scale: Option<f32>,
    no_usb: bool,
    no_discovery: bool,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut args = Args::default();
        let mut it = std::env::args().skip(1);
        while let Some(arg) = it.next() {
            let (name, inline) = match arg.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (arg, None),
            };
            let mut value = || -> Result<String> {
                inline
                    .clone()
                    .or_else(|| it.next())
                    .with_context(|| format!("{name} needs a value"))
            };
            fn num<T: std::str::FromStr>(name: &str, v: String) -> Result<T> {
                v.parse().ok().with_context(|| format!("invalid value '{v}' for {name}"))
            }
            match name.as_str() {
                "--headless" => args.headless = true,
                "--background" => args.background = true,
                "--list" => args.list = true,
                "--no-usb" => args.no_usb = true,
                "--no-discovery" => args.no_discovery = true,
                "--port" => args.port = Some(num(&name, value()?)?),
                "--fps" => args.fps = Some(num(&name, value()?)?),
                "--bitrate" => args.bitrate = Some(num(&name, value()?)?),
                "--scale" => args.scale = Some(num(&name, value()?)?),
                "--monitor" => args.monitor = Some(value()?),
                "--mode" => {
                    args.mode = Some(match value()?.as_str() {
                        "extend" => Mode::Extend,
                        "mirror" => Mode::Mirror,
                        v => bail!("invalid value '{v}' for --mode (extend or mirror)"),
                    })
                }
                "-h" | "--help" => {
                    print!("{HELP}");
                    std::process::exit(0);
                }
                "-V" | "--version" => {
                    println!("extendo {}", env!("CARGO_PKG_VERSION"));
                    std::process::exit(0);
                }
                _ => bail!("unexpected argument '{name}'; see --help"),
            }
        }
        if args.headless && args.background {
            bail!("--background can't be used with --headless");
        }
        Ok(args)
    }
}

fn main() -> Result<()> {
    let args = Args::parse()?;
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
