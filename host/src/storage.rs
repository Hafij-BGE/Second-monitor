//! Phone storage: opens the Extendo app's FTP server in the PC's file
//! manager, over WiFi or through the USB cable (adb forward).

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::adb;
use crate::state::{Shared, StoragePhone};

pub const FTP_PORT: u16 = 2121;
/// Passive data ports the phone uses; all are forwarded over USB.
pub const PASSIVE_PORTS: std::ops::RangeInclusive<u16> = 2122..=2129;
pub const FTP_USER: &str = "extendo";
const DISCOVERY_PORT: u16 = 52022;
const QUERY: &[u8] = b"SMON_FTP?";

/// Looks for phones with "Phone storage" switched on.
pub fn search(shared: Arc<Shared>) {
    shared.update_status(|s| s.storage_searching = true);
    thread::spawn(move || {
        let found = discover().unwrap_or_else(|e| {
            log::warn!("phone storage search failed: {e}");
            Vec::new()
        });
        if found.is_empty() {
            log::info!("no phone with storage sharing found on WiFi");
        }
        shared.update_status(|s| {
            s.storage_phones = found;
            s.storage_searching = false;
        });
    });
}

fn discover() -> std::io::Result<Vec<StoragePhone>> {
    let socket = UdpSocket::bind(("0.0.0.0", 0))?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(Duration::from_millis(200)))?;
    let mut targets: Vec<IpAddr> = vec![IpAddr::V4(Ipv4Addr::BROADCAST)];
    for iface in if_addrs::get_if_addrs().unwrap_or_default() {
        if let if_addrs::IfAddr::V4(v4) = iface.addr {
            if let Some(b) = v4.broadcast {
                targets.push(IpAddr::V4(b));
            }
        }
    }
    for t in &targets {
        let _ = socket.send_to(QUERY, SocketAddr::new(*t, DISCOVERY_PORT));
    }
    let mut phones: Vec<StoragePhone> = Vec::new();
    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut buf = [0u8; 256];
    while Instant::now() < deadline {
        let Ok((n, from)) = socket.recv_from(&mut buf) else {
            continue;
        };
        let text = String::from_utf8_lossy(&buf[..n]);
        let mut parts = text.trim().splitn(3, ' ');
        if parts.next() != Some("SMON_FTP") {
            continue;
        }
        let port = parts
            .next()
            .and_then(|p| p.parse().ok())
            .unwrap_or(FTP_PORT);
        let name = parts.next().unwrap_or("Phone").to_string();
        let address = from.ip().to_string();
        if !phones.iter().any(|p| p.address == address) {
            phones.push(StoragePhone {
                name,
                address,
                port,
            });
        }
    }
    Ok(phones)
}

/// Forwards the FTP ports over USB and opens the phone's files.
pub fn open_usb(shared: Arc<Shared>, serial: String) {
    let Some(adb) = shared.status().adb_path.clone() else {
        return;
    };
    thread::spawn(move || {
        let ports = std::iter::once(FTP_PORT).chain(PASSIVE_PORTS);
        for port in ports {
            let spec = format!("tcp:{port}");
            if let Err(e) = adb::run(&adb, &["-s", &serial, "forward", &spec, &spec]) {
                log::warn!("cannot reach phone storage over USB: {e}");
                return;
            }
        }
        // Over the cable we can read the password from the phone's
        // notification, so the file manager doesn't ask for it.
        let password = phone_password(&adb, &serial);
        if password.is_none() {
            log::warn!("phone storage isn't on: open Extendo on the phone, then Phone storage");
        }
        open_url(&format!("127.0.0.1:{FTP_PORT}"), password.as_deref());
    });
}

fn phone_password(adb: &std::path::PathBuf, serial: &str) -> Option<String> {
    let out = adb::run(
        adb,
        &[
            "-s",
            serial,
            "shell",
            "dumpsys",
            "notification",
            "--noredact",
        ],
    )
    .ok()?;
    let at = out.find("Sharing phone storage")?;
    let rest = &out[at..];
    let p = rest.find("password ")? + "password ".len();
    let password: String = rest[p..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    (password.len() >= 6).then_some(password)
}

/// Opens `ftp://extendo@address/` in the file manager.
pub fn open(address: &str) {
    open_url(address, None);
}

fn open_url(address: &str, password: Option<&str>) {
    let address = if address.contains(':') {
        address.to_string()
    } else {
        format!("{address}:{FTP_PORT}")
    };
    let url = format!("ftp://{FTP_USER}@{address}/");
    log::info!("opening {url} in the file manager");
    #[cfg(windows)]
    {
        // Explorer accepts the password in the URL.
        let login = match password {
            Some(p) => format!("{FTP_USER}:{p}"),
            None => FTP_USER.to_string(),
        };
        crate::desktop::open_in_file_manager(&format!("ftp://{login}@{address}/"));
    }
    #[cfg(not(windows))]
    {
        // GNOME/KDE ignore passwords in URLs; mount first so the file
        // manager opens without asking. Without a password it asks itself.
        if let Some(p) = password {
            mount_gvfs(&url, p);
        }
        crate::desktop::open_in_file_manager(&url);
    }
}

#[cfg(not(windows))]
fn mount_gvfs(url: &str, password: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let child = Command::new("gio")
        .args(["mount", url])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{password}");
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
}
