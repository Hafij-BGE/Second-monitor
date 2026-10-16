//! Desktop window: status, connection help, settings and file transfer.

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{
    Align, Color32, CornerRadius, FontId, Frame, Layout, Margin, RichText, Stroke, Ui, Vec2,
};

use crate::backend::{self, Mode};
use crate::files::human_size;
use crate::state::{AdbDevice, PairingDecision, SessionCommand, Shared, TransferState};
use crate::{mirror, storage};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Screen,
    Mirror,
    Storage,
    Shortcuts,
}

pub fn run(shared: Arc<Shared>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Extendo")
            .with_app_id("extendo")
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../../assets/extendo-256.png"))
                    .unwrap_or_default(),
            )
            .with_inner_size([580.0, 860.0])
            .with_min_inner_size([440.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Extendo",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, shared)))),
    )
}

struct Palette {
    bg: Color32,
    card: Color32,
    border: Color32,
    text: Color32,
    muted: Color32,
    accent: Color32,
    good: Color32,
    warn: Color32,
    bad: Color32,
}

const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x0f, 0x11, 0x17),
    card: Color32::from_rgb(0x1a, 0x1e, 0x2b),
    border: Color32::from_rgb(0x2a, 0x30, 0x45),
    text: Color32::from_rgb(0xe6, 0xe9, 0xf2),
    muted: Color32::from_rgb(0x8a, 0x93, 0xa8),
    accent: Color32::from_rgb(0x4f, 0x8c, 0xff),
    good: Color32::from_rgb(0x3d, 0xd6, 0x8c),
    warn: Color32::from_rgb(0xff, 0xb5, 0x47),
    bad: Color32::from_rgb(0xff, 0x5c, 0x5c),
};

const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xf3, 0xf5, 0xfa),
    card: Color32::WHITE,
    border: Color32::from_rgb(0xe2, 0xe6, 0xef),
    text: Color32::from_rgb(0x1a, 0x1f, 0x2c),
    muted: Color32::from_rgb(0x6b, 0x73, 0x85),
    accent: Color32::from_rgb(0x2f, 0x6f, 0xeb),
    good: Color32::from_rgb(0x1f, 0xa9, 0x6a),
    warn: Color32::from_rgb(0xd9, 0x8a, 0x10),
    bad: Color32::from_rgb(0xd9, 0x3b, 0x3b),
};

const SCALES: [(f32, &str); 6] = [
    (1.0, "100%"),
    (1.25, "125%"),
    (1.5, "150%"),
    (1.75, "175%"),
    (2.0, "200%"),
    (2.5, "250%"),
];

struct App {
    shared: Arc<Shared>,
    /// (value, label) for the monitor picker.
    monitors: Vec<(String, String)>,
    addresses: Vec<IpAddr>,
    addresses_at: Instant,
    host_name: String,
    applied_dark: Option<bool>,
    logo: Option<egui::TextureHandle>,
    tab: Tab,
    /// Device id of the pairing request we already raised the window for.
    pairing_seen: Option<String>,
    /// Text fields.
    mirror_address: String,
    storage_address: String,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, shared: Arc<Shared>) -> Self {
        let ctx = cc.egui_ctx.clone();
        shared.set_repaint(move || ctx.request_repaint());
        shared
            .has_gui
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Self {
            shared,
            monitors: monitor_choices(),
            addresses: local_addresses(),
            addresses_at: Instant::now(),
            host_name: crate::discovery::host_name(),
            applied_dark: None,
            logo: load_logo(&cc.egui_ctx),
            // SMON_TAB=mirror|storage|shortcuts opens that tab (for screenshots).
            tab: match std::env::var("SMON_TAB").as_deref() {
                Ok("mirror") => Tab::Mirror,
                Ok("storage") => Tab::Storage,
                Ok("shortcuts") => Tab::Shortcuts,
                _ => Tab::Screen,
            },
            mirror_address: String::new(),
            storage_address: String::new(),
            pairing_seen: None,
        }
    }

    fn palette(&self) -> &'static Palette {
        if self.shared.settings().dark_theme {
            &DARK
        } else {
            &LIGHT
        }
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let dark = self.shared.settings().dark_theme;
        if self.applied_dark == Some(dark) {
            return;
        }
        self.applied_dark = Some(dark);
        let p = self.palette();
        let mut visuals = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.panel_fill = p.bg;
        visuals.window_fill = p.card;
        visuals.extreme_bg_color = if dark {
            Color32::from_rgb(0x12, 0x15, 0x1f)
        } else {
            Color32::from_rgb(0xee, 0xf1, 0xf7)
        };
        visuals.selection.bg_fill = p.accent;
        visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        visuals.hyperlink_color = p.accent;
        let radius = CornerRadius::same(8);
        for w in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            w.corner_radius = radius;
        }
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
        visuals.widgets.inactive.weak_bg_fill = if dark {
            Color32::from_rgb(0x25, 0x2a, 0x3b)
        } else {
            Color32::from_rgb(0xe9, 0xed, 0xf5)
        };
        visuals.widgets.hovered.weak_bg_fill = if dark {
            Color32::from_rgb(0x2f, 0x35, 0x4a)
        } else {
            Color32::from_rgb(0xdd, 0xe3, 0xef)
        };
        let theme = if dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        };
        ctx.set_theme(theme);
        ctx.set_visuals_of(theme, visuals);
        ctx.all_styles_mut(|s| {
            s.spacing.item_spacing = Vec2::new(10.0, 8.0);
            s.spacing.button_padding = Vec2::new(14.0, 7.0);
            s.spacing.interact_size.y = 30.0;
        });
    }

    fn card(&mut self, ui: &mut Ui, title: &str, add: impl FnOnce(&mut Self, &mut Ui)) {
        let p = self.palette();
        Frame::default()
            .fill(p.card)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::same(18))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(title).size(13.0).color(p.muted).strong());
                ui.add_space(6.0);
                add(self, ui);
            });
        ui.add_space(12.0);
    }

    fn update_setting(&self, f: impl FnOnce(&mut crate::state::Settings)) {
        let mut s = self.shared.settings.lock().unwrap();
        f(&mut s);
        s.save();
    }

    fn header(&mut self, ui: &mut Ui) {
        let p = self.palette();
        ui.horizontal(|ui| {
            match &self.logo {
                Some(logo) => {
                    ui.add(egui::Image::new((logo.id(), Vec2::splat(44.0))).corner_radius(10));
                }
                None => {
                    ui.label(RichText::new("🖥").size(28.0).color(p.accent));
                }
            }
            ui.vertical(|ui| {
                ui.label(RichText::new("Extendo").size(22.0).strong().color(p.text));
                ui.label(
                    RichText::new(format!("Host · {}", self.host_name))
                        .size(13.0)
                        .color(p.muted),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let dark = self.shared.settings().dark_theme;
                let icon = if dark { "☀" } else { "🌙" };
                if ui
                    .button(RichText::new(icon).size(16.0))
                    .on_hover_text("Switch theme")
                    .clicked()
                {
                    self.update_setting(|s| s.dark_theme = !dark);
                }
            });
        });
        ui.add_space(14.0);
    }

    fn status_card(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let status = self.shared.status().clone();
        self.card(ui, "STATUS", |app, ui| {
            if let Some(err) = &status.server_error {
                pill(ui, "Not running", p.bad);
                ui.add_space(4.0);
                ui.label(RichText::new(err).color(p.bad));
                return;
            }
            match &status.viewer {
                Some(v) => {
                    ui.horizontal(|ui| {
                        pill(ui, "● Connected", p.good);
                        let via = if v.usb {
                            "🔌 USB cable".to_string()
                        } else {
                            format!("📶 WiFi · {}", v.address)
                        };
                        ui.label(RichText::new(via).color(p.muted));
                    });
                    ui.add_space(6.0);
                    ui.label(RichText::new(&v.name).size(20.0).strong().color(p.text));
                    if v.width > 0 {
                        ui.label(
                            RichText::new(format!("{} × {}", v.width, v.height)).color(p.muted),
                        );
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        stat(ui, p, &format!("{:.0}", v.fps), "fps");
                        stat(ui, p, &format!("{:.1}", v.mbps), "Mbit/s");
                        stat(ui, p, &format!("{:.0} ms", v.encode_ms), "encode");
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("📤  Send files…").clicked() {
                            app.pick_and_send();
                        }
                        let reconnect = ui
                            .button("⟳  Reconnect")
                            .on_hover_text("Apply changed settings now")
                            .clicked();
                        let disconnect = ui.button("✖  Disconnect").clicked();
                        if reconnect || disconnect {
                            app.shared.command(SessionCommand::Disconnect);
                        }
                    });
                }
                None => {
                    pill(ui, "○ Waiting for device", p.warn);
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Open Extendo on your tablet and tap Connect.")
                            .size(15.0)
                            .color(p.text),
                    );
                }
            }
        });
    }

    fn connect_card(&mut self, ui: &mut Ui) {
        let p = self.palette();
        if self.addresses_at.elapsed() > Duration::from_secs(10) {
            self.addresses = local_addresses();
            self.addresses_at = Instant::now();
        }
        let status = self.shared.status().clone();
        let settings = self.shared.settings();
        self.card(ui, "HOW TO CONNECT", |app, ui| {
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                ui.label(RichText::new("🔌 USB cable").strong().color(p.text));
                if !settings.usb {
                    ui.label(RichText::new("Turned off in Options").color(p.muted));
                } else if status.adb_path.is_none() {
                    ui.label(RichText::new("adb not found").color(p.warn));
                    ui.label(
                        RichText::new("Install Android platform-tools to use USB.")
                            .size(12.0)
                            .color(p.muted),
                    );
                } else if status.usb_devices.is_empty() {
                    ui.label(
                        RichText::new("Plug in the tablet with USB debugging on.").color(p.muted),
                    );
                } else {
                    for d in status.usb_devices.iter().filter(|d| !d.wifi) {
                        ui.label(RichText::new(format!("✔ {}", d.model)).color(p.good));
                    }
                    ui.label(
                        RichText::new("Tap \"USB cable\" in the app.")
                            .size(12.0)
                            .color(p.muted),
                    );
                }

                let ui = &mut cols[1];
                ui.label(RichText::new("📶 WiFi").strong().color(p.text));
                if !settings.wifi {
                    ui.label(RichText::new("Turned off in Options").color(p.muted));
                } else if app.addresses.is_empty() {
                    ui.label(RichText::new("No network connection").color(p.warn));
                } else {
                    for ip in &app.addresses {
                        let text = format!("{ip}:{}", settings.port);
                        if ui
                            .add(
                                egui::Label::new(
                                    RichText::new(&text)
                                        .font(FontId::monospace(14.0))
                                        .color(p.accent),
                                )
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_text("Click to copy")
                            .clicked()
                        {
                            ui.ctx().copy_text(text);
                        }
                    }
                    ui.label(
                        RichText::new("Same network; the app finds this PC.")
                            .size(12.0)
                            .color(p.muted),
                    );
                }
            });
        });
    }

    fn display_card(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let s = self.shared.settings();
        let connected = self.shared.status().viewer.is_some();
        self.card(ui, "DISPLAY", |app, ui| {
            egui::Grid::new("display")
                .num_columns(2)
                .spacing([18.0, 12.0])
                .show(ui, |ui| {
                    ui.label("Mode");
                    ui.horizontal(|ui| {
                        for (mode, label) in [(Mode::Extend, "Extend"), (Mode::Mirror, "Mirror")] {
                            if ui.selectable_label(s.mode == mode, label).clicked() {
                                app.update_setting(|s| s.mode = mode);
                            }
                        }
                    });
                    ui.end_row();

                    ui.label("Monitor");
                    let current = s.monitor.clone().unwrap_or_default();
                    let label = app
                        .monitors
                        .iter()
                        .find(|(v, _)| *v == current)
                        .map(|(_, l)| l.clone())
                        .unwrap_or_else(|| "Automatic".into());
                    egui::ComboBox::from_id_salt("monitor")
                        .selected_text(label)
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(s.monitor.is_none(), "Automatic")
                                .clicked()
                            {
                                app.update_setting(|s| s.monitor = None);
                            }
                            for (value, label) in app.monitors.clone() {
                                if ui
                                    .selectable_label(s.monitor.as_deref() == Some(&value), label)
                                    .clicked()
                                {
                                    app.update_setting(|s| s.monitor = Some(value.clone()));
                                }
                            }
                        });
                    ui.end_row();

                    ui.label("Scale")
                        .on_hover_text("Makes text and windows bigger on the tablet");
                    let scale_label = SCALES
                        .iter()
                        .find(|(v, _)| (*v - s.scale).abs() < 0.01)
                        .map(|(_, l)| *l)
                        .unwrap_or("Custom");
                    egui::ComboBox::from_id_salt("scale")
                        .selected_text(scale_label)
                        .show_ui(ui, |ui| {
                            for (v, l) in SCALES {
                                if ui.selectable_label((v - s.scale).abs() < 0.01, l).clicked() {
                                    app.update_setting(|s| s.scale = v);
                                }
                            }
                        });
                    ui.end_row();

                    ui.label("Frame rate")
                        .on_hover_text("Used when the app's quality is set to Auto");
                    ui.horizontal(|ui| {
                        for fps in [30, 60, 90, 120] {
                            if ui
                                .selectable_label(s.fps == fps, format!("{fps}"))
                                .clicked()
                            {
                                app.update_setting(|s| s.fps = fps);
                            }
                        }
                    });
                    ui.end_row();

                    ui.label("Bitrate")
                        .on_hover_text("Higher = sharper, needs a faster link");
                    let mut bitrate = s.bitrate_mbps;
                    if ui
                        .add(egui::Slider::new(&mut bitrate, 2..=60).suffix(" Mbit/s"))
                        .changed()
                    {
                        app.update_setting(|s| s.bitrate_mbps = bitrate);
                    }
                    ui.end_row();
                });
            let installed = backend::virtual_display_installed();
            if backend::virtual_display_can_stay_on() {
                let mut manage = s.manage_virtual_display;
                if ui
                    .checkbox(
                        &mut manage,
                        "Virtual display only while a tablet is connected (recommended)",
                    )
                    .on_hover_text(
                        "Off: Virtual Display Driver's screen stays on all the time, \
                         for other uses.",
                    )
                    .changed()
                {
                    app.update_setting(|s| s.manage_virtual_display = manage);
                }
                if s.mode == Mode::Extend
                    && ui
                        .small_button("Switch to Parsec Virtual Display Driver")
                        .on_hover_text(
                            "Adds and removes the tablet's screen without restarting the \
                             driver. Downloads Parsec's signed driver; Windows asks for admin \
                             approval.",
                        )
                        .clicked()
                {
                    backend::install_virtual_display(s.tablet_size);
                }
            }
            if s.mode == Mode::Extend && installed == Some(true) {
                if let Some((w, h)) = backend::virtual_display_missing_size() {
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "The virtual display has no {w}×{h} size yet, so the tablet \
                                 letterboxes."
                            ))
                            .size(12.0)
                            .color(p.muted),
                        );
                        if ui
                            .small_button(format!("Add {w}×{h}"))
                            .on_hover_text(
                                "Windows asks for admin approval. Reconnect the tablet \
                                 afterwards.",
                            )
                            .clicked()
                        {
                            backend::install_virtual_display(Some((w, h)));
                        }
                    });
                }
            }
            if s.mode == Mode::Extend && installed == Some(false) {
                ui.add_space(6.0);
                Frame::default()
                    .fill(p.warn.gamma_multiply(0.12))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("Extend needs a virtual display on Windows")
                                .strong()
                                .color(p.text),
                        );
                        ui.label(
                            RichText::new(
                                "Windows can't add a screen by itself, so without one Extendo \
                                 mirrors. Install Parsec's free, signed virtual display driver \
                                 once; Extendo then adds the tablet's screen while it is \
                                 connected and removes it afterwards, even if Extendo crashes.",
                            )
                            .size(12.0)
                            .color(p.muted),
                        );
                        ui.add_space(4.0);
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("🖥  Set up extend").color(Color32::WHITE),
                                )
                                .fill(p.accent),
                            )
                            .on_hover_text(
                                "Downloads Parsec Virtual Display Driver 0.41 from Parsec. \
                                 Windows asks for admin approval.",
                            )
                            .clicked()
                        {
                            backend::install_virtual_display(s.tablet_size);
                        }
                    });
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Changes apply on the next connection.")
                        .size(12.0)
                        .color(p.muted),
                );
                if connected && ui.small_button("Apply now").clicked() {
                    app.shared.command(SessionCommand::Disconnect);
                }
            });
        });
    }

    fn files_card(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let status = self.shared.status().clone();
        let settings = self.shared.settings();
        let connected = status.viewer.is_some();
        self.card(ui, "FILES", |app, ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(connected, egui::Button::new("📤  Send files to tablet…"))
                    .clicked()
                {
                    app.pick_and_send();
                }
                ui.label(
                    RichText::new(if connected {
                        "or drop files on this window"
                    } else {
                        "Connect a device first"
                    })
                    .size(12.0)
                    .color(p.muted),
                );
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Received files:").color(p.muted));
                ui.label(
                    RichText::new(settings.save_dir.display().to_string())
                        .font(FontId::monospace(12.0)),
                );
            });
            ui.horizontal(|ui| {
                if ui.small_button("📂 Open folder").clicked() {
                    let _ = std::fs::create_dir_all(&settings.save_dir);
                    crate::desktop::open_in_file_manager(&settings.save_dir.display().to_string());
                }
                if ui.small_button("Change…").clicked() {
                    let shared = app.shared.clone();
                    std::thread::spawn(move || {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            let mut s = shared.settings.lock().unwrap();
                            s.save_dir = dir;
                            s.save();
                            drop(s);
                            shared.changed();
                        }
                    });
                }
                if status
                    .transfers
                    .iter()
                    .any(|t| t.state != TransferState::Running)
                    && ui.small_button("Clear finished").clicked()
                {
                    app.shared.update_status(|s| {
                        s.transfers.retain(|t| t.state == TransferState::Running)
                    });
                }
            });
            if !status.transfers.is_empty() {
                ui.add_space(6.0);
            }
            for t in status.transfers.iter().rev().take(8) {
                ui.horizontal(|ui| {
                    let arrow = if t.incoming { "⬇" } else { "⬆" };
                    ui.label(RichText::new(arrow).color(p.accent));
                    ui.label(RichText::new(&t.name).color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &t.state {
                        TransferState::Running => {
                            let frac = if t.size > 0 {
                                t.done as f32 / t.size as f32
                            } else {
                                0.0
                            };
                            ui.add(
                                egui::ProgressBar::new(frac)
                                    .desired_width(140.0)
                                    .show_percentage(),
                            );
                        }
                        TransferState::Done => {
                            if t.incoming {
                                if let Some(path) = &t.path {
                                    if ui.small_button("Open").clicked() {
                                        crate::desktop::open_file(path);
                                    }
                                }
                            }
                            ui.label(
                                RichText::new(format!("✔ {}", human_size(t.size))).color(p.good),
                            );
                        }
                        TransferState::Failed(e) => {
                            ui.label(RichText::new(format!("✖ {e}")).color(p.bad));
                        }
                    });
                });
            }
        });
    }

    fn options_card(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let s = self.shared.settings();
        self.card(ui, "OPTIONS", |app, ui| {
            let mut clipboard = s.clipboard;
            if ui
                .checkbox(&mut clipboard, "📋  Sync clipboard with the tablet")
                .changed()
            {
                app.update_setting(|s| s.clipboard = clipboard);
            }
            let mut usb = s.usb;
            if ui
                .checkbox(&mut usb, "🔌  Allow USB connections (adb)")
                .changed()
            {
                app.update_setting(|s| s.usb = usb);
            }
            let mut wifi = s.wifi;
            if ui
                .checkbox(
                    &mut wifi,
                    "📶  Allow WiFi connections (off = USB only, port closed)",
                )
                .changed()
            {
                app.update_setting(|s| s.wifi = wifi);
            }
            ui.horizontal(|ui| {
                ui.label("Port");
                let mut port = s.port;
                if ui
                    .add(egui::DragValue::new(&mut port).range(1024..=65535))
                    .changed()
                {
                    app.update_setting(|s| s.port = port);
                }
                ui.label(RichText::new("restart to apply").size(12.0).color(p.muted));
            });
        });
    }

    fn tabs(&mut self, ui: &mut Ui) {
        let p = self.palette();
        Frame::default()
            .fill(p.card)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::same(4))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.columns(4, |cols| {
                    for (col, (tab, label)) in cols.iter_mut().zip([
                        (Tab::Screen, "🖥 Screen"),
                        (Tab::Mirror, "📱 Mirror"),
                        (Tab::Storage, "📂 Storage"),
                        (Tab::Shortcuts, "⌨ Keys"),
                    ]) {
                        let selected = self.tab == tab;
                        let text = RichText::new(label).size(13.5).color(if selected {
                            Color32::WHITE
                        } else {
                            p.muted
                        });
                        let button = egui::Button::new(text)
                            .fill(if selected {
                                p.accent
                            } else {
                                Color32::TRANSPARENT
                            })
                            .stroke(Stroke::NONE)
                            .corner_radius(CornerRadius::same(9))
                            .min_size(Vec2::new(col.available_width(), 34.0));
                        if col.add(button).clicked() {
                            self.tab = tab;
                        }
                    }
                });
            });
        ui.add_space(12.0);
    }

    fn mirror_devices_card(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let status = self.shared.status().clone();
        let settings = self.shared.settings();
        self.card(ui, "PHONE MIRROR", |app, ui| {
            ui.label(
                RichText::new("See and control your phone on this PC, with sound. Works over a cable or WiFi.")
                    .color(p.text),
            );
            ui.add_space(6.0);
            let Some((_, version)) = &status.scrcpy else {
                ui.label(RichText::new("Mirroring engine not found.").color(p.warn).strong());
                let hint = if cfg!(windows) {
                    "The mirror engine is missing next to extendo.exe; reinstall Extendo."
                } else {
                    "The mirror engine is missing; reinstall the Extendo package."
                };
                ui.label(RichText::new(hint).color(p.muted));
                return;
            };
            if status.adb_path.is_none() {
                ui.label(RichText::new("adb not found: install Android platform-tools.").color(p.warn));
                return;
            }
            if status.usb_devices.is_empty() {
                ui.label(
                    RichText::new("No phone found. Plug one in with USB debugging on, or connect over WiFi below.")
                        .color(p.muted),
                );
            }
            for d in status.usb_devices.clone() {
                let running = status.mirroring.contains(&d.serial);
                let busy = status.mirror_busy.iter().find(|(s, _)| *s == d.serial).map(|(_, m)| m.clone());
                Frame::default()
                    .fill(p.bg)
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(if d.wifi { "📶" } else { "🔌" }).size(18.0));
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&d.model).strong().color(p.text));
                                let how = if d.wifi { format!("WiFi · {}", d.serial) } else { "USB cable".into() };
                                ui.label(RichText::new(how).size(12.0).color(p.muted));
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if running {
                                    if ui.button("■  Stop").clicked() {
                                        mirror::stop(&d.serial);
                                    }
                                    for (icon, tip, action) in [
                                        ("□", "Recent apps", mirror::PhoneAction::Recents),
                                        ("○", "Home", mirror::PhoneAction::Home),
                                        ("◁", "Back", mirror::PhoneAction::Back),
                                    ] {
                                        if ui.button(RichText::new(icon).size(15.0)).on_hover_text(tip).clicked() {
                                            mirror::phone_action(app.shared.clone(), d.serial.clone(), action);
                                        }
                                    }
                                    ui.menu_button("⋯", |ui| {
                                        use mirror::PhoneAction as A;
                                        for (label, action) in [
                                            ("🔊  Volume up", A::VolumeUp),
                                            ("🔉  Volume down", A::VolumeDown),
                                            ("⏻  Power (lock / wake)", A::Power),
                                            ("🔔  Open notifications", A::Notifications),
                                            ("📷  Screenshot to Downloads/Extendo", A::Screenshot),
                                        ] {
                                            if ui.button(label).clicked() {
                                                mirror::phone_action(app.shared.clone(), d.serial.clone(), action);
                                                ui.close();
                                            }
                                        }
                                    });
                                    ui.label(RichText::new("● Mirroring").color(p.good));
                                } else if ui
                                    .add(egui::Button::new(RichText::new("▶  Mirror").color(Color32::WHITE)).fill(p.accent))
                                    .clicked()
                                {
                                    mirror::start(app.shared.clone(), d.clone());
                                }
                                if d.wifi_debugging
                                    && ui
                                        .small_button("🔒 Turn off WiFi")
                                        .on_hover_text(
                                            "Close the phone's debugging port on the network (5555). \
                                             Mirroring then needs the cable again.",
                                        )
                                        .clicked()
                                {
                                    mirror::disable_wifi(app.shared.clone(), d.clone());
                                }
                                if d.wifi {
                                    if ui.small_button("Forget").on_hover_text("Disconnect and remove").clicked() {
                                        mirror::disconnect(app.shared.clone(), d.serial.clone(), true);
                                    }
                                } else if let Some(msg) = &busy {
                                    ui.spinner();
                                    ui.label(RichText::new(msg).size(12.0).color(p.muted));
                                } else if !d.wifi_debugging
                                    && ui
                                    .small_button("📶 Use WiFi")
                                    .on_hover_text("Switch this phone to WiFi so you can unplug the cable")
                                    .clicked()
                                {
                                    mirror::enable_wifi(app.shared.clone(), d.clone());
                                }
                            });
                        });
                    });
                ui.add_space(6.0);
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Connect by address");
                ui.add(egui::TextEdit::singleline(&mut app.mirror_address).hint_text("192.168.1.50").desired_width(160.0));
                if ui.button("Connect").clicked() && !app.mirror_address.trim().is_empty() {
                    mirror::connect(app.shared.clone(), app.mirror_address.trim().to_string());
                }
            });
            let offline: Vec<&String> = settings
                .wifi_phones
                .iter()
                .filter(|a| !status.usb_devices.iter().any(|d| &d.serial == *a))
                .collect();
            if !offline.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Saved:").size(12.0).color(p.muted));
                    for a in offline {
                        if ui.small_button(format!("⟳ {a}")).on_hover_text("Reconnect").clicked() {
                            mirror::connect(app.shared.clone(), a.clone());
                        }
                    }
                });
            }
            ui.label(
                RichText::new(format!(
                    "WiFi: plug the phone in once and press \"Use WiFi\". Android 11+ can also pair with Wireless debugging. · powered by {version}"
                ))
                .size(12.0)
                .color(p.muted),
            );
            if status.viewer.as_ref().is_some_and(|v| v.usb) && !status.mirroring.is_empty() {
                ui.label(
                    RichText::new("Tip: don't mirror the tablet that is showing your PC: it would show itself forever.")
                        .size(12.0)
                        .color(p.warn),
                );
            }
        });
    }

    fn mirror_options_card(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let m = self.shared.settings().mirror;
        self.card(ui, "MIRROR OPTIONS", |app, ui| {
            let mut next = m.clone();
            egui::Grid::new("mirror_opts")
                .num_columns(2)
                .spacing([18.0, 12.0])
                .show(ui, |ui| {
                    ui.label("Resolution");
                    let label = match next.max_size {
                        0 => "Full".to_string(),
                        n => format!("{n} px"),
                    };
                    egui::ComboBox::from_id_salt("mirror_size")
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for (v, l) in [
                                (0, "Full"),
                                (1920, "1920 px"),
                                (1600, "1600 px"),
                                (1280, "1280 px"),
                                (1024, "1024 px"),
                                (800, "800 px"),
                            ] {
                                ui.selectable_value(&mut next.max_size, v, l);
                            }
                        });
                    ui.end_row();

                    ui.label("Frame rate");
                    ui.horizontal(|ui| {
                        for f in [30, 60, 90, 120] {
                            ui.selectable_value(&mut next.max_fps, f, format!("{f}"));
                        }
                    });
                    ui.end_row();

                    ui.label("Bitrate");
                    ui.add(egui::Slider::new(&mut next.bitrate_mbps, 2..=32).suffix(" Mbit/s"));
                    ui.end_row();
                });
            ui.add_space(6.0);
            ui.checkbox(
                &mut next.navbar,
                "◁ ○ □  Navigation bar at the bottom of the mirror window",
            );
            ui.checkbox(
                &mut next.screen_off,
                "🌙  Turn the phone screen off while mirroring (saves battery; Alt+O anytime)",
            );
            ui.checkbox(
                &mut next.audio,
                "🔊  Play the phone's sound on this PC (Android 11+)",
            );
            ui.checkbox(
                &mut next.stay_awake,
                "☕  Keep the phone awake while plugged in",
            );
            ui.checkbox(
                &mut next.physical_keyboard,
                "⌨  Act as a physical keyboard (games, keyboard layouts)",
            );
            ui.checkbox(&mut next.show_touches, "👆  Show touches on the phone");
            ui.checkbox(&mut next.always_on_top, "📌  Keep the mirror window on top");
            ui.checkbox(&mut next.fullscreen, "⛶  Start in fullscreen");
            ui.checkbox(
                &mut next.power_off_on_close,
                "⏻  Lock the phone when the window closes",
            );
            ui.checkbox(
                &mut next.record,
                "⏺  Record to a video in Downloads/Extendo/Recordings",
            );
            ui.checkbox(
                &mut next.camera,
                "📷  Show the phone's camera instead of its screen (Android 12+)",
            );
            ui.label(
                RichText::new("Changes apply to the next mirror window.")
                    .size(12.0)
                    .color(p.muted),
            );
            if next != m {
                app.update_setting(|s| s.mirror = next);
            }
        });
    }

    fn storage_card(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let status = self.shared.status().clone();
        if self.storage_address.is_empty() {
            self.storage_address = self.shared.settings().storage_address;
        }
        self.card(ui, "PHONE STORAGE", |app, ui| {
            ui.label(RichText::new("Browse and copy your phone's files from this PC, like a USB drive.").color(p.text));
            ui.add_space(4.0);
            ui.label(
                RichText::new("1. Open Extendo on the phone, then Phone storage, then Start.\n2. Open it here. When asked, log in as \"extendo\" with the password shown on the phone.")
                    .color(p.muted),
            );
            ui.add_space(10.0);

            ui.label(RichText::new("🔌 Over USB cable").strong().color(p.text));
            let usb: Vec<AdbDevice> = status.usb_devices.iter().filter(|d| !d.wifi).cloned().collect();
            if usb.is_empty() {
                ui.label(RichText::new("No phone plugged in with USB debugging on.").size(12.0).color(p.muted));
            }
            for d in usb {
                ui.horizontal(|ui| {
                    ui.label(&d.model);
                    if ui.button("📂  Open files").clicked() {
                        storage::open_usb(app.shared.clone(), d.serial.clone());
                    }
                });
            }
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new("📶 Over WiFi").strong().color(p.text));
                if status.storage_searching {
                    ui.spinner();
                } else if ui.small_button("⟳ Find phones").clicked() {
                    storage::search(app.shared.clone());
                }
            });
            for ph in &status.storage_phones {
                ui.horizontal(|ui| {
                    ui.label(format!("{}  ·  {}", ph.name, ph.address));
                    if ui.button("📂  Open files").clicked() {
                        storage::open(&format!("{}:{}", ph.address, ph.port));
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.label("Address");
                ui.add(egui::TextEdit::singleline(&mut app.storage_address).hint_text("192.168.1.50").desired_width(160.0));
                if ui.button("Open").clicked() && !app.storage_address.trim().is_empty() {
                    let address = app.storage_address.trim().to_string();
                    app.update_setting(|s| s.storage_address = address.clone());
                    storage::open(&address);
                }
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new("Files opens the phone in your file manager (ftp://). Only use it on networks you trust.")
                    .size(12.0)
                    .color(p.muted),
            );
        });
    }

    fn shortcuts_card(&mut self, ui: &mut Ui) {
        let p = self.palette();
        self.card(ui, "PHONE MIRROR SHORTCUTS", |_, ui| {
            ui.label(RichText::new("Use these in the mirror window. Alt can be replaced by the Super (Windows) key.").color(p.muted));
            ui.add_space(6.0);
            shortcut_grid(ui, p, "mirror_keys", mirror::SHORTCUTS);
        });
        self.card(ui, "TABLET GESTURES (SECOND SCREEN)", |_, ui| {
            shortcut_grid(
                ui,
                p,
                "tablet_keys",
                &[
                    (
                        "Edge handle ‹",
                        "Open the toolbar: keyboard, Ctrl/Alt/Shift/Super, copy, paste, files",
                    ),
                    (
                        "Touchscreen mode",
                        "Touch the PC directly; pinch and swipe work on GNOME",
                    ),
                    ("Trackpad · tap", "Click"),
                    ("Trackpad · two-finger tap", "Right-click"),
                    ("Trackpad · three-finger tap", "Middle-click"),
                    ("Trackpad · two fingers", "Scroll"),
                    ("Trackpad · press and hold", "Drag"),
                    (
                        "Stylus or mouse",
                        "Points directly; stylus button = right-click",
                    ),
                    ("Keyboard on the tablet", "Types on the PC"),
                ],
            );
        });
        self.card(ui, "THIS WINDOW", |_, ui| {
            shortcut_grid(
                ui,
                p,
                "window_keys",
                &[
                    (
                        "Drop files on the window",
                        "Send them to the connected tablet",
                    ),
                    ("Click a WiFi address", "Copy it"),
                ],
            );
        });
    }

    fn security_card(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let status = self.shared.status().clone();
        let settings = self.shared.settings();
        self.card(ui, "SECURITY", |app, ui| {
            ui.horizontal(|ui| {
                pill(ui, "🔒 Encrypted", p.good);
                ui.label(RichText::new("All connections use TLS.").color(p.muted));
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("This PC's code:").color(p.text));
                ui.label(
                    RichText::new(&status.pairing_code)
                        .font(FontId::monospace(18.0))
                        .strong()
                        .color(p.accent),
                );
            });
            ui.label(
                RichText::new(
                    "A new tablet on WiFi must be allowed here first, and shows this same code. \
                     If the codes differ, someone may be intercepting: deny it.",
                )
                .size(12.0)
                .color(p.muted),
            );
            ui.add_space(8.0);
            ui.label(RichText::new("Trusted tablets").strong().color(p.text));
            if settings.trusted_devices.is_empty() {
                ui.label(
                    RichText::new("None yet. USB connections are always allowed.")
                        .size(12.0)
                        .color(p.muted),
                );
            }
            for d in &settings.trusted_devices {
                ui.horizontal(|ui| {
                    ui.label(format!("📱 {}", d.name));
                    if ui.small_button("Remove").clicked() {
                        let id = d.id.clone();
                        app.update_setting(|s| s.trusted_devices.retain(|t| t.id != id));
                    }
                });
            }
            ui.add_space(6.0);
            let mut usb_only = !settings.wifi;
            if ui
                .checkbox(
                    &mut usb_only,
                    "USB only: close the network port (WiFi tablets can't connect)",
                )
                .changed()
            {
                app.update_setting(|s| s.wifi = !usb_only);
            }
        });
    }

    /// Asks whether a new WiFi tablet may connect.
    fn pairing_dialog(&mut self, ctx: &egui::Context) {
        let p = self.palette();
        let status = self.shared.status().clone();
        let Some(req) = status.pairing else {
            self.pairing_seen = None;
            return;
        };
        if self.pairing_seen.as_deref() != Some(&req.id) {
            self.pairing_seen = Some(req.id.clone());
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Critical,
            ));
        }
        egui::Modal::new(egui::Id::new("pairing")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(
                RichText::new("Allow this tablet?")
                    .size(20.0)
                    .strong()
                    .color(p.text),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("📱 {}", req.name))
                    .size(16.0)
                    .color(p.text),
            );
            ui.label(RichText::new(format!("from {} over WiFi", req.address)).color(p.muted));
            ui.add_space(10.0);
            ui.label(RichText::new("Check the tablet shows this code:").color(p.text));
            ui.label(
                RichText::new(&status.pairing_code)
                    .font(FontId::monospace(30.0))
                    .strong()
                    .color(p.accent),
            );
            ui.label(
                RichText::new("If it doesn't match, choose Deny.")
                    .size(12.0)
                    .color(p.muted),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Deny").clicked() {
                    self.shared.answer_pairing(PairingDecision::Deny);
                }
                if ui.button("Allow once").clicked() {
                    self.shared.answer_pairing(PairingDecision::AllowOnce);
                }
                if ui
                    .add(
                        egui::Button::new(RichText::new("Always allow").color(Color32::WHITE))
                            .fill(p.accent),
                    )
                    .clicked()
                {
                    self.shared.answer_pairing(PairingDecision::AlwaysAllow);
                }
            });
        });
    }

    fn footer(&mut self, ui: &mut Ui) {
        let p = self.palette();
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(format!(
                    "Extendo {} · Made by Md Hafijur Rahman",
                    env!("CARGO_PKG_VERSION")
                ))
                .size(12.0)
                .color(p.muted),
            );
            ui.hyperlink_to(
                RichText::new("github.com/Hafij-BGE").size(12.0),
                "https://github.com/Hafij-BGE",
            );
        });
        ui.add_space(8.0);
    }

    fn log_card(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let lines: Vec<String> = self.shared.log.lock().unwrap().iter().cloned().collect();
        egui::CollapsingHeader::new(RichText::new("Activity log").color(p.muted))
            .default_open(false)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in lines {
                            ui.label(
                                RichText::new(line)
                                    .font(FontId::monospace(11.5))
                                    .color(p.muted),
                            );
                        }
                    });
            });
    }

    fn pick_and_send(&self) {
        let shared = self.shared.clone();
        std::thread::spawn(move || {
            if let Some(paths) = rfd::FileDialog::new()
                .set_title("Send files to tablet")
                .pick_files()
            {
                shared.command(SessionCommand::SendFiles(paths));
            }
        });
    }

    fn handle_dropped_files(&self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if !dropped.is_empty() && !self.shared.command(SessionCommand::SendFiles(dropped)) {
            log::warn!("connect a device before sending files");
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_theme(&ctx);
        self.handle_dropped_files(&ctx);
        let p = self.palette();
        egui::CentralPanel::default()
            .frame(Frame::default().fill(p.bg).inner_margin(Margin::same(20)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.header(ui);
                        self.tabs(ui);
                        match self.tab {
                            Tab::Screen => {
                                self.status_card(ui);
                                self.connect_card(ui);
                                self.display_card(ui);
                                self.files_card(ui);
                                self.security_card(ui);
                                self.options_card(ui);
                            }
                            Tab::Mirror => {
                                self.mirror_devices_card(ui);
                                self.mirror_options_card(ui);
                            }
                            Tab::Storage => self.storage_card(ui),
                            Tab::Shortcuts => self.shortcuts_card(ui),
                        }
                        self.log_card(ui);
                        self.footer(ui);
                    });
            });
        self.pairing_dialog(&ctx);
        // Refresh stats even when no other event arrives.
        ctx.request_repaint_after(Duration::from_secs(1));
        debug_screenshot(&ctx);
    }
}

/// `SMON_SCREENSHOT=out.ppm` saves the window once, for checking the layout.
fn debug_screenshot(ctx: &egui::Context) {
    let Some(path) = std::env::var_os("SMON_SCREENSHOT") else {
        return;
    };
    let frame = ctx.cumulative_frame_nr();
    if frame == 30 {
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
    }
    if frame < 25 {
        ctx.request_repaint();
    }
    let shots: Vec<Arc<egui::ColorImage>> = ctx.input(|i| {
        i.raw
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
            .collect()
    });
    for image in shots {
        let [w, h] = image.size;
        let mut ppm = format!("P6 {w} {h} 255\n").into_bytes();
        for px in &image.pixels {
            ppm.extend_from_slice(&[px.r(), px.g(), px.b()]);
        }
        let _ = std::fs::write(&path, ppm);
        log::info!("screenshot saved to {}", PathBuf::from(&path).display());
    }
}

fn pill(ui: &mut Ui, text: &str, color: Color32) {
    Frame::default()
        .fill(color.gamma_multiply(0.15))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color).strong().size(13.0));
        });
}

fn stat(ui: &mut Ui, p: &Palette, value: &str, label: &str) {
    Frame::default()
        .fill(p.bg)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 8))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(value).size(18.0).strong().color(p.text));
                ui.label(RichText::new(label).size(11.0).color(p.muted));
            });
        });
}

fn shortcut_grid(ui: &mut Ui, p: &Palette, _id: &str, rows: &[(&str, &str)]) {
    // Keys column scales with the window; descriptions wrap.
    let key_width = (ui.available_width() * 0.42).clamp(120.0, 240.0);
    for (i, (keys, what)) in rows.iter().enumerate() {
        let fill = if i % 2 == 0 {
            p.bg
        } else {
            Color32::TRANSPARENT
        };
        Frame::default()
            .fill(fill)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [key_width, 18.0],
                        egui::Label::new(
                            RichText::new(*keys)
                                .font(FontId::monospace(13.0))
                                .color(p.accent),
                        )
                        .wrap(),
                    );
                    ui.add(egui::Label::new(RichText::new(*what).color(p.text)).wrap());
                });
            });
    }
}

fn load_logo(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../../assets/extendo-256.png")).ok()?;
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    Some(ctx.load_texture("logo", image, egui::TextureOptions::LINEAR))
}

fn monitor_choices() -> Vec<(String, String)> {
    let list = backend::list_monitors().unwrap_or_default();
    list.into_iter()
        .map(|m| {
            // Windows entries look like "0: \\.\DISPLAY1 1920x1080 ..."; the
            // setting stores the index. Linux entries are connector names.
            match m.split_once(':') {
                Some((idx, rest)) if idx.chars().all(|c| c.is_ascii_digit()) => {
                    (idx.to_string(), rest.trim().to_string())
                }
                _ => (m.clone(), m),
            }
        })
        .collect()
}

fn local_addresses() -> Vec<IpAddr> {
    let mut ips: Vec<IpAddr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        // Container and VM bridges aren't reachable from the tablet.
        .filter(|i| {
            ![
                "docker", "br-", "veth", "virbr", "vmnet", "vboxnet", "tun", "tap", "wg",
            ]
            .iter()
            .any(|p| i.name.starts_with(p))
        })
        .map(|i| i.ip())
        .filter(|ip| ip.is_ipv4())
        .collect();
    ips.sort();
    ips.dedup();
    ips
}
