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
        open(&format!("127.0.0.1:{FTP_PORT}"));
    });
}

/// Opens `ftp://extendo@address/` in the file manager.
pub fn open(address: &str) {
    let address = if address.contains(':') {
        address.to_string()
    } else {
        format!("{address}:{FTP_PORT}")
    };
    let url = format!("ftp://{FTP_USER}@{address}/");
    log::info!("opening {url} (use the password shown in the Extendo app)");
    let program = if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(program).arg(&url).spawn() {
        log::warn!("cannot open {url}: {e}");
    }
}
