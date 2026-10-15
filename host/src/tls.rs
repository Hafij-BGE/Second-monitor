//! TLS for viewer connections.
//!
//! The PC has a long-lived self-signed certificate. The tablet pins its
//! fingerprint when it pairs, and both sides show a short code derived from
//! it, so a man in the middle shows up as a different code.
//!
//! rustls connections can't be cloned, but the session needs a reader thread
//! and several writer threads. [`split`] shares one connection behind a mutex
//! and only holds the lock while moving bytes, never while waiting on the
//! socket.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection};
use sha2::{Digest, Sha256};

/// The PC's TLS identity.
pub struct Identity {
    pub config: Arc<ServerConfig>,
    /// SHA-256 of the certificate.
    pub fingerprint: [u8; 32],
}

impl Identity {
    /// Six digits shown on both screens while pairing.
    pub fn pairing_code(&self) -> String {
        pairing_code(&self.fingerprint)
    }
}

pub fn pairing_code(fingerprint: &[u8; 32]) -> String {
    let n = u32::from_be_bytes([
        fingerprint[0],
        fingerprint[1],
        fingerprint[2],
        fingerprint[3],
    ]);
    format!("{:06}", n % 1_000_000)
}

fn identity_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("extendo"))
}

/// Loads the identity from the config folder, creating it on first run.
pub fn load_identity() -> Result<Identity> {
    let dir = identity_dir().context("no config folder")?;
    let cert_path = dir.join("identity.der");
    let key_path = dir.join("identity.key");
    let (cert, key) = match (std::fs::read(&cert_path), std::fs::read(&key_path)) {
        (Ok(c), Ok(k)) => (c, k),
        _ => {
            let generated = rcgen::generate_simple_self_signed(vec!["extendo".to_string()])
                .context("cannot create the PC's certificate")?;
            let cert = generated.cert.der().to_vec();
            let key = generated.signing_key.serialize_der();
            std::fs::create_dir_all(&dir)?;
            std::fs::write(&cert_path, &cert)?;
            write_private(&key_path, &key)?;
            log::info!("created this PC's connection key in {}", dir.display());
            (cert, key)
        }
    };
    let fingerprint: [u8; 32] = Sha256::digest(&cert).into();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(cert)],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
        )
        .context("invalid certificate")?;
    Ok(Identity {
        config: Arc::new(config),
        fingerprint,
    })
}

/// Writes the private key readable only by the current user.
fn write_private(path: &std::path::Path, data: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(data)
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, data)
    }
}

struct Shared {
    conn: Mutex<ServerConnection>,
    /// Write half of the socket (a clone), used under the lock.
    socket: TcpStream,
}

/// Plaintext reader for one TLS connection.
pub struct TlsReader {
    shared: Arc<Shared>,
    socket: TcpStream,
    buf: Vec<u8>,
}

/// Plaintext writer; clone it freely.
#[derive(Clone)]
pub struct TlsWriter {
    shared: Arc<Shared>,
}

/// Runs the TLS handshake and returns the two halves.
pub fn accept(
    mut socket: TcpStream,
    config: Arc<ServerConfig>,
) -> io::Result<(TlsReader, TlsWriter)> {
    let mut conn = ServerConnection::new(config).map_err(io::Error::other)?;
    while conn.is_handshaking() {
        conn.complete_io(&mut socket)?;
    }
    // Writers flush to the socket right away; don't cap buffered output.
    conn.set_buffer_limit(None);
    let shared = Arc::new(Shared {
        conn: Mutex::new(conn),
        socket: socket.try_clone()?,
    });
    Ok((
        TlsReader {
            shared: shared.clone(),
            socket,
            buf: vec![0u8; 64 * 1024],
        },
        TlsWriter { shared },
    ))
}

fn flush_tls(conn: &mut ServerConnection, mut socket: &TcpStream) -> io::Result<()> {
    while conn.wants_write() {
        conn.write_tls(&mut socket)?;
    }
    Ok(())
}

impl Read for TlsReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        loop {
            {
                let mut conn = self.shared.conn.lock().unwrap();
                match conn.reader().read(out) {
                    Ok(n) => return Ok(n),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(e),
                }
            }
            // Wait for the network without holding the lock.
            let n = self.socket.read(&mut self.buf)?;
            if n == 0 {
                return Ok(0);
            }
            let mut conn = self.shared.conn.lock().unwrap();
            let mut data = &self.buf[..n];
            while !data.is_empty() {
                conn.read_tls(&mut data)?;
                conn.process_new_packets().map_err(io::Error::other)?;
            }
            flush_tls(&mut conn, &self.shared.socket)?;
        }
    }
}

impl Write for TlsWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let mut conn = self.shared.conn.lock().unwrap();
        conn.writer().write_all(data)?;
        flush_tls(&mut conn, &self.shared.socket)?;
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut conn = self.shared.conn.lock().unwrap();
        flush_tls(&mut conn, &self.shared.socket)
    }
}
