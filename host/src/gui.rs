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
use crate::state::{SessionCommand, Shared, TransferState};

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
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, shared: Arc<Shared>) -> Self {
        let ctx = cc.egui_ctx.clone();
        shared.set_repaint(move || ctx.request_repaint());
        Self {
            shared,
            monitors: monitor_choices(),
            addresses: local_addresses(),
            addresses_at: Instant::now(),
            host_name: crate::discovery::host_name(),
            applied_dark: None,
            logo: load_logo(&cc.egui_ctx),
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
                    for d in &status.usb_devices {
                        ui.label(RichText::new(format!("✔ {d}")).color(p.good));
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
                    open_path(&settings.save_dir);
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
                                        open_path(path);
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
                .checkbox(&mut wifi, "📶  Allow WiFi connections")
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
                        self.status_card(ui);
                        self.connect_card(ui);
                        self.display_card(ui);
                        self.files_card(ui);
                        self.options_card(ui);
                        self.log_card(ui);
                        self.footer(ui);
                    });
            });
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

fn open_path(path: &std::path::Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(program).arg(path).spawn() {
        log::warn!("cannot open {}: {e}", path.display());
    }
}
