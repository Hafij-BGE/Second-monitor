//! Desktop window: status, connection help, settings and file transfer.
//!
//! Layout: a header island with the tabs, then per tab a few frosted cards.
//! Security, options and the log open as drawers from the header.

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{
    Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Layout, Margin, Pos2, Rect,
    RichText, Sense, Stroke, Ui, Vec2,
};

use crate::backend::{self, Mode};
use crate::files::human_size;
use crate::gui_kit::{self as kit, Palette};
use crate::state::{
    AdbDevice, PairingDecision, PenSupport, SessionCommand, Shared, TransferState, WifiPairState,
};
use crate::{mirror, storage, wifi_pair};

const ANDROID_APP_URL: &str = "https://github.com/Hafij-BGE/Second-monitor/releases/latest";
/// Lets pen pressure reach /dev/uinput; the .deb installs the same rule.
#[cfg(target_os = "linux")]
const UINPUT_FIX: &str = "echo 'KERNEL==\"uinput\", SUBSYSTEM==\"misc\", TAG+=\"uaccess\", OPTIONS+=\"static_node=uinput\"' | sudo tee /etc/udev/rules.d/70-extendo-uinput.rules && sudo udevadm control --reload && sudo udevadm trigger --name-match=uinput";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Screen,
    Mirror,
    Storage,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Drawer {
    Security,
    Options,
    Log,
}

/// Shortcut deck filter.
#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyGroup {
    All,
    Power,
    Nav,
    View,
    Clip,
    Tablet,
    Pc,
}

impl KeyGroup {
    const LIST: [KeyGroup; 7] = [
        KeyGroup::All,
        KeyGroup::Power,
        KeyGroup::Nav,
        KeyGroup::View,
        KeyGroup::Clip,
        KeyGroup::Tablet,
        KeyGroup::Pc,
    ];

    fn label(self) -> &'static str {
        match self {
            KeyGroup::All => "All",
            KeyGroup::Power => "Screen & Power",
            KeyGroup::Nav => "Navigation",
            KeyGroup::View => "Window & View",
            KeyGroup::Clip => "Clipboard & Drop",
            KeyGroup::Tablet => "Tablet gestures",
            KeyGroup::Pc => "This PC",
        }
    }

    fn color(self) -> Color32 {
        match self {
            KeyGroup::All | KeyGroup::Clip => kit::GREEN,
            KeyGroup::Power => kit::ORANGE,
            KeyGroup::Nav => kit::CYAN,
            KeyGroup::View => kit::PURPLE,
            KeyGroup::Tablet => kit::PINK,
            KeyGroup::Pc => kit::INDIGO,
        }
    }
}

/// Group of each entry in `mirror::SHORTCUTS`, in order.
const MIRROR_GROUPS: [KeyGroup; 24] = {
    use KeyGroup::*;
    [
        Power, Power, Power, Nav, Nav, Nav, Nav, Nav, Nav, Nav, View, View, View, Nav, Power, View,
        View, View, Clip, Clip, Clip, View, Clip, Clip,
    ]
};

/// (keys, what) on the tablet when it is the second screen.
const TABLET_GESTURES: [(&str, &str); 9] = [
    (
        "‹ Edge handle",
        "Open the toolbar: keyboard, Ctrl/Alt/Shift/Super, copy, paste, files",
    ),
    (
        "Touchscreen mode",
        "Touch the PC directly; pinch and swipe work on GNOME",
    ),
    ("Tap", "Trackpad: click"),
    ("2-finger tap", "Trackpad: right-click"),
    ("3-finger tap", "Trackpad: middle-click"),
    ("2-finger drag", "Trackpad: scroll"),
    ("Press & hold", "Trackpad: drag"),
    (
        "Stylus / mouse",
        "Points directly; stylus button = right-click",
    ),
    ("Tablet keyboard", "Types on the PC"),
];

const PC_KEYS: [(&str, &str); 4] = [
    (
        "Ctrl+Alt+E",
        "Switch the tablet between Extend and Mirror (extendo --toggle-mode)",
    ),
    ("Ctrl+Alt+X", "Disconnect the tablet (extendo --disconnect)"),
    (
        "Drop files",
        "Drop files on this window to send them to the tablet",
    ),
    ("Click address", "Click a WiFi address to copy it"),
];

/// Draws one settings control `width` wide.
type Field<'a> = dyn Fn(&mut App, &mut Ui, f32) + 'a;
type MirrorField<'a> = dyn Fn(&mut Ui, &mut crate::state::MirrorSettings, f32) + 'a;

const SCALES: [(f32, &str); 6] = [
    (1.0, "100%"),
    (1.25, "125%"),
    (1.5, "150%"),
    (1.75, "175%"),
    (2.0, "200%"),
    (2.5, "250%"),
];

pub fn run(shared: Arc<Shared>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Extendo")
            .with_app_id("extendo")
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../../assets/extendo-256.png"))
                    .unwrap_or_default(),
            )
            .with_inner_size([960.0, 900.0])
            .with_min_inner_size([460.0, 580.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Extendo",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, shared)))),
    )
}

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
    drawer: Option<Drawer>,
    /// Device id of the pairing request we already raised the window for.
    pairing_seen: Option<String>,
    /// Text fields.
    mirror_address: String,
    storage_address: String,
    key_filter: String,
    key_group: KeyGroup,
    /// Short message at the bottom ("Copied Alt+O").
    toast: Option<(String, Instant)>,
    /// Page of the first-run guide.
    welcome_step: usize,
    /// QR code shown for pairing a phone over WiFi.
    wifi_pairing: Option<wifi_pair::Pairing>,
    /// GNOME keyboard shortcuts: None = not GNOME, Some(added).
    #[cfg(target_os = "linux")]
    gnome_shortcuts: Option<bool>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, shared: Arc<Shared>) -> Self {
        let ctx = cc.egui_ctx.clone();
        shared.set_repaint(move || ctx.request_repaint());
        crate::background::set_window(Some(cc.egui_ctx.clone()));
        shared
            .has_gui
            .store(true, std::sync::atomic::Ordering::Relaxed);
        kit::install_fonts(&cc.egui_ctx);
        Self {
            shared,
            monitors: monitor_choices(),
            addresses: local_addresses(),
            addresses_at: Instant::now(),
            host_name: crate::discovery::host_name(),
            applied_dark: None,
            logo: load_logo(&cc.egui_ctx),
            // SMON_TAB=mirror|storage and SMON_DRAWER=security|options|log
            // open that page (for screenshots).
            tab: match std::env::var("SMON_TAB").as_deref() {
                Ok("mirror") => Tab::Mirror,
                Ok("storage") | Ok("shortcuts") => Tab::Storage,
                _ => Tab::Screen,
            },
            drawer: match std::env::var("SMON_DRAWER").as_deref() {
                Ok("security") => Some(Drawer::Security),
                Ok("options") => Some(Drawer::Options),
                Ok("log") => Some(Drawer::Log),
                _ => None,
            },
            mirror_address: String::new(),
            storage_address: String::new(),
            key_filter: String::new(),
            key_group: KeyGroup::All,
            toast: None,
            pairing_seen: None,
            welcome_step: 0,
            wifi_pairing: None,
            #[cfg(target_os = "linux")]
            gnome_shortcuts: crate::hotkeys::gnome::available()
                .then(crate::hotkeys::gnome::installed),
        }
    }

    fn palette(&self) -> &'static Palette {
        if self.shared.settings().dark_theme {
            &kit::DARK
        } else {
            &kit::LIGHT
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
        let solid = solid_fill(p);
        let field = if dark {
            Color32::from_rgb(0x15, 0x1a, 0x2a)
        } else {
            Color32::from_rgb(0xf1, 0xf4, 0xfa)
        };
        visuals.panel_fill = p.bg;
        visuals.window_fill = solid;
        visuals.window_stroke = Stroke::new(1.0, p.inset_border);
        visuals.window_corner_radius = CornerRadius::same(14);
        visuals.menu_corner_radius = CornerRadius::same(12);
        visuals.extreme_bg_color = field;
        visuals.selection.bg_fill = kit::PURPLE;
        visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        visuals.hyperlink_color = kit::ink(p, kit::PURPLE);
        visuals.override_text_color = None;
        for w in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(10);
        }
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.inset_border);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
        visuals.widgets.inactive.weak_bg_fill = field;
        visuals.widgets.inactive.bg_fill = field;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, p.inset_border);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
        visuals.widgets.hovered.weak_bg_fill = kit::lerp(field, kit::PURPLE, 0.12);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, kit::alpha(kit::PURPLE, 0.5));
        visuals.widgets.active.weak_bg_fill = kit::lerp(field, kit::PURPLE, 0.2);
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
            s.spacing.interact_size.y = 32.0;
            s.spacing.combo_height = 300.0;
        });
    }

    /// A frosted card filling the column.
    fn card(&mut self, ui: &mut Ui, add: impl FnOnce(&mut Self, &mut Ui)) {
        let p = self.palette();
        kit::glass(p, 26, 20).show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(self, ui);
        });
        ui.add_space(16.0);
    }

    fn update_setting(&self, f: impl FnOnce(&mut crate::state::Settings)) {
        self.shared.update_settings(f);
    }

    fn copy(&mut self, ctx: &egui::Context, text: &str, what: &str) {
        ctx.copy_text(text.to_string());
        self.toast = Some((format!("Copied {what}"), Instant::now()));
    }

    // ------------------------------------------------------------ header

    fn header(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let status = self.shared.status().clone();
        let settings = self.shared.settings();
        let log_count = self.shared.log.lock().unwrap().len();
        kit::glass(p, 28, 14).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let wide = ui.available_width() > 700.0;
            let brand = |app: &mut App, ui: &mut Ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(42.0), Sense::hover());
                    ui.painter().circle_filled(
                        rect.center(),
                        21.0,
                        Color32::from_rgb(0x0f, 0x17, 0x2a),
                    );
                    ui.painter().circle_stroke(
                        rect.center(),
                        20.5,
                        Stroke::new(1.5, kit::alpha(kit::CYAN, 0.6)),
                    );
                    if let Some(logo) = &app.logo {
                        egui::Image::new((logo.id(), Vec2::splat(30.0)))
                            .corner_radius(8)
                            .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(30.0)));
                    }
                    ui.label(RichText::new("Extendo").font(kit::bold(19.0)).color(p.text));
                    let (color, text) = match &status.server_error {
                        Some(_) => (kit::RED, format!("Host · {} · stopped", app.host_name)),
                        None => (
                            kit::GREEN,
                            format!("Host · {} · Port {}", app.host_name, settings.port),
                        ),
                    };
                    Frame::default()
                        .fill(p.field)
                        .stroke(Stroke::new(1.0, p.inset_border))
                        .corner_radius(CornerRadius::same(255))
                        .inner_margin(Margin::symmetric(10, 4))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                kit::dot(ui, color, 3.5);
                                ui.label(RichText::new(text).font(kit::mono(11.5)).color(p.muted));
                            });
                        });
                });
            };
            let actions = |app: &mut App, ui: &mut Ui| {
                // Right to left.
                let dark = settings.dark_theme;
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::click());
                ui.painter().circle(
                    rect.center(),
                    17.5,
                    if resp.hovered() {
                        p.inset_border
                    } else {
                        p.field
                    },
                    Stroke::new(1.0, p.inset_border),
                );
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    if dark { "☀" } else { "🌙" },
                    FontId::proportional(15.0),
                    p.text,
                );
                if resp
                    .on_hover_text("Switch theme")
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .clicked()
                {
                    app.update_setting(|s| s.dark_theme = !dark);
                }
                let count = if log_count > 99 {
                    "99+".to_string()
                } else {
                    log_count.to_string()
                };
                if kit::chip(
                    ui,
                    p,
                    "⚡ Log",
                    kit::CYAN,
                    app.drawer == Some(Drawer::Log),
                    Some(&count),
                )
                .on_hover_text("Activity log and About")
                .clicked()
                {
                    app.drawer = Some(Drawer::Log);
                }
                if kit::chip(
                    ui,
                    p,
                    "⚙ Options",
                    kit::PURPLE,
                    app.drawer == Some(Drawer::Options),
                    None,
                )
                .clicked()
                {
                    app.drawer = Some(Drawer::Options);
                }
                let code = format!("🔒 {} · TLS", status.pairing_code);
                if kit::chip(
                    ui,
                    p,
                    &code,
                    kit::GREEN,
                    app.drawer == Some(Drawer::Security),
                    None,
                )
                .on_hover_text("Security: this PC's code and trusted tablets")
                .clicked()
                {
                    app.drawer = Some(Drawer::Security);
                }
            };
            if wide {
                ui.horizontal(|ui| {
                    brand(self, ui);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| actions(self, ui));
                });
            } else {
                brand(self, ui);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| actions(self, ui));
                });
            }
            ui.add_space(10.0);
            self.tabs(ui);
        });
        ui.add_space(18.0);
    }

    fn tabs(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 44.0), Sense::hover());
        ui.painter().rect(
            rect,
            CornerRadius::same(18),
            p.field,
            Stroke::new(1.0, p.inset_border),
            egui::StrokeKind::Inside,
        );
        let tabs = [
            (Tab::Screen, "🖥", "Spatial Screen", kit::SCREEN_GRAD),
            (Tab::Mirror, "📱", "Phone Mirror", kit::MIRROR_GRAD),
            (Tab::Storage, "📂", "Storage & Keys", kit::STORAGE_GRAD),
        ];
        let inner = rect.shrink(4.0);
        let seg = inner.width() / tabs.len() as f32;
        let narrow = seg < 150.0;
        for (i, (tab, icon, label, grad)) in tabs.into_iter().enumerate() {
            let r = Rect::from_min_size(
                Pos2::new(inner.left() + seg * i as f32, inner.top()),
                Vec2::new(seg, inner.height()),
            );
            let resp = ui
                .interact(r, ui.id().with(("tab", i)), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand);
            let selected = self.tab == tab;
            if selected {
                ui.painter().rect_filled(
                    r.translate(Vec2::new(0.0, 3.0)),
                    CornerRadius::same(15),
                    kit::alpha(grad.0, 0.22),
                );
                kit::gradient_rect(ui, r, 14.0, grad.0, grad.1);
            } else if resp.hovered() {
                ui.painter()
                    .rect_filled(r, CornerRadius::same(14), kit::alpha(grad.0, 0.08));
            }
            let fg = if selected { Color32::WHITE } else { p.muted };
            let text = if narrow {
                format!("{icon}  {}", label.split(' ').next().unwrap_or(label))
            } else {
                format!("{icon}  {label}")
            };
            ui.painter().text(
                r.center(),
                Align2::CENTER_CENTER,
                text,
                if selected {
                    kit::semi(13.5)
                } else {
                    FontId::proportional(13.5)
                },
                fg,
            );
            if resp.clicked() {
                self.tab = tab;
            }
        }
    }

    // ------------------------------------------------------------ screen tab

    /// Status and the quick actions.
    fn ribbon(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let status = self.shared.status().clone();
        kit::glass(p, 24, 14).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let connected = status.viewer.is_some();
            let wide = ui.available_width() > 720.0;
            let pill = |ui: &mut Ui| match (&status.server_error, &status.viewer) {
                (Some(_), _) => {
                    kit::status_pill(ui, p, "Not running", kit::RED);
                }
                (None, Some(v)) => {
                    let via = if v.usb {
                        "USB cable".to_string()
                    } else {
                        format!("WiFi · {}", v.address)
                    };
                    kit::status_pill(ui, p, &format!("Connected · {via}"), kit::GREEN);
                }
                (None, None) => {
                    kit::status_pill(
                        ui,
                        p,
                        "Waiting for device — open Extendo on your tablet and tap Connect",
                        kit::AMBER,
                    );
                }
            };
            let buttons = |app: &mut App, ui: &mut Ui| {
                if connected && kit::gradient_button(ui, "📤  Send files", kit::SCREEN_GRAD, true).clicked() {
                    app.pick_and_send();
                }
                if kit::soft_button(ui, p, "Extend ↔ Mirror", Some("Ctrl+Alt+E"))
                    .on_hover_text("Switch the mode; a connected tablet reconnects with it")
                    .clicked()
                {
                    crate::hotkeys::run(crate::hotkeys::Action::ToggleMode);
                }
            };
            if wide {
                ui.horizontal(|ui| {
                    pill(ui);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| buttons(self, ui));
                });
            } else {
                pill(ui);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        // Same buttons, left aligned.
                        if kit::soft_button(ui, p, "Extend ↔ Mirror", Some("Ctrl+Alt+E")).clicked() {
                            crate::hotkeys::run(crate::hotkeys::Action::ToggleMode);
                        }
                        if connected
                            && kit::gradient_button(ui, "📤  Send files", kit::SCREEN_GRAD, true).clicked()
                        {
                            self.pick_and_send();
                        }
                    });
                });
            }
            if let Some(err) = &status.server_error {
                ui.add_space(6.0);
                ui.label(RichText::new(err).color(kit::ink(p, kit::RED)));
            }
            if let Some(v) = &status.viewer {
                ui.add_space(10.0);
                kit::inset(p, 14).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(&v.name).font(kit::bold(18.0)).color(p.text));
                        if v.width > 0 {
                            kit::badge(ui, p, &format!("{} × {}", v.width, v.height), kit::CYAN);
                        }
                    });
                    if let Some(d) = v.device_status {
                        let mut parts = Vec::new();
                        if let Some(b) = d.battery {
                            let charging = if d.charging { " ⚡ charging" } else { "" };
                            parts.push(format!("🔋 Battery {b}%{charging}"));
                        }
                        if let Some(t) = d.temperature {
                            parts.push(format!("🌡 {t:.1} °C"));
                        }
                        let low = d.battery.is_some_and(|b| b <= 15) && !d.charging;
                        let hot = d.temperature.is_some_and(|t| t >= 42.0);
                        if !parts.is_empty() {
                            let color = if low || hot { kit::ink(p, kit::AMBER) } else { p.muted };
                            ui.label(RichText::new(parts.join("   ·   ")).color(color))
                                .on_hover_text("Reported by the tablet");
                        }
                    }
                    match v.pen {
                        PenSupport::Ready => {
                            ui.label(RichText::new("✏ Pen pressure on").color(kit::ink(p, kit::GREEN))).on_hover_text(
                                "The tablet's stylus works as a drawing tablet with pressure and tilt (Krita, GIMP, …).",
                            );
                        }
                        PenSupport::NeedsSetup => {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new("✏ Pen pressure needs a one-time setup.")
                                        .color(kit::ink(p, kit::AMBER)),
                                );
                                #[cfg(target_os = "linux")]
                                if kit::link_button(ui, p, "Copy the command", kit::AMBER)
                                    .on_hover_text(format!(
                                        "Run it in a terminal, then reconnect:\n{UINPUT_FIX}"
                                    ))
                                    .clicked()
                                {
                                    self.copy(ui.ctx(), UINPUT_FIX, "the setup command");
                                }
                            });
                        }
                        PenSupport::Unsupported => {}
                    }
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        stat(ui, p, &format!("{:.0}", v.fps), "fps", kit::PURPLE);
                        stat(ui, p, &format!("{:.1}", v.mbps), "Mbit/s", kit::CYAN);
                        stat(ui, p, &format!("{:.0} ms", v.encode_ms), "encode", kit::PINK);
                        ui.add_space(8.0);
                        let reconnect = kit::link_button(ui, p, "⟳  Reconnect", kit::PURPLE)
                            .on_hover_text("Apply changed settings now")
                            .clicked();
                        let disconnect = kit::link_button(ui, p, "✖  Disconnect", kit::RED).clicked();
                        if reconnect || disconnect {
                            self.shared.command(SessionCommand::Disconnect);
                        }
                    });
                });
            }
        });
        ui.add_space(16.0);
    }

    /// The tablet and this PC side by side, then the display settings.
    fn stage_card(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let s = self.shared.settings();
        let status = self.shared.status().clone();
        let viewer = status.viewer.clone();
        let connected = viewer.is_some();
        self.card(ui, |app, ui| {
            kit::section_title(
                ui,
                p,
                "DISPLAY STAGE",
                kit::PURPLE,
                "how the tablet sits next to this PC",
            );
            ui.add_space(10.0);
            let monitor = s
                .monitor
                .as_ref()
                .and_then(|m| app.monitors.iter().find(|(v, _)| v == m))
                .map(|(_, l)| l.clone())
                .unwrap_or_else(|| "Automatic".into());
            let link = match &viewer {
                Some(v) => format!("{:.0} fps · {:.1} Mbit/s", v.fps, v.mbps),
                None => format!("{} Hz · {} Mbit/s", s.fps, s.bitrate_mbps),
            };
            let info = StageInfo {
                mode: s.mode,
                scale: s.scale,
                fps: viewer
                    .as_ref()
                    .map(|v| v.fps.round() as u32)
                    .unwrap_or(s.fps),
                link,
                tablet: viewer.as_ref().map(|v| v.name.clone()),
                monitor,
                host: app.host_name.clone(),
                time: ui.input(|i| i.time) as f32,
            };
            kit::inset(p, 0)
                .corner_radius(CornerRadius::same(20))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    paint_stage(ui, p, &info, connected);
                });
            if connected {
                ui.ctx().request_repaint_after(Duration::from_millis(60));
            }
            ui.add_space(14.0);
            kit::inset(p, 16).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let gap = 18.0;
                let (cols, col_w) = kit::grid_columns(ui.available_width(), 300.0, 2, gap);
                let mode = |app: &mut App, ui: &mut Ui, w: f32| {
                    kit::field_label(ui, p, "Mode");
                    if let Some(m) = kit::segmented(
                        ui,
                        p,
                        &[(Mode::Extend, "Extend"), (Mode::Mirror, "Mirror")],
                        s.mode,
                        kit::SCREEN_GRAD,
                        w,
                    ) {
                        app.update_setting(|s| s.mode = m);
                    }
                };
                let monitor = |app: &mut App, ui: &mut Ui, w: f32| {
                    kit::field_label(ui, p, "Monitor");
                    let current = s.monitor.clone().unwrap_or_default();
                    let label = app
                        .monitors
                        .iter()
                        .find(|(v, _)| *v == current)
                        .map(|(_, l)| l.clone())
                        .unwrap_or_else(|| "Automatic".into());
                    egui::ComboBox::from_id_salt("monitor")
                        .selected_text(RichText::new(label).font(kit::semi(13.0)))
                        .width(w - 8.0)
                        .height(300.0)
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
                };
                let scale = |app: &mut App, ui: &mut Ui, w: f32| {
                    kit::field_label(ui, p, "Scale");
                    let current = SCALES
                        .iter()
                        .map(|(v, _)| *v)
                        .find(|v| (*v - s.scale).abs() < 0.01)
                        .unwrap_or(s.scale);
                    let options: Vec<(u32, &str)> = SCALES
                        .iter()
                        .map(|(v, l)| ((v * 100.0) as u32, *l))
                        .collect();
                    if let Some(v) = kit::segmented(
                        ui,
                        p,
                        &options,
                        (current * 100.0) as u32,
                        kit::SCREEN_GRAD,
                        w,
                    )
                    .map(|v| v as f32 / 100.0)
                    {
                        app.update_setting(|s| s.scale = v);
                    }
                    ui.label(
                        RichText::new("Makes text and windows bigger on the tablet")
                            .size(11.5)
                            .color(p.faint),
                    );
                };
                let fps = |app: &mut App, ui: &mut Ui, w: f32| {
                    kit::field_label(ui, p, "Frame rate (Hz)");
                    if let Some(f) = kit::segmented(
                        ui,
                        p,
                        &[(30, "30"), (60, "60"), (90, "90"), (120, "120")],
                        s.fps,
                        kit::SCREEN_GRAD,
                        w,
                    ) {
                        app.update_setting(|s| s.fps = f);
                    }
                    ui.label(
                        RichText::new("Used when the app's quality is set to Auto")
                            .size(11.5)
                            .color(p.faint),
                    );
                };
                let rows: [[&Field<'_>; 2]; 2] = [[&mode, &monitor], [&scale, &fps]];
                for row in rows {
                    if cols == 2 {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            for f in row {
                                ui.allocate_ui(Vec2::new(col_w, 0.0), |ui| {
                                    ui.vertical(|ui| {
                                        ui.set_width(col_w);
                                        f(app, ui, col_w);
                                    });
                                });
                            }
                        });
                    } else {
                        for f in row {
                            f(app, ui, col_w);
                            ui.add_space(6.0);
                        }
                    }
                    ui.add_space(10.0);
                }
                ui.horizontal(|ui| {
                    kit::field_label(ui, p, "Bitrate");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        kit::badge(ui, p, &format!("{} Mbit/s", s.bitrate_mbps), kit::CYAN);
                    });
                });
                let mut bitrate = s.bitrate_mbps;
                if kit::gradient_slider(
                    ui,
                    p,
                    &mut bitrate,
                    2..=60,
                    kit::SCREEN_GRAD,
                    ui.available_width(),
                )
                .on_hover_text("Higher = sharper, needs a faster link")
                .changed()
                {
                    app.update_setting(|s| s.bitrate_mbps = bitrate);
                }
            });
            app.virtual_display_notes(ui, &s);
            ui.add_space(8.0);
            let owner = match &viewer {
                Some(v) => Some(v.name.clone()),
                None => s
                    .profiles
                    .iter()
                    .find(|p| p.id == s.last_device)
                    .map(|p| p.name.clone()),
            };
            if let Some(name) = owner.filter(|_| s.profiles.len() > 1 || connected) {
                kit::note(
                    ui,
                    p,
                    &format!("Saved for {name}: each tablet keeps its own display settings."),
                );
            }
            ui.horizontal(|ui| {
                kit::note(ui, p, "Changes apply on the next connection.");
                if connected && kit::link_button(ui, p, "Apply now", kit::PURPLE).clicked() {
                    app.shared.command(SessionCommand::Disconnect);
                }
            });
        });
    }

    /// Windows: the virtual display driver.
    fn virtual_display_notes(&mut self, ui: &mut Ui, s: &crate::state::Settings) {
        let p = self.palette();
        let installed = backend::virtual_display_installed();
        if backend::virtual_display_can_stay_on() {
            ui.add_space(10.0);
            let mut manage = s.manage_virtual_display;
            if kit::option_tile(
                ui,
                p,
                "🖥",
                "Virtual display only while a tablet is connected (recommended)",
                &mut manage,
                kit::PURPLE,
                ui.available_width(),
            )
            .on_hover_text(
                "Off: Virtual Display Driver's screen stays on all the time, for other uses.",
            )
            .changed()
            {
                self.update_setting(|s| s.manage_virtual_display = manage);
            }
            if s.mode == Mode::Extend
                && kit::link_button(
                    ui,
                    p,
                    "Switch to Parsec Virtual Display Driver",
                    kit::PURPLE,
                )
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
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    kit::note(
                        ui,
                        p,
                        &format!("The virtual display has no {w}×{h} size yet, so the tablet letterboxes."),
                    );
                    if kit::link_button(ui, p, &format!("Add {w}×{h}"), kit::PURPLE)
                        .on_hover_text("Windows asks for admin approval. Reconnect the tablet afterwards.")
                        .clicked()
                    {
                        backend::install_virtual_display(Some((w, h)));
                    }
                });
            }
        }
        if s.mode == Mode::Extend && installed == Some(false) {
            ui.add_space(10.0);
            kit::tinted(p, kit::AMBER, 14).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("Extend needs a virtual display on Windows")
                        .font(kit::semi(14.0))
                        .color(p.text),
                );
                kit::note(
                    ui,
                    p,
                    "Windows can't add a screen by itself, so without one Extendo \
                     mirrors. Install Parsec's free, signed virtual display driver \
                     once; Extendo then adds the tablet's screen while it is \
                     connected and removes it afterwards, even if Extendo crashes.",
                );
                ui.add_space(4.0);
                if kit::gradient_button(ui, "🖥  Set up extend", kit::SCREEN_GRAD, true)
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
    }

    /// The two cards under the stage: connecting, and files.
    fn pods(&mut self, ui: &mut Ui) {
        let wide = ui.available_width() > 760.0;
        if wide {
            ui.columns(2, |cols| {
                self.connect_pod(&mut cols[0]);
                self.files_pod(&mut cols[1]);
            });
        } else {
            self.connect_pod(ui);
            self.files_pod(ui);
        }
    }

    fn connect_pod(&mut self, ui: &mut Ui) {
        let p = self.palette();
        if self.addresses_at.elapsed() > Duration::from_secs(10) {
            self.addresses = local_addresses();
            self.addresses_at = Instant::now();
        }
        let status = self.shared.status().clone();
        let settings = self.shared.settings();
        self.card(ui, |app, ui| {
            kit::section_title(ui, p, "INSTANT CONNECT", kit::GREEN, "");
            ui.add_space(8.0);
            ui.label(
                RichText::new("📶  Over WiFi")
                    .font(kit::semi(14.0))
                    .color(p.text),
            );
            if !settings.wifi {
                kit::note(ui, p, "Turned off in Options (USB only).");
            } else if app.addresses.is_empty() {
                ui.label(RichText::new("No network connection").color(kit::ink(p, kit::AMBER)));
            } else {
                for ip in app.addresses.clone() {
                    let text = format!("{ip}:{}", settings.port);
                    let resp = kit::inset(p, 10)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                kit::dot(ui, kit::GREEN, 3.5);
                                ui.label(
                                    RichText::new(&text)
                                        .font(kit::mono(13.5))
                                        .color(kit::ink(p, kit::CYAN)),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new("click to copy").size(11.0).color(p.faint),
                                    );
                                });
                            });
                        })
                        .response
                        .interact(Sense::click())
                        .on_hover_cursor(CursorIcon::PointingHand);
                    if resp.clicked() {
                        app.copy(ui.ctx(), &text, &text);
                    }
                    ui.add_space(2.0);
                }
                kit::note(
                    ui,
                    p,
                    "Same network; the tablet app finds this PC by itself.",
                );
            }
            ui.add_space(10.0);
            ui.label(
                RichText::new("🔌  Over USB cable")
                    .font(kit::semi(14.0))
                    .color(p.text),
            );
            if !settings.usb {
                kit::note(ui, p, "Turned off in Options.");
            } else if status.adb_path.is_none() {
                ui.label(RichText::new("adb not found").color(kit::ink(p, kit::AMBER)));
                kit::note(ui, p, "Install Android platform-tools to use USB.");
            } else if status.usb_devices.iter().all(|d| d.wifi) {
                kit::note(ui, p, "Plug in the tablet with USB debugging on.");
            } else {
                for d in status.usb_devices.iter().filter(|d| !d.wifi) {
                    ui.label(
                        RichText::new(format!("✔ {}", d.model)).color(kit::ink(p, kit::GREEN)),
                    );
                }
                kit::note(ui, p, "Tap \"USB cable\" in the app.");
            }
        });
    }

    fn files_pod(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let status = self.shared.status().clone();
        let settings = self.shared.settings();
        let connected = status.viewer.is_some();
        let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        self.card(ui, |app, ui| {
            kit::section_title(ui, p, "FILE DROP PORTAL", kit::ORANGE, "");
            ui.add_space(8.0);
            let (rect, resp) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), 92.0),
                if connected {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            );
            let lit = connected && (resp.hovered() || hovering);
            ui.painter().rect_filled(
                rect,
                CornerRadius::same(16),
                if lit {
                    kit::tint(p, kit::ORANGE)
                } else {
                    p.inset
                },
            );
            let r = rect.shrink(1.0);
            let pts = [
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
                r.left_top(),
            ];
            ui.painter().extend(egui::Shape::dashed_line(
                &pts,
                Stroke::new(1.5, kit::alpha(kit::ORANGE, if lit { 0.9 } else { 0.45 })),
                7.0,
                5.0,
            ));
            ui.painter().text(
                rect.center() - Vec2::new(0.0, 12.0),
                Align2::CENTER_CENTER,
                if connected {
                    "📤  Drop files here"
                } else {
                    "📤  Connect a tablet first"
                },
                kit::semi(14.5),
                p.text,
            );
            ui.painter().text(
                rect.center() + Vec2::new(0.0, 13.0),
                Align2::CENTER_CENTER,
                if connected {
                    "or click to choose; they go to the tablet's Downloads"
                } else {
                    "then drop files on this window to send them"
                },
                FontId::proportional(12.0),
                p.muted,
            );
            if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                app.pick_and_send();
            }
            for t in status.transfers.iter().rev().take(8) {
                ui.horizontal(|ui| {
                    let arrow = if t.incoming { "⬇" } else { "⬆" };
                    ui.label(RichText::new(arrow).color(kit::ink(p, kit::ORANGE)));
                    ui.add(egui::Label::new(RichText::new(&t.name).color(p.text)).truncate());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &t.state {
                        TransferState::Running => {
                            let frac = if t.size > 0 {
                                t.done as f32 / t.size as f32
                            } else {
                                0.0
                            };
                            ui.add(
                                egui::ProgressBar::new(frac)
                                    .desired_width(130.0)
                                    .fill(kit::ORANGE)
                                    .show_percentage(),
                            );
                        }
                        TransferState::Done => {
                            if t.incoming {
                                if let Some(path) = &t.path {
                                    if kit::link_button(ui, p, "Open", kit::ORANGE).clicked() {
                                        crate::desktop::open_file(path);
                                    }
                                }
                            }
                            ui.label(
                                RichText::new(format!("✔ {}", human_size(t.size)))
                                    .color(kit::ink(p, kit::GREEN)),
                            );
                        }
                        TransferState::Failed(e) => {
                            ui.label(RichText::new(format!("✖ {e}")).color(kit::ink(p, kit::RED)));
                        }
                    });
                });
            }
            ui.add_space(8.0);
            kit::inset(p, 10).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("RECEIVED FILES")
                        .font(kit::semi(10.5))
                        .color(p.muted),
                );
                let dir = settings.save_dir.display().to_string();
                ui.add(
                    egui::Label::new(RichText::new(&dir).font(kit::mono(12.0)).color(p.text))
                        .truncate(),
                )
                .on_hover_text(dir);
                ui.horizontal_wrapped(|ui| {
                    if kit::link_button(ui, p, "📂 Open folder", kit::ORANGE).clicked() {
                        let _ = std::fs::create_dir_all(&settings.save_dir);
                        crate::desktop::open_in_file_manager(
                            &settings.save_dir.display().to_string(),
                        );
                    }
                    if kit::link_button(ui, p, "Change…", kit::ORANGE).clicked() {
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
                        && kit::link_button(ui, p, "Clear finished", kit::ORANGE).clicked()
                    {
                        app.shared.update_status(|s| {
                            s.transfers.retain(|t| t.state == TransferState::Running)
                        });
                    }
                });
            });
        });
    }

    // ------------------------------------------------------------ mirror tab

    fn mirror_stage_card(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let status = self.shared.status().clone();
        let settings = self.shared.settings();
        self.card(ui, |app, ui| {
            kit::section_title(
                ui,
                p,
                "PHONE STAGE",
                kit::PINK,
                "see and control your phone on this PC, with sound",
            );
            ui.add_space(10.0);
            let engine = status.scrcpy.clone();
            if engine.is_none() {
                kit::tinted(p, kit::AMBER, 14).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new("⚠  Mirroring engine not found")
                            .font(kit::semi(14.0))
                            .color(kit::ink(p, kit::AMBER)),
                    );
                    let hint = if cfg!(windows) {
                        "The mirror engine is missing next to extendo.exe; reinstall Extendo."
                    } else {
                        "The mirror engine is missing; reinstall the Extendo package."
                    };
                    kit::note(ui, p, hint);
                });
                ui.add_space(10.0);
            } else if status.adb_path.is_none() {
                kit::tinted(p, kit::AMBER, 14).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new("adb not found: install Android platform-tools.")
                            .color(kit::ink(p, kit::AMBER)),
                    );
                });
                ui.add_space(10.0);
            }
            let first = status.usb_devices.first().cloned();
            let phone = PhoneInfo {
                state: if engine.is_none() {
                    PhoneState::NoEngine
                } else if let Some(d) = &first {
                    if status.mirroring.contains(&d.serial) {
                        PhoneState::Mirroring(d.model.clone())
                    } else {
                        PhoneState::Ready(d.model.clone())
                    }
                } else {
                    PhoneState::NoPhone
                },
                m: settings.mirror.clone(),
            };
            let ready = engine.is_some() && status.adb_path.is_some();
            let wide = ui.available_width() > 680.0;
            if wide {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui(Vec2::new(230.0, 420.0), |ui| {
                        ui.vertical_centered(|ui| paint_phone(ui, p, &phone, 1.0));
                    });
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        if ready {
                            app.mirror_devices(ui);
                        } else {
                            kit::note(
                                ui,
                                p,
                                "Phones show up here once the engine and adb are available.",
                            );
                        }
                    });
                });
            } else {
                ui.vertical_centered(|ui| paint_phone(ui, p, &phone, 0.85));
                ui.add_space(10.0);
                if ready {
                    app.mirror_devices(ui);
                }
            }
        });
    }

    fn mirror_devices(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let status = self.shared.status().clone();
        let settings = self.shared.settings();
        let version = status
            .scrcpy
            .as_ref()
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        if status.usb_devices.is_empty() {
            kit::note(
                ui,
                p,
                "No phone found. Plug one in with USB debugging on, or connect over WiFi below.",
            );
            ui.add_space(6.0);
        }
        for d in status.usb_devices.clone() {
            let running = status.mirroring.contains(&d.serial);
            let busy = status
                .mirror_busy
                .iter()
                .find(|(s, _)| *s == d.serial)
                .map(|(_, m)| m.clone());
            kit::inset(p, 12).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(if d.wifi { "📶" } else { "🔌" }).size(18.0));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&d.model).font(kit::semi(14.0)).color(p.text));
                        let how = if d.wifi {
                            format!("WiFi · {}", d.serial)
                        } else {
                            "USB cable".into()
                        };
                        ui.label(RichText::new(how).size(12.0).color(p.muted));
                    });
                    if running {
                        kit::status_pill(ui, p, "Mirroring", kit::GREEN);
                    }
                });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if running {
                        if kit::link_button(ui, p, "■  Stop", kit::RED).clicked() {
                            mirror::stop(&d.serial);
                        }
                        for (icon, tip, action) in [
                            ("◁", "Back", mirror::PhoneAction::Back),
                            ("○", "Home", mirror::PhoneAction::Home),
                            ("□", "Recent apps", mirror::PhoneAction::Recents),
                        ] {
                            if kit::link_button(ui, p, icon, kit::PINK)
                                .on_hover_text(tip)
                                .clicked()
                            {
                                mirror::phone_action(self.shared.clone(), d.serial.clone(), action);
                            }
                        }
                        ui.menu_button(
                            RichText::new("⋯ More").color(kit::ink(p, kit::PINK)),
                            |ui| {
                                use mirror::PhoneAction as A;
                                for (label, action) in [
                                    ("🔊  Volume up", A::VolumeUp),
                                    ("🔉  Volume down", A::VolumeDown),
                                    ("🔒  Power (lock / wake)", A::Power),
                                    ("🔔  Open notifications", A::Notifications),
                                    ("📷  Screenshot to Downloads/Extendo", A::Screenshot),
                                ] {
                                    if ui.button(label).clicked() {
                                        mirror::phone_action(
                                            self.shared.clone(),
                                            d.serial.clone(),
                                            action,
                                        );
                                        ui.close();
                                    }
                                }
                            },
                        );
                    } else if kit::gradient_button(ui, "▶  Mirror", kit::MIRROR_GRAD, true)
                        .clicked()
                    {
                        mirror::start(self.shared.clone(), d.clone());
                    }
                    if d.wifi_debugging
                        && kit::link_button(ui, p, "🔒 Turn off WiFi", kit::AMBER)
                            .on_hover_text(
                                "Close the phone's debugging port on the network (5555). \
                                 Mirroring then needs the cable again.",
                            )
                            .clicked()
                    {
                        mirror::disable_wifi(self.shared.clone(), d.clone());
                    }
                    if d.wifi {
                        if kit::link_button(ui, p, "Forget", kit::RED)
                            .on_hover_text("Disconnect and remove")
                            .clicked()
                        {
                            mirror::disconnect(self.shared.clone(), d.serial.clone(), true);
                        }
                    } else if let Some(msg) = &busy {
                        ui.spinner();
                        ui.label(RichText::new(msg).size(12.0).color(p.muted));
                    } else if !d.wifi_debugging
                        && kit::link_button(ui, p, "📶 Use WiFi", kit::CYAN)
                            .on_hover_text("Switch this phone to WiFi so you can unplug the cable")
                            .clicked()
                    {
                        mirror::enable_wifi(self.shared.clone(), d.clone());
                    }
                });
            });
            ui.add_space(6.0);
        }
        ui.add_space(4.0);
        kit::field_label(ui, p, "Connect by address");
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 110.0).clamp(120.0, 260.0);
            kit::text_field(ui, p, &mut self.mirror_address, "192.168.1.50", w);
            if kit::link_button(ui, p, "Connect", kit::PINK).clicked()
                && !self.mirror_address.trim().is_empty()
            {
                mirror::connect(self.shared.clone(), self.mirror_address.trim().to_string());
            }
        });
        ui.add_space(6.0);
        if kit::gradient_button(
            ui,
            "▦  Pair with a QR code (Android 11+)",
            kit::MIRROR_GRAD,
            true,
        )
        .on_hover_text("No cable needed: scan the code from the phone's Wireless debugging screen")
        .clicked()
        {
            self.wifi_pairing = wifi_pair::start(self.shared.clone());
        }
        let offline: Vec<String> = settings
            .wifi_phones
            .iter()
            .filter(|a| !status.usb_devices.iter().any(|d| &d.serial == *a))
            .cloned()
            .collect();
        if !offline.is_empty() {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Saved:").size(12.0).color(p.muted));
                for a in offline {
                    if kit::link_button(ui, p, &format!("⟳ {a}"), kit::PINK)
                        .on_hover_text("Reconnect")
                        .clicked()
                    {
                        mirror::connect(self.shared.clone(), a.clone());
                    }
                }
            });
        }
        ui.add_space(4.0);
        kit::note(
            ui,
            p,
            &format!(
                "WiFi: plug the phone in once and press \"Use WiFi\". Android 11+ can also pair with Wireless debugging. · powered by {version}"
            ),
        );
        if status.viewer.as_ref().is_some_and(|v| v.usb) && !status.mirroring.is_empty() {
            ui.label(
                RichText::new(
                    "Tip: don't mirror the tablet that is showing your PC: it would show itself forever.",
                )
                .size(12.0)
                .color(kit::ink(p, kit::AMBER)),
            );
        }
    }

    fn mirror_options_card(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let m = self.shared.settings().mirror;
        self.card(ui, |app, ui| {
            let mut next = m.clone();
            kit::heading(
                ui,
                p,
                "Phone Mirror Stream & Behavior",
                "Works over a cable or WiFi. Changes apply to the next mirror window.",
            );
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);
            let gap = 16.0;
            let (cols, col_w) = kit::grid_columns(ui.available_width(), 220.0, 3, gap);
            let resolution = |ui: &mut Ui, next: &mut crate::state::MirrorSettings, w: f32| {
                kit::field_label(ui, p, "Resolution");
                let label = match next.max_size {
                    0 => "Full".to_string(),
                    n => format!("{n} px"),
                };
                egui::ComboBox::from_id_salt("mirror_size")
                    .selected_text(RichText::new(label).font(kit::semi(13.0)))
                    .width(w - 8.0)
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
            };
            let fps = |ui: &mut Ui, next: &mut crate::state::MirrorSettings, w: f32| {
                kit::field_label(ui, p, "Frame rate");
                if let Some(f) = kit::segmented(
                    ui,
                    p,
                    &[(30, "30"), (60, "60"), (90, "90"), (120, "120")],
                    next.max_fps,
                    kit::MIRROR_GRAD,
                    w,
                ) {
                    next.max_fps = f;
                }
            };
            let bitrate = |ui: &mut Ui, next: &mut crate::state::MirrorSettings, w: f32| {
                ui.horizontal(|ui| {
                    ui.set_width(w);
                    kit::field_label(ui, p, "Bitrate");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        kit::badge(ui, p, &format!("{} Mbit/s", next.bitrate_mbps), kit::PINK);
                    });
                });
                kit::gradient_slider(ui, p, &mut next.bitrate_mbps, 2..=32, kit::MIRROR_GRAD, w);
            };
            let parts: [&MirrorField<'_>; 3] = [&resolution, &fps, &bitrate];
            if cols == 3 {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for f in parts {
                        ui.allocate_ui(Vec2::new(col_w, 0.0), |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(col_w);
                                f(ui, &mut next, col_w);
                            });
                        });
                    }
                });
            } else {
                let w = ui.available_width();
                for f in parts {
                    f(ui, &mut next, w);
                    ui.add_space(8.0);
                }
            }
            ui.add_space(12.0);
            let tiles: [(&str, &str, bool, Color32); 11] = [
                (
                    "◁",
                    "Navigation bar at the bottom of the mirror window",
                    next.navbar,
                    kit::PINK,
                ),
                (
                    "🌙",
                    "Turn the phone screen off while mirroring (saves battery; Alt+O anytime)",
                    next.screen_off,
                    kit::PURPLE,
                ),
                (
                    "🔊",
                    "Play the phone's sound on this PC (Android 11+)",
                    next.audio,
                    kit::PINK,
                ),
                (
                    "☕",
                    "Keep the phone awake while plugged in",
                    next.stay_awake,
                    kit::ORANGE,
                ),
                (
                    "⌨",
                    "Act as a physical keyboard (games, keyboard layouts)",
                    next.physical_keyboard,
                    kit::CYAN,
                ),
                (
                    "👆",
                    "Show touches on the phone",
                    next.show_touches,
                    kit::PINK,
                ),
                (
                    "📌",
                    "Keep the mirror window on top",
                    next.always_on_top,
                    kit::ORANGE,
                ),
                ("⛶", "Start in fullscreen", next.fullscreen, kit::PURPLE),
                (
                    "🔒",
                    "Lock the phone when the window closes",
                    next.power_off_on_close,
                    kit::RED,
                ),
                (
                    "⏺",
                    "Record to a video in Downloads/Extendo/Recordings",
                    next.record,
                    kit::RED,
                ),
                (
                    "📷",
                    "Show the phone's camera instead of its screen (Android 12+)",
                    next.camera,
                    kit::CYAN,
                ),
            ];
            let (cols, w) = kit::grid_columns(ui.available_width(), 300.0, 2, 10.0);
            let mut values: Vec<bool> = tiles.iter().map(|t| t.2).collect();
            kit::flow(
                ui,
                tiles.iter().enumerate(),
                cols,
                10.0,
                |ui, (i, (icon, label, _, color))| {
                    kit::option_tile(ui, p, icon, label, &mut values[i], *color, w);
                },
            );
            [
                next.navbar,
                next.screen_off,
                next.audio,
                next.stay_awake,
                next.physical_keyboard,
                next.show_touches,
                next.always_on_top,
                next.fullscreen,
                next.power_off_on_close,
                next.record,
                next.camera,
            ] = values.try_into().unwrap_or([false; 11]);
            if next != m {
                app.update_setting(|s| s.mirror = next);
            }
        });
    }

    // ------------------------------------------------------------ storage tab

    fn storage_card(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let status = self.shared.status().clone();
        if self.storage_address.is_empty() {
            self.storage_address = self.shared.settings().storage_address;
        }
        self.card(ui, |app, ui| {
            kit::badge(ui, p, "PC ↔ Android phone · FTP bridge", kit::GREEN);
            ui.add_space(6.0);
            ui.label(
                RichText::new("Browse and copy your phone's files from this PC, like a USB drive.")
                    .font(kit::bold(17.0))
                    .color(p.text),
            );
            ui.add_space(8.0);
            let (cols, w) = kit::grid_columns(ui.available_width(), 260.0, 2, 12.0);
            let steps = [
                ("1", "Open Extendo on the phone, then Phone storage, then Start."),
                ("2", "Open it here. When asked, log in as \"extendo\" with the password shown on the phone."),
            ];
            kit::flow(ui, steps, cols, 12.0, |ui, (n, text)| {
                kit::inset(p, 12).show(ui, |ui| {
                    ui.set_width(w - 26.0);
                    ui.horizontal_top(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                        ui.painter().circle_filled(rect.center(), 12.0, kit::GREEN);
                        ui.painter().text(rect.center(), Align2::CENTER_CENTER, n, kit::bold(12.0), Color32::WHITE);
                        ui.add(egui::Label::new(RichText::new(text).size(13.0).color(p.text)).wrap());
                    });
                });
            });
            ui.add_space(4.0);
            kit::inset(p, 14).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let usb: Vec<AdbDevice> =
                    status.usb_devices.iter().filter(|d| !d.wifi).cloned().collect();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("🔌  Over USB cable").font(kit::semi(14.0)).color(p.text));
                    if usb.is_empty() {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new("No phone plugged in with USB debugging on.")
                                    .font(kit::mono(11.0))
                                    .color(p.faint),
                            );
                        });
                    }
                });
                for d in usb {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&d.model).color(p.text));
                        if kit::link_button(ui, p, "📂  Open files", kit::GREEN).clicked() {
                            storage::open_usb(app.shared.clone(), d.serial.clone());
                        }
                    });
                }
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("📶  Over WiFi").font(kit::semi(14.0)).color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if status.storage_searching {
                            ui.spinner();
                        } else if kit::link_button(ui, p, "⟳ Find phones", kit::GREEN).clicked() {
                            storage::search(app.shared.clone());
                        }
                    });
                });
                for ph in &status.storage_phones {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{}  ·  {}", ph.name, ph.address)).color(p.text),
                        );
                        if kit::link_button(ui, p, "📂  Open files", kit::GREEN).clicked() {
                            storage::open(&format!("{}:{}", ph.address, ph.port));
                        }
                    });
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 200.0).max(140.0);
                    kit::text_field(ui, p, &mut app.storage_address, "192.168.1.50", w);
                    if kit::gradient_button(ui, "Open in Files (ftp://)", kit::STORAGE_GRAD, true).clicked()
                        && !app.storage_address.trim().is_empty()
                    {
                        let address = app.storage_address.trim().to_string();
                        app.update_setting(|s| s.storage_address = address.clone());
                        storage::open(&address);
                    }
                });
                ui.add_space(4.0);
                kit::note(
                    ui,
                    p,
                    "Files opens the phone in your file manager (ftp://). Only use it on networks you trust.",
                );
            });
        });
    }

    fn shortcuts_card(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let mut all: Vec<(KeyGroup, &str, &str)> = mirror::SHORTCUTS
            .iter()
            .enumerate()
            .map(|(i, (k, w))| {
                (
                    MIRROR_GROUPS.get(i).copied().unwrap_or(KeyGroup::View),
                    *k,
                    *w,
                )
            })
            .collect();
        all.extend(
            TABLET_GESTURES
                .iter()
                .map(|(k, w)| (KeyGroup::Tablet, *k, *w)),
        );
        all.extend(PC_KEYS.iter().map(|(k, w)| (KeyGroup::Pc, *k, *w)));
        self.card(ui, |app, ui| {
            let wide = ui.available_width() > 640.0;
            let title = |ui: &mut Ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        kit::dot(ui, kit::GREEN, 4.0);
                        ui.label(RichText::new("Shortcut Deck").font(kit::bold(17.0)).color(p.text));
                    });
                    kit::note(
                        ui,
                        p,
                        "Mirror window keys: Alt can be replaced by the Super (Windows) key. Click a card to copy it.",
                    );
                });
            };
            let count = all.len();
            if wide {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui(Vec2::new(ui.available_width() - 240.0, 0.0), title);
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        kit::text_field(ui, p, &mut app.key_filter, &format!("Filter {count} shortcuts…"), 220.0);
                    });
                });
            } else {
                title(ui);
                kit::text_field(ui, p, &mut app.key_filter, &format!("Filter {count} shortcuts…"), ui.available_width());
            }
            ui.add_space(8.0);
            let filter = app.key_filter.trim().to_lowercase();
            let matches = |k: &str, w: &str| {
                filter.is_empty() || k.to_lowercase().contains(&filter) || w.to_lowercase().contains(&filter)
            };
            ui.horizontal_wrapped(|ui| {
                for g in KeyGroup::LIST {
                    let n = all
                        .iter()
                        .filter(|(gg, k, w)| (g == KeyGroup::All || *gg == g) && matches(k, w))
                        .count();
                    let label = format!("{} ({n})", g.label());
                    if kit::chip(ui, p, &label, g.color(), app.key_group == g, None).clicked() {
                        app.key_group = g;
                    }
                }
            });
            ui.add_space(10.0);
            let shown: Vec<(KeyGroup, &str, &str)> = all
                .iter()
                .copied()
                .filter(|(g, k, w)| (app.key_group == KeyGroup::All || *g == app.key_group) && matches(k, w))
                .collect();
            if shown.is_empty() {
                kit::note(ui, p, "No shortcut matches.");
            }
            let (cols, w) = kit::grid_columns(ui.available_width(), 300.0, 2, 10.0);
            let mut copied = None;
            kit::flow(ui, shown, cols, 10.0, |ui, (g, keys, what)| {
                if kit::keycap_card(ui, p, what, &keys.replace("  ", " "), g.color(), w)
                    .on_hover_text("Click to copy")
                    .clicked()
                {
                    copied = Some(keys.to_string());
                }
            });
            if let Some(k) = copied {
                app.copy(ui.ctx(), &k, &k);
            }
            if matches!(app.key_group, KeyGroup::All | KeyGroup::Pc) {
                ui.add_space(4.0);
                let how = if cfg!(windows) {
                    "Ctrl+Alt+E and Ctrl+Alt+X work in any app; turn them on or off under Options."
                } else {
                    "Ctrl+Alt+E and Ctrl+Alt+X: add them under Options (GNOME), or bind the commands in your desktop's keyboard settings."
                };
                kit::note(ui, p, how);
            }
        });
    }

    // ------------------------------------------------------------ drawers

    fn drawer(&mut self, ctx: &egui::Context) {
        let Some(which) = self.drawer else { return };
        let p = self.palette();
        let screen = ctx.content_rect();
        let width = (screen.width() - 24.0).min(400.0);
        let height = screen.height() - 24.0;
        let id = egui::Id::new("drawer");
        let frame = kit::glass(p, 24, 20).fill(solid_fill(p));
        let mut close = false;
        let response = egui::Modal::new(id)
            .area(egui::Modal::default_area(id).anchor(Align2::RIGHT_TOP, Vec2::new(-12.0, 12.0)))
            .frame(frame)
            .backdrop_color(kit::alpha(Color32::BLACK, 0.3))
            .show(ctx, |ui| {
                ui.set_width(width - 40.0);
                ui.set_height(height - 40.0);
                let (title, color) = match which {
                    Drawer::Security => ("Security & trusted tablets", kit::GREEN),
                    Drawer::Options => ("Host options", kit::PURPLE),
                    Drawer::Log => ("Activity log & About", kit::CYAN),
                };
                ui.horizontal(|ui| {
                    kit::dot(ui, color, 4.0);
                    ui.label(RichText::new(title).font(kit::bold(16.0)).color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if kit::link_button(ui, p, "×", color)
                            .on_hover_text("Close (Esc)")
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
                ui.add_space(10.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match which {
                        Drawer::Security => self.security_drawer(ui),
                        Drawer::Options => self.options_drawer(ui),
                        Drawer::Log => self.log_drawer(ui),
                    });
            });
        if close || response.should_close() {
            self.drawer = None;
        }
    }

    fn security_drawer(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let status = self.shared.status().clone();
        let settings = self.shared.settings();
        kit::tinted(p, kit::GREEN, 12).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("🔒  Encrypted")
                    .font(kit::semi(14.0))
                    .color(kit::ink(p, kit::GREEN)),
            );
            kit::note(ui, p, "All connections use TLS.");
        });
        ui.add_space(10.0);
        kit::inset(p, 16).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("THIS PC'S CODE")
                        .font(kit::semi(11.0))
                        .color(p.muted),
                );
                ui.label(
                    RichText::new(&status.pairing_code)
                        .font(kit::mono(34.0))
                        .color(kit::ink(p, kit::GREEN)),
                );
            });
            kit::note(
                ui,
                p,
                "A new tablet on WiFi must be allowed here first, and shows this same code. \
                 If the codes differ, someone may be intercepting: deny it.",
            );
        });
        ui.add_space(12.0);
        kit::field_label(ui, p, "Trusted tablets");
        if settings.trusted_devices.is_empty() {
            kit::note(ui, p, "None yet. USB connections are always allowed.");
        }
        for d in &settings.trusted_devices {
            kit::inset(p, 10).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("📱 {}", d.name)).color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if kit::link_button(ui, p, "Remove", kit::RED).clicked() {
                            let id = d.id.clone();
                            self.update_setting(|s| s.trusted_devices.retain(|t| t.id != id));
                        }
                    });
                });
            });
            ui.add_space(4.0);
        }
        ui.add_space(10.0);
        let mut usb_only = !settings.wifi;
        if kit::option_tile(
            ui,
            p,
            "🔌",
            "USB only: close the network port (WiFi tablets can't connect)",
            &mut usb_only,
            kit::AMBER,
            ui.available_width(),
        )
        .changed()
        {
            self.update_setting(|s| s.wifi = !usb_only);
        }
    }

    fn options_drawer(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let s = self.shared.settings();
        let w = ui.available_width();
        let tile =
            |app: &mut App, ui: &mut Ui, icon: &str, label: &str, value: bool, color: Color32| {
                let mut v = value;
                let changed = kit::option_tile(ui, p, icon, label, &mut v, color, w).changed();
                ui.add_space(6.0);
                let _ = app;
                changed.then_some(v)
            };
        if let Some(v) = tile(
            self,
            ui,
            "📋",
            "Sync clipboard with the tablet",
            s.clipboard,
            kit::PURPLE,
        ) {
            self.update_setting(|s| s.clipboard = v);
        }
        if let Some(v) = tile(
            self,
            ui,
            "🔌",
            "Allow USB connections (adb)",
            s.usb,
            kit::CYAN,
        ) {
            self.update_setting(|s| s.usb = v);
        }
        if let Some(v) = tile(
            self,
            ui,
            "📶",
            "Allow WiFi connections (off = USB only, port closed)",
            s.wifi,
            kit::GREEN,
        ) {
            self.update_setting(|s| s.wifi = v);
        }
        if let Some(v) = tile(
            self,
            ui,
            "⚡",
            "Open the tablet app when the tablet is plugged in (USB). Needs Extendo 0.3+ on the tablet.",
            s.usb_autostart,
            kit::ORANGE,
        ) {
            self.update_setting(|s| s.usb_autostart = v);
        }
        if let Some(v) = tile(
            self,
            ui,
            "🌙",
            "Keep running in the background when the window closes",
            s.run_in_background,
            kit::INDIGO,
        ) {
            self.update_setting(|s| s.run_in_background = v);
        }
        let at_login = crate::background::start_at_login();
        if let Some(v) = tile(
            self,
            ui,
            "🚀",
            "Start Extendo when I log in (in the background)",
            at_login,
            kit::PINK,
        ) {
            match crate::background::set_start_at_login(v) {
                Ok(()) if v => log::info!("Extendo will start when you log in"),
                Ok(()) => log::info!("Extendo won't start when you log in"),
                Err(e) => log::error!("{e:#}"),
            }
        }
        ui.add_space(4.0);
        kit::inset(p, 12).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Port").font(kit::semi(13.0)).color(p.text));
                let mut port = s.port;
                if ui
                    .add(egui::DragValue::new(&mut port).range(1024..=65535))
                    .changed()
                {
                    self.update_setting(|s| s.port = port);
                }
                ui.label(RichText::new("restart to apply").size(12.0).color(p.muted));
            });
        });
        ui.add_space(8.0);
        kit::inset(p, 12).show(ui, |ui| {
            ui.set_width(ui.available_width());
            kit::field_label(ui, p, "Shortcuts in any app");
            self.shortcut_options(ui, &s);
        });
        ui.add_space(12.0);
        kit::note(
            ui,
            p,
            "Open the window again from the tray icon or by starting Extendo.",
        );
        if kit::link_button(ui, p, "Quit Extendo", kit::RED).clicked() {
            crate::background::send(crate::background::Request::Quit);
        }
    }

    /// Keyboard shortcuts that work in any app.
    fn shortcut_options(&mut self, ui: &mut Ui, s: &crate::state::Settings) {
        let p = self.palette();
        #[cfg(windows)]
        {
            let mut on = s.hotkeys;
            if kit::option_tile(
                ui,
                p,
                "⌨",
                "Ctrl+Alt+E switches Extend / Mirror, Ctrl+Alt+X disconnects",
                &mut on,
                kit::PURPLE,
                ui.available_width(),
            )
            .changed()
            {
                self.update_setting(|s| s.hotkeys = on);
                crate::hotkeys::set_enabled(on);
            }
        }
        #[cfg(target_os = "linux")]
        {
            let _ = s;
            match self.gnome_shortcuts {
                Some(added) => {
                    kit::note(
                        ui,
                        p,
                        "Ctrl+Alt+E switches Extend / Mirror, Ctrl+Alt+X disconnects.",
                    );
                    let label = if added {
                        "Remove from GNOME"
                    } else {
                        "Add to GNOME"
                    };
                    if kit::link_button(ui, p, label, kit::PURPLE)
                        .on_hover_text("Settings › Keyboard › Custom Shortcuts")
                        .clicked()
                    {
                        let r = if added {
                            crate::hotkeys::gnome::remove()
                        } else {
                            crate::hotkeys::gnome::install()
                        };
                        match r {
                            Ok(()) => self.gnome_shortcuts = Some(!added),
                            Err(e) => log::error!("keyboard shortcuts: {e:#}"),
                        }
                    }
                }
                None => {
                    kit::note(
                        ui,
                        p,
                        "Bind \"extendo --toggle-mode\" and \"extendo --disconnect\" in your desktop's keyboard settings.",
                    );
                }
            }
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = (s, p);
        }
    }

    fn log_drawer(&mut self, ui: &mut Ui) {
        let p = self.palette();
        kit::inset(p, 14).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(format!("Extendo {}", env!("CARGO_PKG_VERSION")))
                    .font(kit::bold(15.0))
                    .color(p.text),
            );
            ui.label(RichText::new("Developed by Md. Hafijur Rahman").color(p.muted));
            ui.horizontal(|ui| {
                ui.label(RichText::new("GitHub:").color(p.muted));
                ui.hyperlink("https://github.com/Hafij-BGE/Second-monitor");
            });
            ui.label(RichText::new("License: MIT").color(p.muted));
            kit::note(
                ui,
                p,
                "Third-party software: scrcpy (Apache-2.0), OpenH264 (BSD-2-Clause), egui, rustls \
                 and other Rust libraries (MIT/Apache-2.0), Inter typeface (OFL-1.1), Parsec Virtual \
                 Display Driver on Windows. See THIRD_PARTY_NOTICES.md in the GitHub repository.",
            );
            kit::note(ui, p, "This app was generated with AI. Developer support may not be available.");
        });
        ui.add_space(12.0);
        kit::field_label(ui, p, "Activity");
        let lines: Vec<String> = self.shared.log.lock().unwrap().iter().cloned().collect();
        kit::inset(p, 10).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if lines.is_empty() {
                kit::note(ui, p, "Nothing yet.");
            }
            for line in lines.iter().rev() {
                let color = if line.contains("ERROR") {
                    kit::ink(p, kit::RED)
                } else if line.contains("WARN") {
                    kit::ink(p, kit::AMBER)
                } else {
                    p.muted
                };
                ui.add(
                    egui::Label::new(RichText::new(line).font(kit::mono(11.5)).color(color)).wrap(),
                );
            }
        });
    }

    // ------------------------------------------------------------ dialogs

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
        egui::Modal::new(egui::Id::new("pairing"))
            .frame(modal_frame(p))
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(
                    RichText::new("Allow this tablet?")
                        .font(kit::bold(20.0))
                        .color(p.text),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("📱 {}", req.name))
                        .font(kit::semi(16.0))
                        .color(p.text),
                );
                ui.label(RichText::new(format!("from {} over WiFi", req.address)).color(p.muted));
                ui.add_space(10.0);
                kit::inset(p, 14).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("Check the tablet shows this code").color(p.text));
                        ui.label(
                            RichText::new(&status.pairing_code)
                                .font(kit::mono(32.0))
                                .color(kit::ink(p, kit::GREEN)),
                        );
                        ui.label(
                            RichText::new("If it doesn't match, choose Deny.")
                                .size(12.0)
                                .color(p.muted),
                        );
                    });
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if kit::link_button(ui, p, "Deny", kit::RED).clicked() {
                        self.shared.answer_pairing(PairingDecision::Deny);
                    }
                    if kit::link_button(ui, p, "Allow once", kit::PURPLE).clicked() {
                        self.shared.answer_pairing(PairingDecision::AllowOnce);
                    }
                    if kit::gradient_button(ui, "Always allow", kit::SCREEN_GRAD, true).clicked() {
                        self.shared.answer_pairing(PairingDecision::AlwaysAllow);
                    }
                });
            });
    }

    /// First start: what Extendo does and how to connect, in four short steps.
    fn welcome_guide(&mut self, ctx: &egui::Context) {
        let settings = self.shared.settings();
        if settings.welcome_done || self.shared.status().pairing.is_some() {
            return;
        }
        const STEPS: usize = 4;
        let p = self.palette();
        let addresses = self.addresses.clone();
        let mut finish = false;
        let response = egui::Modal::new(egui::Id::new("welcome"))
            .frame(modal_frame(p))
            .show(ctx, |ui| {
                ui.set_width(460.0);
                // Progress dots.
                ui.horizontal(|ui| {
                    for i in 0..STEPS {
                        let w = if i == self.welcome_step { 26.0 } else { 8.0 };
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 8.0), Sense::hover());
                        if i == self.welcome_step {
                            kit::gradient_rect(ui, rect, 4.0, kit::VIOLET, kit::CYAN);
                        } else {
                            ui.painter().rect_filled(rect, CornerRadius::same(4), p.inset_border);
                        }
                    }
                    ui.label(
                        RichText::new(format!("Step {} of {STEPS}", self.welcome_step + 1))
                            .size(12.0)
                            .color(p.muted),
                    );
                });
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.set_min_height(240.0);
                    let title = |ui: &mut Ui, text: &str| {
                        ui.label(RichText::new(text).font(kit::bold(21.0)).color(p.text));
                        ui.add_space(6.0);
                    };
                    match self.welcome_step {
                        0 => {
                            title(ui, "👋  Welcome to Extendo");
                            ui.label(
                                RichText::new("Use an Android tablet as a second screen for this PC, over a USB cable or WiFi.")
                                    .color(p.text),
                            );
                            ui.add_space(8.0);
                            for (tab, what, color) in [
                                ("🖥 Spatial Screen", "extend or mirror this PC onto the tablet", kit::PURPLE),
                                ("📱 Phone Mirror", "see and control a phone here, with sound", kit::PINK),
                                ("📂 Storage & Keys", "open a phone's files like a USB drive; gestures and shortcuts", kit::GREEN),
                            ] {
                                kit::tinted(p, color, 10).show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(RichText::new(tab).font(kit::semi(13.5)).color(kit::ink(p, color)));
                                    ui.label(RichText::new(what).size(12.5).color(p.muted));
                                });
                                ui.add_space(4.0);
                            }
                        }
                        1 => {
                            title(ui, "📲  Install the tablet app");
                            ui.label(
                                RichText::new("The tablet needs the Extendo app (Android 7 or newer).")
                                    .color(p.text),
                            );
                            ui.add_space(6.0);
                            ui.hyperlink_to(RichText::new("Download the Android app").strong(), ANDROID_APP_URL);
                            ui.add_space(6.0);
                            kit::note(ui, p, "Install it on a phone too if you want to open its files from this PC.");
                        }
                        2 => {
                            title(ui, "🔗  Connect");
                            ui.label(RichText::new("🔌 USB cable (lowest delay)").font(kit::semi(14.0)).color(p.text));
                            kit::note(
                                ui,
                                p,
                                "Turn on USB debugging once, plug in, then tap \"Connect over USB\" in the app. After that, the app opens by itself when you plug the tablet in.",
                            );
                            egui::CollapsingHeader::new("How to turn on USB debugging")
                                .id_salt("welcome_usb")
                                .show(ui, |ui| {
                                    for step in [
                                        "1. On the tablet open Settings › About tablet.",
                                        "2. Tap Build number seven times.",
                                        "3. Open Settings › Developer options and turn on USB debugging.",
                                        "4. Plug in the cable and allow this PC.",
                                    ] {
                                        ui.label(RichText::new(step).color(p.text));
                                    }
                                });
                            ui.add_space(6.0);
                            ui.label(RichText::new("📶 WiFi").font(kit::semi(14.0)).color(p.text));
                            kit::note(
                                ui,
                                p,
                                "Same network as this PC: open the app and tap this PC. Allow it here when both screens show the same code.",
                            );
                            if !addresses.is_empty() {
                                let list: Vec<String> = addresses
                                    .iter()
                                    .map(|ip| format!("{ip}:{}", settings.port))
                                    .collect();
                                ui.label(
                                    RichText::new(format!("This PC: {}", list.join("   ")))
                                        .font(kit::mono(12.0))
                                        .color(kit::ink(p, kit::CYAN)),
                                );
                            }
                        }
                        _ => {
                            title(ui, "✅  You're set");
                            ui.label(
                                RichText::new("Everything is in the tabs at the top: Spatial Screen, Phone Mirror, and Storage & Keys. Security, options and the log are the buttons at the top right.")
                                    .color(p.text),
                            );
                            ui.add_space(6.0);
                            if settings.run_in_background {
                                kit::note(
                                    ui,
                                    p,
                                    "Closing the window keeps Extendo running in the tray, so tablets can still connect.",
                                );
                            }
                            kit::note(ui, p, "Each tablet keeps its own resolution, frame rate and quality.");
                        }
                    }
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if kit::link_button(ui, p, "Skip", kit::PURPLE).clicked() {
                        finish = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let last = self.welcome_step + 1 == STEPS;
                        let label = if last { "Start" } else { "Next" };
                        if kit::gradient_button(ui, label, kit::SCREEN_GRAD, true).clicked() {
                            if last {
                                finish = true;
                            } else {
                                self.welcome_step += 1;
                            }
                        }
                        if self.welcome_step > 0 && kit::link_button(ui, p, "Back", kit::PURPLE).clicked() {
                            self.welcome_step -= 1;
                        }
                    });
                });
            });
        if finish || response.should_close() {
            self.welcome_step = 0;
            self.update_setting(|s| s.welcome_done = true);
        }
    }

    /// The QR code for pairing a phone over WiFi, and how it is going.
    fn wifi_pair_dialog(&mut self, ctx: &egui::Context) {
        let Some(pairing) = &self.wifi_pairing else {
            return;
        };
        let p = self.palette();
        let state = self.shared.status().wifi_pair.clone();
        let mut close = false;
        let mut new_code = false;
        let response = egui::Modal::new(egui::Id::new("wifi_pair"))
            .frame(modal_frame(p))
            .show(ctx, |ui| {
                ui.set_width(400.0);
                ui.label(RichText::new("Pair a phone over WiFi").font(kit::bold(20.0)).color(p.text));
                ui.add_space(4.0);
                kit::note(
                    ui,
                    p,
                    "On the phone: Settings › Developer options › Wireless debugging › Pair device with QR code. The phone and this PC must be on the same WiFi.",
                );
                ui.add_space(8.0);
                ui.vertical_centered(|ui| draw_qr(ui, &pairing.qr, 220.0));
                ui.add_space(8.0);
                match &state {
                    Some(WifiPairState::Waiting) | None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Waiting for the phone to scan the code…").color(p.text));
                        });
                    }
                    Some(WifiPairState::Pairing) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Phone found, pairing…").color(p.text));
                        });
                    }
                    Some(WifiPairState::Connected(addr)) => {
                        ui.label(
                            RichText::new(format!(
                                "● Paired and connected ({addr}). Press Mirror next to the phone."
                            ))
                            .color(kit::ink(p, kit::GREEN)),
                        );
                    }
                    Some(WifiPairState::Failed(e)) => {
                        ui.label(RichText::new(e).color(kit::ink(p, kit::RED)));
                    }
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let done = matches!(state, Some(WifiPairState::Connected(_)));
                    if kit::link_button(ui, p, if done { "Done" } else { "Cancel" }, kit::PINK).clicked() {
                        close = true;
                    }
                    if matches!(state, Some(WifiPairState::Failed(_)))
                        && kit::gradient_button(ui, "New code", kit::MIRROR_GRAD, true).clicked()
                    {
                        new_code = true;
                    }
                });
            });
        if new_code {
            self.wifi_pairing = None;
            self.wifi_pairing = wifi_pair::start(self.shared.clone());
        } else if close || response.should_close() {
            self.wifi_pairing = None;
            self.shared.update_status(|s| s.wifi_pair = None);
        }
    }

    // ------------------------------------------------------------ footer

    fn footer(&mut self, ui: &mut Ui) {
        let p = self.palette();
        kit::glass(p, 18, 10).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                let (text, color) = match backend::virtual_display_installed() {
                    None => ("Virtual display · built in".to_string(), kit::PURPLE),
                    Some(true) => ("Virtual Display Driver · ready".to_string(), kit::PURPLE),
                    Some(false) => (
                        "Virtual Display Driver · not installed".to_string(),
                        kit::AMBER,
                    ),
                };
                kit::dot(ui, color, 3.5);
                ui.label(
                    RichText::new(text)
                        .font(kit::mono(11.5))
                        .color(kit::ink(p, color)),
                );
                ui.label(RichText::new("|").color(p.faint));
                ui.label(
                    RichText::new("TLS · PIN pairing")
                        .font(kit::mono(11.5))
                        .color(p.muted),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let resp = ui
                        .add(
                            egui::Label::new(
                                RichText::new("Logs & About")
                                    .font(kit::mono(11.5))
                                    .color(kit::ink(p, kit::CYAN)),
                            )
                            .sense(Sense::click()),
                        )
                        .on_hover_cursor(CursorIcon::PointingHand);
                    if resp.clicked() {
                        self.drawer = Some(Drawer::Log);
                    }
                    ui.label(RichText::new("·").color(p.faint));
                    ui.label(
                        RichText::new(format!(
                            "{} · v{}",
                            session_name(),
                            env!("CARGO_PKG_VERSION")
                        ))
                        .font(kit::mono(11.5))
                        .color(p.muted),
                    );
                });
            });
        });
        ui.add_space(16.0);
    }

    fn toast(&mut self, ctx: &egui::Context) {
        let Some((text, at)) = &self.toast else {
            return;
        };
        if at.elapsed() > Duration::from_millis(1600) {
            self.toast = None;
            return;
        }
        let p = self.palette();
        let text = text.clone();
        egui::Area::new(egui::Id::new("toast"))
            .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -24.0))
            .order(egui::Order::Tooltip)
            .interactable(false)
            .show(ctx, |ui| {
                Frame::default()
                    .fill(if p.dark {
                        Color32::from_rgb(0xe2, 0xe8, 0xf0)
                    } else {
                        Color32::from_rgb(0x0f, 0x17, 0x2a)
                    })
                    .corner_radius(CornerRadius::same(255))
                    .inner_margin(Margin::symmetric(16, 8))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("✔  {text}"))
                                .font(kit::semi(13.0))
                                .color(p.bg),
                        );
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(200));
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

impl Drop for App {
    fn drop(&mut self) {
        crate::background::set_window(None);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        for request in crate::background::take() {
            match request {
                crate::background::Request::Show => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                crate::background::Request::Quit => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close)
                }
            }
        }
        self.apply_theme(&ctx);
        self.handle_dropped_files(&ctx);
        let p = self.palette();
        egui::CentralPanel::default()
            .frame(Frame::default().fill(p.bg))
            .show(ui, |ui| {
                kit::backdrop(ui.painter(), ui.max_rect(), p);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let full = ui.available_width();
                        let width = (full - 32.0).min(1040.0);
                        let pad = ((full - width) / 2.0).max(0.0);
                        ui.add_space(16.0);
                        ui.horizontal_top(|ui| {
                            ui.add_space(pad);
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                self.header(ui);
                                match self.tab {
                                    Tab::Screen => {
                                        self.ribbon(ui);
                                        self.stage_card(ui);
                                        self.pods(ui);
                                    }
                                    Tab::Mirror => {
                                        self.mirror_stage_card(ui);
                                        self.mirror_options_card(ui);
                                    }
                                    Tab::Storage => {
                                        self.storage_card(ui);
                                        self.shortcuts_card(ui);
                                    }
                                }
                                self.footer(ui);
                            });
                        });
                    });
            });
        self.drawer(&ctx);
        self.welcome_guide(&ctx);
        self.wifi_pair_dialog(&ctx);
        self.pairing_dialog(&ctx);
        self.toast(&ctx);
        // Refresh stats even when no other event arrives.
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}

// ---------------------------------------------------------------- pictures

fn solid_fill(p: &Palette) -> Color32 {
    if p.dark {
        Color32::from_rgb(0x10, 0x14, 0x22)
    } else {
        Color32::WHITE
    }
}

fn modal_frame(p: &Palette) -> Frame {
    kit::glass(p, 24, 22).fill(solid_fill(p))
}

fn stat(ui: &mut Ui, p: &Palette, value: &str, label: &str, color: Color32) {
    Frame::default()
        .fill(kit::tint(p, color))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(14, 6))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(value).font(kit::bold(17.0)).color(p.text));
                ui.label(RichText::new(label).size(11.0).color(kit::ink(p, color)));
            });
        });
}

struct StageInfo {
    mode: Mode,
    scale: f32,
    fps: u32,
    link: String,
    tablet: Option<String>,
    monitor: String,
    host: String,
    time: f32,
}

/// The tablet and this PC as two screens joined by a data line.
fn paint_stage(ui: &mut Ui, p: &Palette, info: &StageInfo, live: bool) {
    let width = ui.available_width();
    // Side by side (base 670 × 210), or stacked on narrow windows (300 × 440).
    let stacked = width < 600.0;
    let (base_w, base_h) = if stacked {
        (300.0, 440.0)
    } else {
        (670.0, 210.0)
    };
    let (s, height) = if stacked {
        let s = ((width - 32.0) / base_w).min(1.15);
        (s, base_h * s + 30.0)
    } else {
        let height = (width * 0.34).clamp(200.0, 260.0);
        (
            ((width - 32.0) / base_w)
                .min((height - 20.0) / base_h)
                .min(1.15),
            height,
        )
    };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    let font = |size: f32| FontId::proportional((size * s).max(8.0));
    let mono = |size: f32| FontId::monospace((size * s).max(8.0));
    let semi = |size: f32| kit::semi((size * s).max(8.0));
    let origin = rect.center() - Vec2::new(base_w / 2.0, base_h / 2.0) * s;
    let at = |x: f32, y: f32| origin + Vec2::new(x, y) * s;
    let teal = Color32::from_rgb(0x2d, 0xd4, 0xbf);

    // Tablet.
    let tablet_at = if stacked {
        at(35.0, 0.0)
    } else {
        at(0.0, 18.0)
    };
    let tablet = Rect::from_min_size(tablet_at, Vec2::new(230.0, 156.0) * s);
    kit::glow(
        &painter,
        tablet.center(),
        170.0 * s,
        kit::alpha(teal, if p.dark { 0.16 } else { 0.12 }),
    );
    painter.rect(
        tablet,
        CornerRadius::same((18.0 * s) as u8),
        Color32::from_rgb(0x0b, 0x12, 0x20),
        Stroke::new(2.0, kit::alpha(teal, 0.75)),
        egui::StrokeKind::Inside,
    );
    let screen = tablet.shrink(9.0 * s);
    let mirror = info.mode == Mode::Mirror;
    let (a, b) = if mirror {
        (
            Color32::from_rgb(0x2e, 0x10, 0x65),
            Color32::from_rgb(0x1e, 0x3a, 0x8a),
        )
    } else {
        (
            Color32::from_rgb(0x0e, 0x3b, 0x3b),
            Color32::from_rgb(0x0b, 0x24, 0x33),
        )
    };
    gradient_in(ui, &painter, screen, 11.0 * s, a, b);
    painter.circle_filled(
        tablet.center_top() + Vec2::new(0.0, 4.5 * s),
        1.6 * s,
        Color32::from_gray(70),
    );
    let label = if mirror {
        "Same as PC · Mirror"
    } else {
        "Display 2 · Extended"
    };
    let lbl = painter.layout_no_wrap(label.into(), mono(9.5), teal);
    let lr = Rect::from_min_size(
        screen.min + Vec2::new(9.0, 9.0) * s,
        lbl.size() + Vec2::new(14.0, 6.0) * s,
    );
    painter.rect(
        lr,
        CornerRadius::same(255),
        kit::alpha(teal, 0.12),
        Stroke::new(1.0, kit::alpha(teal, 0.6)),
        egui::StrokeKind::Inside,
    );
    painter.galley(lr.center() - lbl.size() / 2.0, lbl, teal);
    let pct = format!("{:.0}%", info.scale * 100.0);
    let pg = painter.layout_no_wrap(pct, mono(9.5), teal);
    let pr = Rect::from_min_size(
        Pos2::new(
            screen.right() - 9.0 * s - pg.size().x - 12.0 * s,
            screen.top() + 9.0 * s,
        ),
        pg.size() + Vec2::new(12.0, 6.0) * s,
    );
    painter.rect_filled(pr, CornerRadius::same(5), Color32::from_black_alpha(90));
    painter.galley(pr.center() - pg.size() / 2.0, pg, teal);
    let card = Rect::from_min_size(
        Pos2::new(screen.left() + 9.0 * s, screen.center().y - 18.0 * s),
        Vec2::new(screen.width() - 18.0 * s, 40.0 * s),
    );
    painter.rect(
        card,
        CornerRadius::same((8.0 * s) as u8),
        Color32::from_black_alpha(70),
        Stroke::new(1.0, Color32::from_white_alpha(25)),
        egui::StrokeKind::Inside,
    );
    let (t1, t2) = if mirror {
        ("Mirrored Screen", "Shows the same picture as this PC")
    } else {
        ("Extended Workspace", "Drag windows past the monitor edge")
    };
    painter.text(
        card.left_top() + Vec2::new(9.0, 8.0) * s,
        Align2::LEFT_TOP,
        t1,
        semi(11.0),
        Color32::WHITE,
    );
    painter.text(
        card.left_top() + Vec2::new(9.0, 24.0) * s,
        Align2::LEFT_TOP,
        t2,
        font(9.0),
        Color32::from_gray(170),
    );
    let bottom = screen.bottom() - 13.0 * s;
    let state = match &info.tablet {
        Some(_) => "● Live",
        None => "Standby · Ready",
    };
    painter.text(
        Pos2::new(screen.left() + 10.0 * s, bottom),
        Align2::LEFT_CENTER,
        state,
        font(9.5),
        if info.tablet.is_some() {
            Color32::from_rgb(0x4a, 0xde, 0x80)
        } else {
            Color32::from_gray(170)
        },
    );
    painter.text(
        Pos2::new(screen.right() - 10.0 * s, bottom),
        Align2::RIGHT_CENTER,
        format!("{} FPS", info.fps),
        mono(9.5),
        Color32::from_gray(200),
    );
    let caption = info
        .tablet
        .clone()
        .unwrap_or_else(|| "Extendo Tablet Display".into());
    painter.text(
        tablet.center_bottom() + Vec2::new(0.0, 15.0 * s),
        Align2::CENTER_CENTER,
        caption,
        mono(10.5),
        kit::ink(p, teal),
    );

    // This PC.
    let pc_at = if stacked {
        at(10.0, 240.0)
    } else {
        at(390.0, 8.0)
    };
    let pc = Rect::from_min_size(pc_at, Vec2::new(280.0, 160.0) * s);
    kit::glow(
        &painter,
        pc.center(),
        170.0 * s,
        kit::alpha(kit::PURPLE, if p.dark { 0.14 } else { 0.10 }),
    );
    let neck = Rect::from_center_size(
        pc.center_bottom() + Vec2::new(0.0, 8.0 * s),
        Vec2::new(30.0, 16.0) * s,
    );
    painter.rect_filled(
        neck,
        CornerRadius::ZERO,
        Color32::from_rgb(0x47, 0x55, 0x69),
    );
    let foot = Rect::from_center_size(
        pc.center_bottom() + Vec2::new(0.0, 17.0 * s),
        Vec2::new(90.0, 5.0) * s,
    );
    painter.rect_filled(
        foot,
        CornerRadius::same(3),
        Color32::from_rgb(0x64, 0x74, 0x8b),
    );
    painter.rect(
        pc,
        CornerRadius::same((12.0 * s) as u8),
        Color32::from_rgb(0x0b, 0x0f, 0x1a),
        Stroke::new(1.5, Color32::from_rgb(0x33, 0x41, 0x55)),
        egui::StrokeKind::Inside,
    );
    let ps = pc.shrink(8.0 * s);
    gradient_in(
        ui,
        &painter,
        ps,
        6.0 * s,
        Color32::from_rgb(0x2e, 0x10, 0x65),
        Color32::from_rgb(0x1e, 0x3a, 0x8a),
    );
    let bar = Rect::from_min_size(ps.min, Vec2::new(ps.width(), 14.0 * s));
    painter.rect_filled(
        bar,
        CornerRadius::same((6.0 * s) as u8),
        Color32::from_black_alpha(110),
    );
    if cfg!(target_os = "linux") {
        painter.text(
            bar.left_center() + Vec2::new(8.0 * s, 0.0),
            Align2::LEFT_CENTER,
            "Activities",
            font(8.5),
            Color32::from_gray(225),
        );
    }
    painter.text(
        bar.center(),
        Align2::CENTER_CENTER,
        format!("Host · {}", info.host),
        font(8.5),
        Color32::from_gray(225),
    );
    let win = Rect::from_min_size(
        ps.min + Vec2::new(14.0, 30.0) * s,
        Vec2::new(96.0, 62.0) * s,
    );
    painter.rect(
        win,
        CornerRadius::same((6.0 * s) as u8),
        Color32::from_rgb(0x0f, 0x17, 0x2a),
        Stroke::new(1.0, Color32::from_white_alpha(30)),
        egui::StrokeKind::Inside,
    );
    for (i, c) in [kit::RED, kit::AMBER, kit::GREEN].into_iter().enumerate() {
        painter.circle_filled(
            win.min + Vec2::new(9.0 + i as f32 * 8.0, 9.0) * s,
            2.4 * s,
            c,
        );
    }
    for (i, (w, c)) in [
        (70.0, kit::PURPLE),
        (52.0, kit::CYAN),
        (62.0, Color32::from_gray(90)),
    ]
    .into_iter()
    .enumerate()
    {
        let r = Rect::from_min_size(
            win.min + Vec2::new(9.0, 20.0 + i as f32 * 11.0) * s,
            Vec2::new(w, 5.0) * s,
        );
        painter.rect_filled(r, CornerRadius::same(3), c);
    }
    let primary = if mirror {
        "Shared with tablet"
    } else {
        "Primary Monitor"
    };
    let pgal = painter.layout_no_wrap(primary.into(), mono(9.0), Color32::WHITE);
    let pr = Rect::from_min_size(
        Pos2::new(
            ps.right() - 12.0 * s - pgal.size().x - 14.0 * s,
            ps.top() + 50.0 * s,
        ),
        pgal.size() + Vec2::new(14.0, 8.0) * s,
    );
    painter.rect(
        pr,
        CornerRadius::same(4),
        kit::alpha(kit::PURPLE, 0.45),
        Stroke::new(1.0, Color32::from_white_alpha(70)),
        egui::StrokeKind::Inside,
    );
    painter.galley(pr.center() - pgal.size() / 2.0, pgal, Color32::WHITE);
    let mut mon = info.monitor.clone();
    if mon.chars().count() > 28 {
        mon = mon.chars().take(27).collect::<String>() + "…";
    }
    painter.text(
        pr.center_bottom() + Vec2::new(0.0, 11.0 * s),
        Align2::CENTER_CENTER,
        mon,
        mono(8.5),
        Color32::from_gray(200),
    );
    let dock = Rect::from_center_size(
        ps.center_bottom() - Vec2::new(0.0, 12.0 * s),
        Vec2::new(64.0, 13.0) * s,
    );
    painter.rect_filled(dock, CornerRadius::same(5), Color32::from_black_alpha(120));
    for (i, c) in [kit::PURPLE, kit::CYAN, kit::PINK, kit::GREEN]
        .into_iter()
        .enumerate()
    {
        let r = Rect::from_center_size(
            dock.left_center() + Vec2::new(11.0 + i as f32 * 14.0, 0.0) * s,
            Vec2::splat(7.0 * s),
        );
        painter.rect_filled(r, CornerRadius::same(2), c);
    }
    let os = if cfg!(windows) {
        "Windows PC"
    } else {
        "Linux PC"
    };
    painter.text(
        pc.center_bottom() + Vec2::new(0.0, 32.0 * s),
        Align2::CENTER_CENTER,
        format!("{os} · Host ({})", info.host),
        mono(10.5),
        p.muted,
    );

    // The link between them.
    let (from, to) = if stacked {
        (
            tablet.center_bottom() + Vec2::new(0.0, 26.0 * s),
            pc.center_top() - Vec2::new(0.0, 2.0),
        )
    } else {
        (
            tablet.right_center() + Vec2::new(2.0, 0.0),
            pc.left_center() - Vec2::new(2.0, 0.0),
        )
    };
    painter.line_segment(
        [from, to],
        Stroke::new(6.0 * s, kit::alpha(kit::CYAN, 0.12)),
    );
    let offset = if live {
        -(info.time * 40.0) % 16.0
    } else {
        0.0
    };
    painter.extend(egui::Shape::dashed_line_with_offset(
        &[from, to],
        Stroke::new(2.0, kit::alpha(kit::CYAN, if live { 0.95 } else { 0.55 })),
        &[8.0],
        &[8.0],
        -offset,
    ));
    let lg = painter.layout_no_wrap(info.link.clone(), mono(9.5), kit::ink(p, kit::CYAN));
    let mid = Pos2::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0);
    let br = Rect::from_center_size(mid, lg.size() + Vec2::new(18.0, 10.0) * s);
    painter.rect(
        br,
        CornerRadius::same(255),
        solid_fill(p),
        Stroke::new(1.0, kit::alpha(kit::CYAN, 0.5)),
        egui::StrokeKind::Inside,
    );
    painter.galley(br.center() - lg.size() / 2.0, lg, kit::ink(p, kit::CYAN));
}

/// `kit::gradient_rect` on a clipped painter.
fn gradient_in(ui: &Ui, painter: &egui::Painter, rect: Rect, radius: f32, a: Color32, b: Color32) {
    let _ = painter;
    kit::gradient_rect(ui, rect, radius, a, b);
}

enum PhoneState {
    NoEngine,
    NoPhone,
    Ready(String),
    Mirroring(String),
}

struct PhoneInfo {
    state: PhoneState,
    m: crate::state::MirrorSettings,
}

/// A phone that previews the mirror settings.
fn paint_phone(ui: &mut Ui, p: &Palette, info: &PhoneInfo, scale: f32) {
    let s = scale;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(200.0, 410.0) * s, Sense::hover());
    let painter = ui.painter_at(rect.expand(30.0));
    let body = Rect::from_min_size(rect.min, Vec2::new(200.0, 390.0) * s);
    kit::glow(
        &painter,
        body.center(),
        190.0 * s,
        kit::alpha(kit::PINK, if p.dark { 0.16 } else { 0.10 }),
    );
    painter.rect_filled(
        Rect::from_min_size(
            body.right_top() + Vec2::new(0.0, 70.0 * s),
            Vec2::new(3.0, 34.0) * s,
        ),
        CornerRadius::same(2),
        Color32::from_rgb(0x33, 0x41, 0x55),
    );
    painter.rect(
        body,
        CornerRadius::same((30.0 * s) as u8),
        Color32::from_rgb(0x11, 0x18, 0x27),
        Stroke::new(2.0, kit::alpha(kit::PINK, 0.55)),
        egui::StrokeKind::Inside,
    );
    let screen = body.shrink(8.0 * s);
    let mirroring = matches!(info.state, PhoneState::Mirroring(_));
    let dimmed = mirroring && info.m.screen_off;
    kit::gradient_rect(
        ui,
        screen,
        24.0 * s,
        Color32::from_rgb(0x1e, 0x1b, 0x4b),
        Color32::from_rgb(0x3b, 0x07, 0x64),
    );
    let font = |size: f32| FontId::proportional(size * s);
    painter.circle_filled(
        screen.center_top() + Vec2::new(0.0, 12.0 * s),
        3.5 * s,
        Color32::BLACK,
    );
    let status_y = screen.top() + 12.0 * s;
    painter.text(
        Pos2::new(screen.left() + 14.0 * s, status_y),
        Align2::LEFT_CENTER,
        "Extendo",
        FontId::monospace(9.0 * s),
        Color32::from_gray(210),
    );
    if info.m.audio {
        painter.text(
            Pos2::new(screen.right() - 14.0 * s, status_y),
            Align2::RIGHT_CENTER,
            "♪",
            font(11.0),
            kit::ink(p, kit::CYAN),
        );
    }
    // Badges for the options that change what you see.
    let mut badges: Vec<(&str, Color32)> = Vec::new();
    if info.m.record {
        badges.push(("● REC", kit::RED));
    }
    if info.m.always_on_top {
        badges.push(("📌 On top", kit::ORANGE));
    }
    if info.m.camera {
        badges.push(("📷 Camera", kit::CYAN));
    }
    if info.m.fullscreen {
        badges.push(("⛶ Fullscreen", kit::PURPLE));
    }
    let mut x = screen.left() + 12.0 * s;
    for (text, c) in badges {
        let g = painter.layout_no_wrap(text.into(), font(8.5), Color32::WHITE);
        let r = Rect::from_min_size(
            Pos2::new(x, screen.top() + 26.0 * s),
            g.size() + Vec2::new(10.0, 4.0) * s,
        );
        if r.right() > screen.right() - 8.0 {
            break;
        }
        painter.rect_filled(r, CornerRadius::same(255), kit::alpha(c, 0.85));
        painter.galley(r.center() - g.size() / 2.0, g, Color32::WHITE);
        x = r.right() + 4.0 * s;
    }
    let c = screen.center() - Vec2::new(0.0, 20.0 * s);
    let icon = Rect::from_center_size(c - Vec2::new(0.0, 36.0 * s), Vec2::splat(44.0 * s));
    painter.rect(
        icon,
        CornerRadius::same((12.0 * s) as u8),
        kit::alpha(kit::PINK, 0.25),
        Stroke::new(1.0, kit::alpha(kit::PINK, 0.6)),
        egui::StrokeKind::Inside,
    );
    let glyph = match info.state {
        PhoneState::NoEngine => "⚠",
        PhoneState::NoPhone => "🔌",
        PhoneState::Ready(_) => "📱",
        PhoneState::Mirroring(_) => "▶",
    };
    painter.text(
        icon.center(),
        Align2::CENTER_CENTER,
        glyph,
        font(20.0),
        Color32::WHITE,
    );
    let (title, sub) = match &info.state {
        PhoneState::NoEngine => (
            "Engine standby".to_string(),
            "Reinstall Extendo to mirror phones".to_string(),
        ),
        PhoneState::NoPhone => (
            "No phone yet".to_string(),
            "Plug in with USB debugging, or pair over WiFi".to_string(),
        ),
        PhoneState::Ready(m) => (m.clone(), "Ready: press Mirror".to_string()),
        PhoneState::Mirroring(m) => (m.clone(), "● Mirroring now".to_string()),
    };
    let tg = painter.layout(
        title,
        kit::semi(14.0 * s),
        Color32::WHITE,
        screen.width() - 30.0 * s,
    );
    painter.galley(
        Pos2::new(c.x - tg.size().x / 2.0, c.y + 2.0 * s),
        tg,
        Color32::WHITE,
    );
    let sg = painter.layout(
        sub,
        font(10.5),
        Color32::from_gray(200),
        screen.width() - 36.0 * s,
    );
    painter.galley(
        Pos2::new(c.x - sg.size().x / 2.0, c.y + 26.0 * s),
        sg,
        Color32::from_gray(200),
    );
    let note = if info.m.screen_off {
        "🌙 Phone screen turns off while mirroring"
    } else if info.m.show_touches {
        "👆 Touches are shown on the phone"
    } else if info.m.stay_awake {
        "☕ Stays awake while plugged in"
    } else {
        ""
    };
    if !note.is_empty() {
        let ng = painter.layout(
            note.into(),
            font(9.0),
            Color32::from_gray(180),
            screen.width() - 30.0 * s,
        );
        painter.galley(
            Pos2::new(c.x - ng.size().x / 2.0, screen.bottom() - 70.0 * s),
            ng,
            Color32::from_gray(180),
        );
    }
    if dimmed {
        painter.rect_filled(
            screen,
            CornerRadius::same((24.0 * s) as u8),
            Color32::from_black_alpha(150),
        );
        painter.text(
            screen.center(),
            Align2::CENTER_CENTER,
            "🌙 Screen off",
            font(12.0),
            Color32::from_gray(220),
        );
    }
    if info.m.navbar {
        let nav = Rect::from_min_max(
            Pos2::new(screen.left(), screen.bottom() - 30.0 * s),
            screen.max,
        );
        painter.rect_filled(
            nav,
            CornerRadius {
                nw: 0,
                ne: 0,
                sw: (24.0 * s) as u8,
                se: (24.0 * s) as u8,
            },
            Color32::from_black_alpha(120),
        );
        for (i, g) in ["◁", "○", "□"].into_iter().enumerate() {
            painter.text(
                Pos2::new(
                    nav.left() + nav.width() * (0.25 + 0.25 * i as f32),
                    nav.center().y,
                ),
                Align2::CENTER_CENTER,
                g,
                font(12.0),
                Color32::from_gray(220),
            );
        }
    }
    painter.text(
        body.center_bottom() + Vec2::new(0.0, 12.0 * s),
        Align2::CENTER_CENTER,
        "Live preview of your settings",
        FontId::monospace(10.0 * s),
        p.muted,
    );
}

/// "GNOME · Wayland", "Windows", …
fn session_name() -> String {
    if cfg!(windows) {
        return "Windows".into();
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .and_then(|d| d.split(':').next_back().map(str::to_string))
        .filter(|d| !d.is_empty());
    let session = std::env::var("XDG_SESSION_TYPE").ok().map(|t| {
        let mut c = t.chars();
        c.next()
            .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
            .unwrap_or_default()
    });
    match (desktop, session) {
        (Some(d), Some(s)) => format!("{d} · {s}"),
        (Some(d), None) => d,
        (None, Some(s)) => s,
        (None, None) => "Linux".into(),
    }
}

/// A QR code on a white square with the quiet zone scanners need.
fn draw_qr(ui: &mut Ui, qr: &wifi_pair::QrImage, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(6), Color32::WHITE);
    let quiet = 4.0;
    let cell = size / (qr.width as f32 + 2.0 * quiet);
    for (i, dark) in qr.dark.iter().enumerate() {
        if *dark {
            let (x, y) = ((i % qr.width) as f32, (i / qr.width) as f32);
            let min = rect.min + Vec2::new((x + quiet) * cell, (y + quiet) * cell);
            painter.rect_filled(
                egui::Rect::from_min_size(min, Vec2::splat(cell + 0.3)),
                CornerRadius::ZERO,
                Color32::BLACK,
            );
        }
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
