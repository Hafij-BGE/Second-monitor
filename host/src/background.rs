//! Running in the background: Extendo keeps serving tablets after its
//! window closes. A tray icon and starting Extendo again open the window;
//! "Start at login" launches it with `--background`.

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};

/// Something asked the running copy to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Show,
    Quit,
}

#[derive(Default)]
struct Control {
    queue: Mutex<Vec<Request>>,
    ready: Condvar,
    /// The open window, woken up when a request arrives.
    window: Mutex<Option<egui::Context>>,
}

fn control() -> &'static Control {
    static CONTROL: OnceLock<Control> = OnceLock::new();
    CONTROL.get_or_init(Control::default)
}

static QUIT: AtomicBool = AtomicBool::new(false);

/// Set once the user chose Quit (tray or window).
pub fn quit_requested() -> bool {
    QUIT.load(Ordering::Relaxed)
}

pub fn send(request: Request) {
    if request == Request::Quit {
        QUIT.store(true, Ordering::Relaxed);
    }
    let c = control();
    c.queue.lock().unwrap().push(request);
    c.ready.notify_all();
    if let Some(ctx) = c.window.lock().unwrap().as_ref() {
        ctx.request_repaint();
    }
}

/// The GUI registers its context while open so requests wake it up.
pub fn set_window(ctx: Option<egui::Context>) {
    *control().window.lock().unwrap() = ctx;
}

/// Requests that arrived since the last call (for the open window).
pub fn take() -> Vec<Request> {
    std::mem::take(&mut *control().queue.lock().unwrap())
}

/// Blocks while the window is closed until something asks for it.
pub fn wait() -> Request {
    let c = control();
    let mut queue = c.queue.lock().unwrap();
    loop {
        if quit_requested() {
            return Request::Quit;
        }
        if !queue.is_empty() {
            queue.clear();
            return Request::Show;
        }
        queue = c.ready.wait(queue).unwrap();
    }
}

// ---------------------------------------------------------------------------
// One running copy

/// Outcome of looking for a copy of Extendo that is already running.
pub enum Instance {
    /// No other copy; this one listens for later launches.
    First,
    /// Another copy is running and was asked to show its window.
    AlreadyRunning,
}

/// Becomes the running copy, or hands over to the one already running.
/// `show` asks that copy to open its window.
pub fn claim(show: bool) -> Instance {
    let message: &[u8] = if show { b"show\n" } else { b"ping\n" };
    if ipc::connect()
        .and_then(|mut s| s.write_all(message).map_err(Into::into))
        .is_ok()
    {
        return Instance::AlreadyRunning;
    }
    match ipc::listen() {
        Ok(()) => {}
        Err(e) => log::warn!("starting Extendo again won't open this window: {e:#}"),
    }
    Instance::First
}

fn handle_client(stream: impl Read) {
    let mut line = String::new();
    let mut reader = BufReader::new(stream.take(64));
    if reader.read_line(&mut line).is_ok() && line.trim() == "show" {
        send(Request::Show);
    }
}

#[cfg(unix)]
mod ipc {
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;

    use super::*;

    fn path() -> PathBuf {
        let dir = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
        // The user id keeps users apart when there is no runtime dir.
        let uid = std::fs::metadata(dirs::home_dir().unwrap_or_default())
            .map(|m| std::os::unix::fs::MetadataExt::uid(&m))
            .unwrap_or(0);
        dir.join(format!("extendo-{uid}.sock"))
    }

    pub fn connect() -> Result<UnixStream> {
        let s = UnixStream::connect(path())?;
        s.set_write_timeout(Some(Duration::from_secs(1)))?;
        Ok(s)
    }

    pub fn listen() -> Result<()> {
        let path = path();
        // Nobody answered, so a leftover socket is stale.
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).with_context(|| format!("{}", path.display()))?;
        std::thread::Builder::new()
            .name("instance".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                    handle_client(stream);
                }
            })?;
        Ok(())
    }
}

#[cfg(windows)]
mod ipc {
    use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};

    use super::*;

    /// Local-only control port (next to the discovery and storage ports).
    const PORT: u16 = 52023;

    pub fn connect() -> Result<TcpStream> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, PORT));
        let s = TcpStream::connect_timeout(&addr, Duration::from_millis(300))?;
        s.set_write_timeout(Some(Duration::from_secs(1)))?;
        Ok(s)
    }

    pub fn listen() -> Result<()> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, PORT))
            .with_context(|| format!("port {PORT}"))?;
        std::thread::Builder::new()
            .name("instance".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                    handle_client(stream);
                }
            })?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tray icon

/// Shows the tray icon (Open Extendo / Quit). Without a tray (GNOME needs
/// the AppIndicator extension), starting Extendo again opens the window.
pub fn start_tray() {
    if let Err(e) = tray::start() {
        log::info!("no tray icon ({e:#}); open Extendo from the app menu to see the window");
    }
}

/// Removes the tray icon right away when quitting.
pub fn stop_tray() {
    tray::stop();
}

/// Square RGBA icon scaled down from the app icon.
#[cfg(target_os = "linux")]
fn icon_rgba(size: u32) -> Option<Vec<u8>> {
    let icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../../assets/extendo-256.png")).ok()?;
    let (w, h) = (icon.width, icon.height);
    let step = (w / size).max(1);
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            // Box filter over the source pixels this one covers.
            let mut sum = [0u32; 4];
            let mut n = 0;
            for sy in y * step..((y + 1) * step).min(h) {
                for sx in x * step..((x + 1) * step).min(w) {
                    let i = ((sy * w + sx) * 4) as usize;
                    for (total, v) in sum.iter_mut().zip(&icon.rgba[i..i + 4]) {
                        *total += *v as u32;
                    }
                    n += 1;
                }
            }
            let n = n.max(1);
            out.extend(sum.iter().map(|v| (v / n) as u8));
        }
    }
    Some(out)
}

#[cfg(target_os = "linux")]
mod tray {
    use std::sync::Mutex;

    use ksni::blocking::{Handle, TrayMethods};
    use ksni::menu::StandardItem;

    use super::*;

    static HANDLE: Mutex<Option<Handle<ExtendoTray>>> = Mutex::new(None);

    struct ExtendoTray {
        icon: Vec<ksni::Icon>,
    }

    impl ksni::Tray for ExtendoTray {
        fn id(&self) -> String {
            "extendo".into()
        }

        fn title(&self) -> String {
            "Extendo".into()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            self.icon.clone()
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: "Extendo".into(),
                description: "Running in the background".into(),
                ..Default::default()
            }
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            send(Request::Show);
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            vec![
                StandardItem {
                    label: "Open Extendo".into(),
                    activate: Box::new(|_| send(Request::Show)),
                    ..Default::default()
                }
                .into(),
                ksni::MenuItem::Separator,
                StandardItem {
                    label: "Quit".into(),
                    activate: Box::new(|_| send(Request::Quit)),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    pub fn start() -> Result<()> {
        let icon = [64u32, 32]
            .into_iter()
            .filter_map(|size| {
                let rgba = icon_rgba(size)?;
                // ARGB32 in network byte order.
                let data = rgba
                    .chunks_exact(4)
                    .flat_map(|p| [p[3], p[0], p[1], p[2]])
                    .collect();
                Some(ksni::Icon {
                    width: size as i32,
                    height: size as i32,
                    data,
                })
            })
            .collect();
        // The panel may come up after Extendo when started at login.
        let handle = ExtendoTray { icon }
            .assume_sni_available(true)
            .spawn()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        *HANDLE.lock().unwrap() = Some(handle);
        Ok(())
    }

    pub fn stop() {
        if let Some(handle) = HANDLE.lock().unwrap().take() {
            handle.shutdown().wait();
        }
    }
}

#[cfg(windows)]
mod tray {
    use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    };
    use windows::Win32::UI::WindowsAndMessaging::*;

    use super::*;

    const CALLBACK: u32 = WM_APP + 1;
    const OPEN: usize = 1;
    const QUIT: usize = 2;

    static WINDOW: AtomicIsize = AtomicIsize::new(0);
    static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

    fn data(hwnd: HWND) -> NOTIFYICONDATAW {
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            ..Default::default()
        };
        for (dst, src) in nid.szTip.iter_mut().zip("Extendo".encode_utf16()) {
            *dst = src;
        }
        nid
    }

    fn add_icon(hwnd: HWND) -> bool {
        let mut nid = data(hwnd);
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = CALLBACK;
        unsafe {
            let module = GetModuleHandleW(None).unwrap_or_default();
            let size = GetSystemMetrics(SM_CXSMICON);
            // The .exe icon has resource id 1 (see build.rs).
            nid.hIcon = LoadImageW(
                Some(module.into()),
                // MAKEINTRESOURCE(1)
                PCWSTR(std::ptr::without_provenance(1)),
                IMAGE_ICON,
                size,
                size,
                LR_DEFAULTCOLOR,
            )
            .map(|h| HICON(h.0))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .unwrap_or_default();
            Shell_NotifyIconW(NIM_ADD, &nid).as_bool()
        }
    }

    fn show_menu(hwnd: HWND) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                return;
            };
            let _ = AppendMenuW(menu, MF_STRING, OPEN, w!("Open Extendo"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, QUIT, w!("Quit"));
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            // Lets the menu close when clicking elsewhere.
            let _ = SetForegroundWindow(hwnd);
            let cmd = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                pt.x,
                pt.y,
                Some(0),
                hwnd,
                None,
            );
            let _ = DestroyMenu(menu);
            match cmd.0 as usize {
                OPEN => send(Request::Show),
                QUIT => send(Request::Quit),
                _ => {}
            }
        }
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == CALLBACK {
            match (lparam.0 & 0xffff) as u32 {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => send(Request::Show),
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
            return LRESULT(0);
        }
        if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) {
            // Explorer restarted; put the icon back.
            add_icon(hwnd);
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    pub fn start() -> Result<()> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("tray".into())
            .spawn(move || unsafe {
                let module = GetModuleHandleW(None).unwrap_or_default();
                let class = w!("ExtendoTray");
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(window_proc),
                    hInstance: module.into(),
                    lpszClassName: class,
                    ..Default::default()
                };
                RegisterClassW(&wc);
                TASKBAR_CREATED.store(
                    RegisterWindowMessageW(w!("TaskbarCreated")),
                    Ordering::Relaxed,
                );
                // A hidden window that receives the icon's clicks.
                let hwnd = match CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    class,
                    w!("Extendo"),
                    WS_OVERLAPPED,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    Some(module.into()),
                    None,
                ) {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = tx.send(Err(anyhow::anyhow!("{e}")));
                        return;
                    }
                };
                if !add_icon(hwnd) {
                    let _ = tx.send(Err(anyhow::anyhow!("the taskbar refused the icon")));
                    let _ = DestroyWindow(hwnd);
                    return;
                }
                WINDOW.store(hwnd.0 as isize, Ordering::Relaxed);
                let _ = tx.send(Ok(()));
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            })?;
        rx.recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("timed out")))
    }

    pub fn stop() {
        let hwnd = WINDOW.swap(0, Ordering::Relaxed);
        if hwnd != 0 {
            let nid = data(HWND(hwnd as *mut _));
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            }
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod tray {
    use super::*;

    pub fn start() -> Result<()> {
        anyhow::bail!("not supported here")
    }

    pub fn stop() {}
}

// ---------------------------------------------------------------------------
// Start at login

/// The command that starts this copy of Extendo in the background.
fn launch_path() -> Result<std::path::PathBuf> {
    // An AppImage runs from a temporary mount; start the AppImage itself.
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Ok(appimage.into());
    }
    std::env::current_exe().context("can't find extendo's own path")
}

#[cfg(target_os = "linux")]
mod login {
    use std::path::PathBuf;

    use super::*;

    fn entry() -> Option<PathBuf> {
        Some(
            dirs::config_dir()?
                .join("autostart")
                .join("extendo.desktop"),
        )
    }

    pub fn enabled() -> bool {
        entry().is_some_and(|p| p.is_file())
    }

    pub fn set(on: bool) -> Result<()> {
        let path = entry().context("no config directory")?;
        if !on {
            match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => return Ok(()),
            }
        }
        let exe = launch_path()?;
        // Desktop entry quoting: backslash, quote, backtick and dollar.
        let mut quoted = String::from('"');
        for c in exe.to_string_lossy().chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        quoted.push('"');
        let text = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Extendo\n\
             Comment=Use an Android tablet as a second monitor\n\
             Exec={quoted} --background\n\
             Icon=extendo\n\
             Terminal=false\n\
             X-GNOME-Autostart-enabled=true\n"
        );
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, text).with_context(|| format!("{}", path.display()))?;
        Ok(())
    }
}

#[cfg(windows)]
mod login {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
    };

    use super::*;

    const KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
    const VALUE: PCWSTR = w!("Extendo");

    pub fn enabled() -> bool {
        let mut size = 0u32;
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                KEY,
                VALUE,
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&mut size),
            )
            .is_ok()
        }
    }

    pub fn set(on: bool) -> Result<()> {
        unsafe {
            if !on {
                let r = RegDeleteKeyValueW(HKEY_CURRENT_USER, KEY, VALUE);
                // ERROR_FILE_NOT_FOUND: already off.
                if r.is_err() && r.0 != 2 {
                    anyhow::bail!("can't remove Extendo from startup ({})", r.to_hresult());
                }
                return Ok(());
            }
            let command = format!("\"{}\" --background", launch_path()?.display());
            let wide: Vec<u16> = command.encode_utf16().chain([0]).collect();
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                KEY,
                VALUE,
                REG_SZ.0,
                Some(wide.as_ptr().cast()),
                (wide.len() * 2) as u32,
            )
            .ok()
            .context("can't add Extendo to startup")?;
        }
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod login {
    use super::*;

    pub fn enabled() -> bool {
        false
    }

    pub fn set(_on: bool) -> Result<()> {
        anyhow::bail!("not supported here")
    }
}

/// Whether Extendo starts (in the background) when the user logs in.
pub fn start_at_login() -> bool {
    login::enabled()
}

pub fn set_start_at_login(on: bool) -> Result<()> {
    login::set(on)
}
