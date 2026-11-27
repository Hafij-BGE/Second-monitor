//! Harbor Settings' shared glass look (spec: ~/Music/apps/glass-theme/PLANNER-SPEC.md).
//!
//! Resolve order, same as Harbor: ~/.config/harbor/glass-extendo (key=value
//! block, or empty/`harbor`/`global` = follow) -> ~/.config/harbor/glass (the
//! "all Harbor apps" scope; missing = off, `global`/empty = desktop) ->
//! Fusion Desk's glass-style/glass-effect GSettings, else glass-global.
//! The answer is cached for a few seconds so the window can ask every frame.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static CACHE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("harbor"))
}

/// `Some(is_planner)` for a key=value block, `None` for "follow" content.
fn block(text: &str) -> Option<bool> {
    let t = text.trim();
    if t.is_empty() || t == "harbor" || t == "global" || !t.contains('=') {
        return None;
    }
    let (mut effect, mut style) = ("STRONG".to_string(), "FROSTED".to_string());
    for line in t.lines() {
        if let Some((k, v)) = line.split_once('=') {
            let v = v.split_whitespace().next().unwrap_or("").to_uppercase();
            match k.trim() {
                "effect" => effect = v,
                "style" => style = v,
                _ => {}
            }
        }
    }
    Some(effect != "OFF" && style == "PLANNER")
}

#[cfg(unix)]
fn fusion() -> Option<bool> {
    let home = std::env::var_os("HOME")?;
    let schemas = PathBuf::from(home)
        .join(".local/share/gnome-shell/extensions/fusion-desk@hafij-deu.github.io/schemas");
    if !schemas.is_dir() {
        return None;
    }
    let get = |key: &str| -> Option<String> {
        let out = std::process::Command::new("gsettings")
            .arg("--schemadir")
            .arg(&schemas)
            .args(["get", "org.gnome.shell.extensions.fusion-desk", key])
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().trim_matches('\'').to_uppercase())
    };
    Some(get("glass-effect")? != "OFF" && get("glass-style")? == "PLANNER")
}

#[cfg(not(unix))]
fn fusion() -> Option<bool> {
    None
}

fn resolve() -> bool {
    if let Ok(v) = std::env::var("PLANNER_GLASS") {
        return v.trim().eq_ignore_ascii_case("planner") || block(&v).unwrap_or(false);
    }
    let Some(d) = dir() else { return false };
    let read = |name: &str| std::fs::read_to_string(d.join(name)).ok();
    if let Some(own) = read("glass-extendo").as_deref().and_then(block) {
        return own;
    }
    match read("glass") {
        None => false,
        Some(t) => match block(&t) {
            Some(v) => v,
            None => fusion()
                .or_else(|| read("glass-global").as_deref().and_then(block))
                .unwrap_or(false),
        },
    }
}

/// True while Harbor Settings resolves the Planner glass style for Extendo.
pub fn planner() -> bool {
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    match *c {
        Some((at, v)) if at.elapsed() < Duration::from_secs(3) => v,
        _ => {
            let v = resolve();
            *c = Some((Instant::now(), v));
            v
        }
    }
}

#[cfg(test)]
mod tests {
    use super::block;

    #[test]
    fn blocks() {
        assert_eq!(block("harbor"), None);
        assert_eq!(block(""), None);
        assert_eq!(block("effect=STRONG\nstyle=PLANNER\n"), Some(true));
        assert_eq!(block("effect=OFF\nstyle=PLANNER\n"), Some(false));
        assert_eq!(block("style=FROSTED"), Some(false));
    }
}
