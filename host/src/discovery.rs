//! Answers WiFi discovery broadcasts from the Android app.

use std::net::UdpSocket;
use std::thread;

use crate::protocol::{DISCOVERY_PORT, DISCOVERY_QUERY};

pub fn spawn(tcp_port: u16) {
    thread::Builder::new()
        .name("discovery".into())
        .spawn(move || {
            let socket = match UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("WiFi discovery disabled: cannot bind UDP {DISCOVERY_PORT}: {e}");
                    return;
                }
            };
            let reply = format!("SMON_HOST {tcp_port} {}", host_name());
            log::info!("WiFi discovery listening on UDP {DISCOVERY_PORT}");
            let mut buf = [0u8; 64];
            loop {
                match socket.recv_from(&mut buf) {
                    Ok((n, from)) if buf[..n].starts_with(DISCOVERY_QUERY) => {
                        log::debug!("discovery query from {from}");
                        let _ = socket.send_to(reply.as_bytes(), from);
                    }
                    Ok(_) => {}
                    Err(e) => log::warn!("discovery socket error: {e}"),
                }
            }
        })
        .expect("spawn discovery thread");
}

fn host_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Second Monitor host".into())
        .replace(char::is_whitespace, "-")
}
