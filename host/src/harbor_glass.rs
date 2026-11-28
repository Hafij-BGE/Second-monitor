//! Harbor Settings' shared glass look (spec: ~/Music/apps/glass-theme/PLANNER-SPEC.md).
//!
//! Resolve order, same as Harbor: ~/.config/harbor/glass-extendo (key=value
//! block, or empty/`harbor`/`global` = follow) -> ~/.config/harbor/glass (the
//! old "all Harbor apps" scope; a block is used, anything else follows) ->
//! Fusion Desk's glass GSettings, else glass-global.
//! The answer is cached for a few seconds so the window can ask every frame.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// What Harbor Settings resolves for Extendo.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Look {
    /// Lumina Glass (the Planner style) is on.
    pub planner: bool,
    /// Lumina Colourful: the three colours of the faint backdrop tint.
    pub colours: Option<[[u8; 3]; 3]>,
    /// Colour intensity, 0.2 to 1.0.
    pub intensity: f32,
}

static CACHE: Mutex<Option<(Instant, Look)>> = Mutex::new(None);

fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("harbor"))
}

/// The key=value pairs of a block, or `None` for "follow" content.
fn pairs(text: &str) -> Option<Vec<(String, String)>> {
    let t = text.trim();
    if t.is_empty() || t == "harbor" || t == "global" || !t.contains('=') {
        return None;
    }
    Some(
        t.lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| {
                let v = v.split_whitespace().next().unwrap_or("");
                (k.trim().to_string(), v.trim_matches('\'').to_string())
            })
            .collect(),
    )
}

fn get<'a>(p: &'a [(String, String)], key: &str) -> Option<&'a str> {
    p.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// `Some(is_planner)` for a key=value block, `None` for "follow" content.
#[cfg(test)]
fn block(text: &str) -> Option<bool> {
    pairs(text).map(|p| look(&p).planner)
}

fn look(p: &[(String, String)]) -> Look {
    let up = |k: &str, d: &str| get(p, k).unwrap_or(d).to_uppercase();
    let planner = up("effect", "STRONG") != "OFF" && up("style", "FROSTED") == "PLANNER";
    let colourful = planner && up("lumina", "CALM") == "COLOURFUL";
    let intensity = get(p, "lumina-intensity")
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(70.0)
        .clamp(20.0, 100.0)
        / 100.0;
    let colours = colourful.then(|| {
        lumina_colours(
            &up("lumina-colours", "AURORA"),
            get(p, "lumina-colour").unwrap_or("#8b5cf6"),
        )
    });
    Look { planner, colours, intensity }
}

fn hex(v: &str) -> Option<[u8; 3]> {
    let v = v.trim().trim_start_matches('#');
    if v.len() != 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&v[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

/// The colour set's three colours (same values as planner_glass.py).
fn lumina_colours(set: &str, custom: &str) -> [[u8; 3]; 3] {
    let h = |s: &str| hex(s).unwrap_or([0x8b, 0x5c, 0xf6]);
    let fixed = |a, b, c| [h(a), h(b), h(c)];
    match set {
        "SUNSET" => fixed("#f97316", "#ec4899", "#8b5cf6"),
        "OCEAN" => fixed("#06b6d4", "#3b82f6", "#6366f1"),
        "CANDY" => fixed("#f472b6", "#a78bfa", "#34d399"),
        "FOREST" => fixed("#22c55e", "#14b8a6", "#0ea5e9"),
        "ACCENT" => triad(system_accent()),
        "CUSTOM" => triad(h(custom)),
        _ => fixed("#ec4899", "#8b5cf6", "#38bdf8"),
    }
}

/// GNOME's accent colour, blue when unknown.
fn system_accent() -> [u8; 3] {
    #[cfg(unix)]
    {
        let name = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "accent-color"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_matches('\'').to_string())
            .unwrap_or_default();
        match name.as_str() {
            "teal" => return [0x21, 0x90, 0xa4],
            "green" => return [0x3a, 0x94, 0x4a],
            "yellow" => return [0xc8, 0x88, 0x00],
            "orange" => return [0xed, 0x5b, 0x00],
            "red" => return [0xe6, 0x2d, 0x42],
            "pink" => return [0xd5, 0x61, 0x99],
            "purple" => return [0x91, 0x41, 0xac],
            "slate" => return [0x6f, 0x83, 0x96],
            _ => {}
        }
    }
    [0x35, 0x84, 0xe4]
}

/// One colour as three neighbours: hue -32, 0, +32 degrees.
fn triad(rgb: [u8; 3]) -> [[u8; 3]; 3] {
    let [r, g, b] = rgb.map(|v| v as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let l0 = (max + min) / 2.0;
    let s = if d == 0.0 { 0.0 } else { d / (1.0 - (2.0 * l0 - 1.0).abs()) }.max(0.55);
    let l = l0.clamp(0.45, 0.62);
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    let at = |dh: f32| {
        let h = (h + dh).rem_euclid(1.0);
        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let x = c * (1.0 - ((h * 6.0).rem_euclid(2.0) - 1.0).abs());
        let m = l - c / 2.0;
        let (r, g, b) = match (h * 6.0) as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        [r, g, b].map(|v| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8)
    };
    [at(-32.0 / 360.0), at(0.0), at(32.0 / 360.0)]
}

/// Fusion Desk's glass keys, when its schema is installed.
#[cfg(unix)]
fn fusion() -> Option<Vec<(String, String)>> {
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
            .then(|| String::from_utf8_lossy(&out.stdout).trim().trim_matches('\'').to_string())
    };
    let mut p = vec![
        ("effect".to_string(), get("glass-effect")?),
        ("style".to_string(), get("glass-style")?),
    ];
    // Older Fusion Desk builds have no Lumina keys: those come from glass-global.
    for key in ["lumina", "lumina-colours", "lumina-colour", "lumina-intensity"] {
        if let Some(v) = get(&format!("glass-{key}")) {
            p.push((key.to_string(), v));
        }
    }
    Some(p)
}

#[cfg(not(unix))]
fn fusion() -> Option<Vec<(String, String)>> {
    None
}

fn resolve() -> Look {
    if let Ok(v) = std::env::var("PLANNER_GLASS") {
        if v.trim().eq_ignore_ascii_case("planner") {
            return Look { planner: true, intensity: 0.7, ..Look::default() };
        }
        return pairs(&v.replace(',', "\n")).map(|p| look(&p)).unwrap_or_default();
    }
    let Some(d) = dir() else { return Look::default() };
    let read = |name: &str| std::fs::read_to_string(d.join(name)).ok();
    for name in ["glass-extendo", "glass"] {
        if let Some(p) = read(name).as_deref().and_then(pairs) {
            return look(&p);
        }
    }
    let global = read("glass-global").as_deref().and_then(pairs).unwrap_or_default();
    match fusion() {
        Some(mut p) => {
            for (k, v) in &global {
                if k.starts_with("lumina") && get(&p, k).is_none() {
                    p.push((k.clone(), v.clone()));
                }
            }
            look(&p)
        }
        None => look(&global),
    }
}

/// What Harbor Settings resolves for Extendo, cached for 3 s.
pub fn current() -> Look {
    // The Windows build has no Harbor Settings: there the look is the app's own choice.
    if cfg!(windows) {
        return Look::default();
    }
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

/// True while Harbor Settings resolves the Planner glass style for Extendo.
pub fn planner() -> bool {
    current().planner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks() {
        assert_eq!(block("harbor"), None);
        assert_eq!(block(""), None);
        assert_eq!(block("effect=STRONG\nstyle=PLANNER\n"), Some(true));
        assert_eq!(block("effect=OFF\nstyle=PLANNER\n"), Some(false));
        assert_eq!(block("style=FROSTED"), Some(false));
    }

    #[test]
    fn lumina() {
        let calm = look(&pairs("style=PLANNER").unwrap());
        assert!(calm.planner && calm.colours.is_none());
        let c = look(&pairs("style=PLANNER\nlumina=COLOURFUL\nlumina-intensity=40").unwrap());
        assert_eq!(c.colours, Some([[0xec, 0x48, 0x99], [0x8b, 0x5c, 0xf6], [0x38, 0xbd, 0xf8]]));
        assert!((c.intensity - 0.4).abs() < 1e-6);
        // Colourful needs the Lumina style itself
        let off = look(&pairs("style=FROSTED\nlumina=COLOURFUL").unwrap());
        assert!(off.colours.is_none());
    }
}
