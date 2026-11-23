//! Pairs a phone over WiFi with a QR code, like Android Studio does
//! (Android 11+: Developer options > Wireless debugging > Pair device with
//! QR code). The phone then mirrors without a cable.
//!
//! The QR code holds a service name and a password. The phone announces a
//! `_adb-tls-pairing` service with that name over mDNS; adb pairs with it,
//! then connects to the phone's `_adb-tls-connect` service.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::adb;
use crate::state::{Shared, WifiPairState};

/// How long the code stays valid.
const TIMEOUT: Duration = Duration::from_secs(180);

/// QR code modules, row by row; true = dark.
pub struct QrImage {
    pub width: usize,
    pub dark: Vec<bool>,
}

/// A pairing in progress; dropping it stops waiting.
pub struct Pairing {
    pub qr: QrImage,
    cancel: Arc<AtomicBool>,
}

impl Drop for Pairing {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Shows a fresh code and waits for a phone in the background.
pub fn start(shared: Arc<Shared>) -> Option<Pairing> {
    let adb = shared.status().adb_path.clone()?;
    let name = format!(
        "extendo-{}",
        random_text(6, b"abcdefghijkmnpqrstuvwxyz23456789")
    );
    let password = random_text(10, b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789");
    let text = format!("WIFI:T:ADB;S:{name};P:{password};;");
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let qr = QrImage {
        width: code.width(),
        dark: code
            .to_colors()
            .into_iter()
            .map(|c| c == qrcode::Color::Dark)
            .collect(),
    };
    let cancel = Arc::new(AtomicBool::new(false));
    shared.update_status(|s| s.wifi_pair = Some(WifiPairState::Waiting));
    {
        let cancel = cancel.clone();
        thread::Builder::new()
            .name("wifi-pair".into())
            .spawn(move || {
                let result = wait_and_pair(&shared, &adb, &name, &password, &cancel);
                // A closed dialog already cleared the state (or a new code owns it).
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let state = match result {
                    Ok(address) => WifiPairState::Connected(address),
                    Err(e) => {
                        log::warn!("WiFi pairing: {e}");
                        WifiPairState::Failed(e)
                    }
                };
                shared.update_status(|s| s.wifi_pair = Some(state));
            })
            .ok()?;
    }
    Some(Pairing { qr, cancel })
}

fn wait_and_pair(
    shared: &Shared,
    adb: &PathBuf,
    name: &str,
    password: &str,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let started = Instant::now();
    let pair_at = loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        if started.elapsed() > TIMEOUT {
            return Err("No phone scanned the code in time. Try again.".into());
        }
        if let Some((_, _, addr)) = services(adb)?
            .into_iter()
            .find(|(n, kind, _)| n == name && kind.contains("_adb-tls-pairing"))
        {
            break addr;
        }
        thread::sleep(Duration::from_secs(1));
    };
    shared.update_status(|s| s.wifi_pair = Some(WifiPairState::Pairing));
    log::info!("WiFi pairing: phone found at {pair_at}, pairing");
    let out =
        adb::run(adb, &["pair", &pair_at, password]).map_err(|e| format!("pairing failed: {e}"))?;
    if !out.contains("Successfully paired") {
        return Err(format!("pairing failed: {out}"));
    }
    // Same phone, other service: the port for debugging connections.
    let ip = pair_at
        .rsplit_once(':')
        .map(|(ip, _)| ip)
        .unwrap_or(&pair_at)
        .to_string();
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && !cancel.load(Ordering::Relaxed) {
        let connect_at = services(adb).ok().and_then(|list| {
            list.into_iter()
                .find(|(_, kind, addr)| {
                    kind.contains("_adb-tls-connect") && addr.starts_with(&format!("{ip}:"))
                })
                .map(|(_, _, addr)| addr)
        });
        if let Some(addr) = connect_at {
            // adb may already have connected by itself.
            let _ = adb::run(adb, &["connect", &addr]);
            log::info!("WiFi pairing: phone connected at {addr}");
            return Ok(addr);
        }
        thread::sleep(Duration::from_secs(1));
    }
    Ok(ip)
}

/// `adb mdns services`: (instance name, service type, ip:port).
fn services(adb: &PathBuf) -> Result<Vec<(String, String, String)>, String> {
    let out = adb::run(adb, &["mdns", "services"])
        .map_err(|e| format!("this adb can't find phones on WiFi (mdns): {e}"))?;
    Ok(out
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let (name, kind, addr) = (parts.next()?, parts.next()?, parts.next()?);
            addr.contains(':')
                .then(|| (name.to_string(), kind.to_string(), addr.to_string()))
        })
        .collect())
}

fn random_text(len: usize, alphabet: &[u8]) -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut bytes = vec![0u8; len];
    if SystemRandom::new().fill(&mut bytes).is_err() {
        // Practically never; still unique enough for a 3-minute code.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        bytes = t.to_le_bytes()[..len.min(16)].to_vec();
    }
    bytes
        .iter()
        .map(|b| alphabet[*b as usize % alphabet.len()] as char)
        .collect()
}
