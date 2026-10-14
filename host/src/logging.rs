//! Logs to stderr and to the GUI's log panel.

use std::sync::{Arc, OnceLock};

use log::{Level, LevelFilter, Metadata, Record};

use crate::state::Shared;

struct Logger {
    level: LevelFilter,
    shared: OnceLock<Arc<Shared>>,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

/// `RUST_LOG=debug` (or `extendo=debug`) shows timing stats.
pub fn init() {
    let level = match std::env::var("RUST_LOG").unwrap_or_default() {
        s if s.contains("trace") => LevelFilter::Trace,
        s if s.contains("debug") => LevelFilter::Debug,
        s if s.contains("warn") => LevelFilter::Warn,
        s if s.contains("error") => LevelFilter::Error,
        _ => LevelFilter::Info,
    };
    let logger = LOGGER.get_or_init(|| Logger {
        level,
        shared: OnceLock::new(),
    });
    if log::set_logger(logger).is_ok() {
        log::set_max_level(level);
    }
}

/// Starts copying log lines into the GUI.
pub fn attach(shared: Arc<Shared>) {
    if let Some(logger) = LOGGER.get() {
        let _ = logger.shared.set(shared);
    }
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        // Dependencies (zbus, wgpu, ...) are chatty: only their warnings.
        let limit = if metadata.target().starts_with("extendo") {
            self.level
        } else {
            self.level.min(LevelFilter::Warn)
        };
        metadata.level() <= limit
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let time = time_of_day();
        eprintln!(
            "[{time} {:5} {}] {}",
            record.level(),
            record.target(),
            record.args()
        );
        if record.level() <= Level::Info {
            if let Some(shared) = self.shared.get() {
                shared.push_log(format!("{time}  {}", record.args()));
            }
        }
    }

    fn flush(&self) {}
}

/// UTC HH:MM:SS without pulling in a time crate.
fn time_of_day() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        % 86_400;
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}
