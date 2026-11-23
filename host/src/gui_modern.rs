//! Modern look: floating header islands, one stage per mode, a control
//! ribbon and a mode dock at the bottom. Same features as the Studio look;
//! Options › Window look switches between them.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MTab {
    Screen,
    Mirror,
    Storage,
    Keys,
}

impl MTab {
    /// SMON_TAB=mirror|storage|keys opens that mode (for screenshots).
    pub(super) fn from_env() -> Self {
        match std::env::var("SMON_TAB").as_deref() {
            Ok("mirror") => MTab::Mirror,
            Ok("storage") => MTab::Storage,
            Ok("keys") | Ok("shortcuts") => MTab::Keys,
            _ => MTab::Screen,
        }
    }

    fn grad(self) -> (Color32, Color32) {
        match self {
            MTab::Screen => kit::M_SCREEN_GRAD,
            MTab::Mirror => kit::M_MIRROR_GRAD,
            MTab::Storage => kit::M_STORAGE_GRAD,
            MTab::Keys => kit::M_KEYS_GRAD,
        }
    }

    /// Soft glows behind the stage.
    fn aurora(self) -> (Color32, Color32) {
        let c = Color32::from_rgb;
        match self {
            MTab::Screen => (c(0x93, 0xc5, 0xfd), c(0xc4, 0xb5, 0xfd)),
            MTab::Mirror => (c(0xfd, 0xa4, 0xaf), c(0xf0, 0xab, 0xfc)),
            MTab::Storage => (c(0xfc, 0xd3, 0x4d), c(0xfd, 0xba, 0x74)),
            MTab::Keys => (c(0x6e, 0xe7, 0xb7), c(0x5e, 0xea, 0xd4)),
        }
    }
}

// Device "hardware" (laptop, tablet) stays dark on both themes.
const HW_BODY: Color32 = Color32::from_rgb(0x0f, 0x17, 0x2a);
const HW_INNER: Color32 = Color32::from_rgb(0x16, 0x20, 0x36);
const HW_DEEP: Color32 = Color32::from_rgb(0x0b, 0x12, 0x20);
const HW_TEXT: Color32 = Color32::from_rgb(0xe2, 0xe8, 0xf0);
const HW_MUTED: Color32 = Color32::from_rgb(0x94, 0xa3, 0xb8);
const HW_ACCENT: Color32 = Color32::from_rgb(0x93, 0xc5, 0xfd);

const DOCK_H: f32 = 44.0;

/// Draws one stage column.
type Column<'a> = dyn FnMut(&mut App, &mut Ui) + 'a;

fn hw_frame(margin: i8) -> Frame {
    Frame::default()
        .fill(HW_BODY)
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(22)))
        .corner_radius(CornerRadius::same(22))
        .inner_margin(Margin::same(margin))
        .shadow(egui::Shadow {
            offset: [0, 18],
            blur: 40,
            spread: 0,
            color: Color32::from_black_alpha(70),
        })
}

fn mem(ctx: &egui::Context, key: &str, default: f32) -> f32 {
    ctx.data(|d| d.get_temp(egui::Id::new(key)))
        .unwrap_or(default)
}

fn remember(ctx: &egui::Context, key: &str, value: f32) {
    let old = mem(ctx, key, -1.0);
    if (old - value).abs() > 0.5 {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(key), value));
        ctx.request_repaint();
    }
}

fn text_w(ui: &Ui, text: &str, font: &FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE)
        .size()
        .x
}

/// A small gradient pill button.
fn grad_pill(ui: &mut Ui, text: &str, grad: (Color32, Color32), enabled: bool) -> egui::Response {
    let font = kit::semi(12.0);
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(text_w(ui, text, &font) + 26.0, 28.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let (a, b) = if !enabled {
        (kit::alpha(grad.0, 0.4), kit::alpha(grad.1, 0.4))
    } else if resp.hovered() {
        (
            kit::lerp(grad.0, Color32::WHITE, 0.1),
            kit::lerp(grad.1, Color32::WHITE, 0.1),
        )
    } else {
        grad
    };
    if enabled {
        ui.painter().rect_filled(
            rect.translate(Vec2::new(0.0, 2.0)),
            CornerRadius::same(255),
            kit::alpha(grad.0, 0.22),
        );
    }
    kit::gradient_rect(ui, rect, 14.0, a, b);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        font,
        Color32::WHITE,
    );
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// A full-width gradient button.
fn wide_button(
    ui: &mut Ui,
    text: &str,
    grad: (Color32, Color32),
    enabled: bool,
    width: f32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(width, 40.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let (a, b) = if !enabled {
        (kit::alpha(grad.0, 0.35), kit::alpha(grad.1, 0.35))
    } else if resp.hovered() {
        (
            kit::lerp(grad.0, Color32::WHITE, 0.1),
            kit::lerp(grad.1, Color32::WHITE, 0.1),
        )
    } else {
        grad
    };
    kit::gradient_rect(ui, rect, 20.0, a, b);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        kit::semi(13.5),
        Color32::WHITE,
    );
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// A quiet button on the dark hardware cards.
fn hw_button(ui: &mut Ui, text: &str) -> egui::Response {
    let font = kit::semi(12.0);
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(text_w(ui, text, &font) + 24.0, 28.0),
        Sense::click(),
    );
    ui.painter().rect(
        rect,
        CornerRadius::same(9),
        Color32::from_white_alpha(if resp.hovered() { 34 } else { 18 }),
        Stroke::new(1.0, Color32::from_white_alpha(26)),
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, text, font, HW_TEXT);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

fn hw_chip(ui: &mut Ui, text: &str, color: Color32) {
    Frame::default()
        .fill(Color32::from_white_alpha(16))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).font(kit::mono(10.5)).color(color));
        });
}

/// A coloured status chip on the dark hardware cards.
fn hw_pill(ui: &mut Ui, text: &str, color: Color32) {
    Frame::default()
        .fill(kit::alpha(color, 0.16))
        .stroke(Stroke::new(1.0, kit::alpha(color, 0.4)))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.label(RichText::new(text).font(kit::semi(11.0)).color(kit::lerp(
                color,
                Color32::WHITE,
                0.3,
            )));
        });
}

/// Status badge with a dot ("Waiting for device").
fn status_badge(ui: &mut Ui, p: &Palette, text: &str, color: Color32, pulse: bool) {
    Frame::default()
        .fill(kit::tint(p, color))
        .stroke(Stroke::new(1.0, kit::alpha(color, 0.3)))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                if pulse {
                    let t = (ui.input(|i| i.time) as f32 * 1.6).fract();
                    ui.painter().circle_filled(
                        rect.center(),
                        3.5 + 4.0 * t,
                        kit::alpha(color, 0.45 * (1.0 - t)),
                    );
                    ui.ctx().request_repaint_after(Duration::from_millis(50));
                }
                ui.painter().circle_filled(rect.center(), 3.5, color);
                ui.label(
                    RichText::new(text)
                        .font(kit::semi(11.5))
                        .color(kit::ink(p, color)),
                );
            });
        });
}

/// Starts a new ribbon row when `width` no longer fits in this one.
fn fit(ui: &mut Ui, width: f32) {
    if ui.available_rect_before_wrap().width() < width {
        ui.end_row();
    }
}

/// Ribbon field label.
fn rlabel(ui: &mut Ui, p: &Palette, text: &str) {
    ui.label(RichText::new(text).font(kit::semi(12.0)).color(p.muted));
}

/// The animated data line between two devices.
fn fiber(ui: &mut Ui, size: Vec2, grad: (Color32, Color32), live: bool, vertical: bool) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let (p0, p1, p2, p3) = if vertical {
        let a = rect.center_top();
        let d = rect.center_bottom();
        (
            a,
            a + Vec2::new(rect.width() * 0.45, rect.height() * 0.35),
            d - Vec2::new(rect.width() * 0.45, rect.height() * 0.35),
            d,
        )
    } else {
        let a = rect.left_center();
        let d = rect.right_center();
        (
            a,
            a + Vec2::new(rect.width() * 0.35, -rect.height() * 0.45),
            d + Vec2::new(-rect.width() * 0.35, rect.height() * 0.45),
            d,
        )
    };
    let pts: Vec<Pos2> = (0..=48)
        .map(|i| {
            let u = i as f32 / 48.0;
            let v = 1.0 - u;
            let k = |a: Pos2, b: Pos2, c: Pos2, d: Pos2| {
                a.to_vec2() * v * v * v
                    + b.to_vec2() * 3.0 * v * v * u
                    + c.to_vec2() * 3.0 * v * u * u
                    + d.to_vec2() * u * u * u
            };
            k(p0, p1, p2, p3).to_pos2()
        })
        .collect();
    let painter = ui.painter();
    painter.line(pts.clone(), Stroke::new(7.0, kit::alpha(grad.0, 0.14)));
    let offset = if live {
        ui.ctx().request_repaint_after(Duration::from_millis(33));
        (ui.input(|i| i.time) as f32 * 30.0) % 16.0
    } else {
        0.0
    };
    let half = pts.len() / 2;
    for (part, color) in [(&pts[..=half], grad.0), (&pts[half..], grad.1)] {
        painter.extend(egui::Shape::dashed_line_with_offset(
            part,
            Stroke::new(2.6, color),
            &[10.0],
            &[6.0],
            16.0 - offset,
        ));
    }
}

impl App {
    pub(super) fn modern_ui(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let (ga, gb) = self.mtab.aurora();
        let ctx = ui.ctx().clone();
        let screen = ctx.content_rect();
        egui::CentralPanel::default()
            .frame(Frame::default().fill(p.bg).inner_margin(Margin::same(14)))
            .show(ui, |ui| {
                kit::modern_backdrop(ui.painter(), screen, p, ga, gb);
                self.m_header(ui);
                ui.add_space(12.0);
                let avail = ui.available_rect_before_wrap();
                let ribbon_h = mem(&ctx, "m_ribbon_h", 58.0);
                let dock_w = mem(&ctx, "m_dock_w", 420.0);
                let rib_w = avail.width().min(1180.0);
                let dock = Rect::from_center_size(
                    Pos2::new(avail.center().x, avail.bottom() - DOCK_H / 2.0),
                    Vec2::new(dock_w.min(avail.width()), DOCK_H),
                );
                let ribbon = Rect::from_min_size(
                    Pos2::new(avail.center().x - rib_w / 2.0, dock.top() - 10.0 - ribbon_h),
                    Vec2::new(rib_w, ribbon_h),
                );
                let stage =
                    Rect::from_min_max(avail.min, Pos2::new(avail.right(), ribbon.top() - 12.0));
                let region = |ui: &mut Ui, r: Rect, add: &mut dyn FnMut(&mut Ui)| {
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(r)
                            .layout(Layout::top_down(Align::Min)),
                        |ui| add(ui),
                    )
                };
                region(ui, stage, &mut |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("m_stage_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.m_stage(ui, stage.height()));
                });
                let r = region(ui, ribbon, &mut |ui| self.m_ribbon(ui));
                remember(&ctx, "m_ribbon_h", r.response.rect.height());
                let r = region(ui, dock, &mut |ui| self.m_dock(ui));
                remember(&ctx, "m_dock_w", r.response.rect.width());
            });
        self.m_popover(&ctx);
    }

    // ------------------------------------------------------------ header

    fn m_header(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let actions_w = mem(&ctx, "m_actions_w", 560.0);
        let brand_w = mem(&ctx, "m_brand_w", 470.0);
        let full = ui.available_width();
        if full >= brand_w + actions_w + 12.0 {
            ui.horizontal(|ui| {
                let r = ui.scope(|ui| self.m_brand(ui));
                remember(&ctx, "m_brand_w", r.response.rect.width());
                ui.add_space((ui.available_width() - actions_w).max(0.0));
                let r = ui.scope(|ui| self.m_actions(ui));
                remember(&ctx, "m_actions_w", r.response.rect.width());
            });
        } else {
            let r = ui.scope(|ui| self.m_brand(ui));
            remember(&ctx, "m_brand_w", r.response.rect.width());
            ui.add_space(8.0);
            let r = ui.scope(|ui| self.m_actions(ui));
            remember(&ctx, "m_actions_w", r.response.rect.width());
        }
    }

    fn m_brand(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let status = self.shared.status().clone();
        let narrow = ui.available_width() < 470.0;
        kit::island(p, 255, Margin::symmetric(12, 6)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
                ui.painter().rect_filled(
                    rect.translate(Vec2::new(0.0, 3.0)),
                    CornerRadius::same(13),
                    kit::alpha(kit::INDIGO, 0.3),
                );
                kit::gradient_rect(ui, rect, 13.0, kit::BLUE, kit::FUCHSIA);
                if let Some(logo) = &self.logo {
                    egui::Image::new((logo.id(), Vec2::splat(24.0)))
                        .corner_radius(6)
                        .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(24.0)));
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Extendo").font(kit::bold(15.0)).color(p.text));
                        Frame::default()
                            .fill(kit::tint(p, kit::INDIGO))
                            .stroke(Stroke::new(1.0, kit::alpha(kit::INDIGO, 0.3)))
                            .corner_radius(CornerRadius::same(255))
                            .inner_margin(Margin::symmetric(7, 1))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                        .font(kit::mono(10.0))
                                        .color(kit::ink(p, kit::INDIGO)),
                                );
                            });
                    });
                    ui.label(
                        RichText::new(format!("Host · {}", self.host_name))
                            .size(11.0)
                            .color(p.muted),
                    );
                });
                if !narrow {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(1.0, 22.0), Sense::hover());
                    ui.painter()
                        .vline(r.center().x, r.y_range(), Stroke::new(1.0, p.glass_border));
                }
                let connected = status.viewer.is_some();
                match (&status.server_error, &status.viewer) {
                    (Some(e), _) => {
                        status_badge(ui, p, "Not running", kit::RED, false);
                        ui.label("").on_hover_text(e);
                    }
                    (None, Some(v)) => {
                        let via = if v.usb { "USB" } else { "WiFi" };
                        status_badge(ui, p, &format!("Connected · {via}"), kit::GREEN, false);
                    }
                    (None, None) => status_badge(ui, p, "Waiting for device", kit::AMBER, true),
                }
                if connected {
                    if grad_pill(ui, "📤  Send files", kit::M_SCREEN_GRAD, true).clicked() {
                        self.pick_and_send();
                    }
                } else if grad_pill(ui, "How to connect", kit::M_SCREEN_GRAD, true)
                    .on_hover_text("Open the connection guide")
                    .clicked()
                {
                    self.welcome_step = 1;
                    self.update_setting(|s| s.welcome_done = false);
                }
            });
        });
    }

    fn m_actions(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let code = self.shared.status().pairing_code.clone();
        let dark = self.shared.settings().dark_theme;
        let log_count = self.shared.log.lock().unwrap().len();
        let count = if log_count > 99 {
            "99+".to_string()
        } else {
            log_count.to_string()
        };
        kit::island(p, 255, Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let tls = format!("🔒  TLS {code}");
                for (drawer, text, badge) in [
                    (Drawer::Wifi, "📶  WiFi & USB", None),
                    (Drawer::Security, tls.as_str(), None),
                    (Drawer::Options, "⚙  Options", None),
                    (Drawer::Log, "⚡  Logs & Info", Some(count.as_str())),
                ] {
                    let open = self.drawer == Some(drawer);
                    if kit::pill(ui, p, text, open, badge).clicked() {
                        self.drawer = if open { None } else { Some(drawer) };
                    }
                }
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
                ui.painter().circle(
                    rect.center(),
                    14.5,
                    if resp.hovered() {
                        p.field
                    } else if p.dark {
                        p.inset
                    } else {
                        Color32::WHITE
                    },
                    Stroke::new(1.0, p.glass_border),
                );
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    if dark { "☀" } else { "🌙" },
                    FontId::proportional(14.0),
                    if dark { kit::AMBER } else { kit::INDIGO },
                );
                if resp
                    .on_hover_text("Switch light / dark")
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .clicked()
                {
                    self.update_setting(|s| s.dark_theme = !dark);
                }
            });
        });
    }

    /// WiFi, Security and Options open as a popover under the header; the
    /// log as a full-height drawer.
    fn m_popover(&mut self, ctx: &egui::Context) {
        let Some(which) = self.drawer else { return };
        if which == Drawer::Log {
            self.drawer(ctx);
            return;
        }
        let p = self.palette();
        let screen = ctx.content_rect();
        let width = (screen.width() - 28.0).min(440.0);
        let max_h = (screen.height() - 110.0).max(200.0);
        let id = egui::Id::new("m_popover");
        let frame = kit::island(p, 24, Margin::same(16)).fill(solid_fill(p));
        let mut close = false;
        let host = self.host_name.clone();
        let response = egui::Modal::new(id)
            .area(egui::Modal::default_area(id).anchor(Align2::RIGHT_TOP, Vec2::new(-14.0, 72.0)))
            .frame(frame)
            .backdrop_color(kit::alpha(Color32::BLACK, if p.dark { 0.3 } else { 0.08 }))
            .show(ctx, |ui| {
                ui.set_width(width - 32.0);
                let (title, sub) = match which {
                    Drawer::Wifi => (
                        "WiFi & USB".to_string(),
                        "How a tablet reaches this PC".to_string(),
                    ),
                    Drawer::Security => (
                        "Security".to_string(),
                        "Encrypted connections and trusted tablets".to_string(),
                    ),
                    _ => (
                        "Host options".to_string(),
                        format!("Background service and desktop · {host}"),
                    ),
                };
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(RichText::new(title).font(kit::bold(16.0)).color(p.text));
                        ui.label(RichText::new(sub).size(11.5).color(p.muted));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (rect, resp) =
                            ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().circle_filled(rect.center(), 13.0, p.field);
                        }
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            "×",
                            FontId::proportional(17.0),
                            p.muted,
                        );
                        if resp
                            .on_hover_text("Close (Esc)")
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
                ui.add_space(6.0);
                let (r, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
                ui.painter()
                    .hline(r.x_range(), r.center().y, Stroke::new(1.0, p.inset_border));
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .max_height(max_h - 90.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| match which {
                        Drawer::Wifi => self.wifi_panel(ui),
                        Drawer::Security => self.security_drawer(ui),
                        _ => self.options_drawer(ui),
                    });
            });
        if close || response.should_close() {
            self.drawer = None;
        }
    }

    /// The PC's WiFi addresses and USB status.
    pub(super) fn wifi_panel(&mut self, ui: &mut Ui) {
        let p = self.palette();
        if self.addresses_at.elapsed() > Duration::from_secs(10) {
            self.addresses = local_addresses();
            self.addresses_at = Instant::now();
        }
        let status = self.shared.status().clone();
        let settings = self.shared.settings();
        ui.horizontal_wrapped(|ui| {
            kit::badge(ui, p, "WIFI ENDPOINTS", kit::BLUE);
            ui.label(
                RichText::new("Same network: the tablet app finds this PC by itself")
                    .size(11.5)
                    .color(p.muted),
            );
        });
        ui.add_space(6.0);
        if !settings.wifi {
            kit::tinted(p, kit::AMBER, 12).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("WiFi is off: USB only. Turn it on in Options.")
                        .color(kit::ink(p, kit::AMBER)),
                );
            });
        } else if self.addresses.is_empty() {
            ui.label(RichText::new("No network connection").color(kit::ink(p, kit::AMBER)));
        } else {
            for ip in self.addresses.clone() {
                let text = format!("{ip}:{}", settings.port);
                let overlay = matches!(ip, IpAddr::V4(v) if v.octets()[0] == 100 && (v.octets()[1] & 0xc0) == 64);
                let (kind, color) = if overlay {
                    ("OVERLAY / MESH IP", kit::GREEN)
                } else {
                    ("LOCAL NETWORK", kit::BLUE)
                };
                kit::inset(p, 12).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(RichText::new(kind).font(kit::semi(10.5)).color(p.muted));
                            ui.label(
                                RichText::new(&text)
                                    .font(kit::mono(13.5))
                                    .color(kit::ink(p, color)),
                            );
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let grad = if overlay {
                                kit::M_KEYS_GRAD
                            } else {
                                kit::M_SCREEN_GRAD
                            };
                            if grad_pill(ui, "Copy", grad, true).clicked() {
                                self.copy(ui.ctx(), &text, &text);
                            }
                        });
                    });
                });
                ui.add_space(4.0);
            }
        }
        ui.add_space(8.0);
        kit::inset(p, 12).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("🔌  Over USB cable")
                    .font(kit::semi(13.5))
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
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            kit::note(ui, p, "No tablet app yet?");
            ui.hyperlink_to("Download the Android app", ANDROID_APP_URL);
        });
    }

    // ------------------------------------------------------------ dock

    fn m_dock(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let tabs = [
            (MTab::Screen, "Screen", kit::BLUE),
            (MTab::Mirror, "Mirror", kit::ROSE),
            (MTab::Storage, "Storage", kit::AMBER),
            (MTab::Keys, "Keys", kit::GREEN),
        ];
        kit::island(p, 255, Margin::same(5)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (tab, label, dot) in tabs {
                    let font = kit::semi(13.5);
                    let (rect, resp) = ui.allocate_exact_size(
                        Vec2::new(text_w(ui, label, &font) + 46.0, DOCK_H - 10.0),
                        Sense::click(),
                    );
                    let selected = self.mtab == tab;
                    let grad = tab.grad();
                    if selected {
                        ui.painter().rect_filled(
                            rect.translate(Vec2::new(0.0, 3.0)),
                            CornerRadius::same(255),
                            kit::alpha(grad.0, 0.25),
                        );
                        kit::gradient_rect(ui, rect, rect.height() / 2.0, grad.0, grad.1);
                    } else {
                        ui.painter().rect(
                            rect,
                            CornerRadius::same(255),
                            if resp.hovered() {
                                p.field
                            } else if p.dark {
                                p.inset
                            } else {
                                Color32::WHITE
                            },
                            Stroke::new(1.0, p.glass_border),
                            egui::StrokeKind::Inside,
                        );
                    }
                    ui.painter().circle_filled(
                        Pos2::new(rect.left() + 18.0, rect.center().y),
                        3.5,
                        if selected { Color32::WHITE } else { dot },
                    );
                    ui.painter().text(
                        Pos2::new(rect.left() + 29.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        label,
                        font,
                        if selected { Color32::WHITE } else { p.text },
                    );
                    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        self.mtab = tab;
                    }
                }
            });
        });
    }

    // ------------------------------------------------------------ ribbon

    fn m_ribbon(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let w = ui.available_width();
        kit::island(p, 22, Margin::symmetric(16, 10)).show(ui, |ui| {
            ui.set_width(w - 32.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 8.0);
                match self.mtab {
                    MTab::Screen => self.m_ribbon_screen(ui),
                    MTab::Mirror => self.m_ribbon_mirror(ui),
                    MTab::Storage => self.m_ribbon_storage(ui),
                    MTab::Keys => self.m_ribbon_keys(ui),
                }
            });
        });
    }

    fn m_ribbon_screen(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let s = self.shared.settings();
        let grad = kit::M_SCREEN_GRAD;
        fit(ui, 230.0);
        rlabel(ui, p, "Mode");
        if let Some(m) = kit::segmented(
            ui,
            p,
            &[(Mode::Extend, "Extend"), (Mode::Mirror, "Mirror")],
            s.mode,
            grad,
            150.0,
        ) {
            self.update_setting(|s| s.mode = m);
        }
        ui.add_space(6.0);
        fit(ui, 280.0);
        rlabel(ui, p, "Monitor");
        let current = s.monitor.clone().unwrap_or_default();
        let label = self
            .monitors
            .iter()
            .find(|(v, _)| *v == current)
            .map(|(_, l)| l.clone())
            .unwrap_or_else(|| "Automatic".into());
        egui::ComboBox::from_id_salt("m_monitor")
            .selected_text(RichText::new(label).font(kit::semi(12.5)))
            .width(160.0)
            .height(300.0)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(s.monitor.is_none(), "Automatic")
                    .clicked()
                {
                    self.update_setting(|s| s.monitor = None);
                }
                for (value, label) in self.monitors.clone() {
                    if ui
                        .selectable_label(s.monitor.as_deref() == Some(&value), label)
                        .clicked()
                    {
                        self.update_setting(|s| s.monitor = Some(value.clone()));
                    }
                }
            });
        ui.add_space(6.0);
        fit(ui, 200.0);
        rlabel(ui, p, "Scale");
        let scale_label = SCALES
            .iter()
            .find(|(v, _)| (*v - s.scale).abs() < 0.01)
            .map(|(_, l)| l.to_string())
            .unwrap_or_else(|| format!("{:.0}%", s.scale * 100.0));
        egui::ComboBox::from_id_salt("m_scale")
            .selected_text(RichText::new(scale_label).font(kit::mono(12.5)))
            .width(84.0)
            .show_ui(ui, |ui| {
                for (v, l) in SCALES {
                    if ui
                        .selectable_label((v - s.scale).abs() < 0.01, l)
                        .on_hover_text("Makes text and windows bigger on the tablet")
                        .clicked()
                    {
                        self.update_setting(|s| s.scale = v);
                    }
                }
            })
            .response
            .on_hover_text("Makes text and windows bigger on the tablet");
        ui.add_space(6.0);
        fit(ui, 280.0);
        rlabel(ui, p, "Frame rate");
        if let Some(f) = kit::segmented(
            ui,
            p,
            &[(30, "30"), (60, "60"), (90, "90"), (120, "120")],
            s.fps,
            grad,
            170.0,
        ) {
            self.update_setting(|s| s.fps = f);
        }
        ui.add_space(6.0);
        fit(ui, 310.0);
        rlabel(ui, p, "Bitrate");
        let mut bitrate = s.bitrate_mbps;
        if kit::gradient_slider(ui, p, &mut bitrate, 2..=60, grad, 130.0)
            .on_hover_text("Higher = sharper, needs a faster link")
            .changed()
        {
            self.update_setting(|s| s.bitrate_mbps = bitrate);
        }
        ui.label(
            RichText::new(format!("{} Mbit/s", s.bitrate_mbps))
                .font(kit::mono(12.0))
                .color(kit::ink(p, kit::BLUE)),
        );
    }

    fn m_ribbon_mirror(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let m = self.shared.settings().mirror;
        let mut next = m.clone();
        let grad = kit::M_MIRROR_GRAD;
        let label = match next.max_size {
            0 => "Full res".to_string(),
            n => format!("{n} px"),
        };
        egui::ComboBox::from_id_salt("m_mirror_size")
            .selected_text(RichText::new(label).font(kit::semi(12.5)))
            .width(96.0)
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
            })
            .response
            .on_hover_text("Resolution of the mirror window");
        fit(ui, 170.0);
        if let Some(f) = kit::segmented(
            ui,
            p,
            &[(30, "30"), (60, "60"), (90, "90"), (120, "120")],
            next.max_fps,
            grad,
            160.0,
        ) {
            next.max_fps = f;
        }
        fit(ui, 190.0);
        kit::gradient_slider(ui, p, &mut next.bitrate_mbps, 2..=32, grad, 110.0)
            .on_hover_text("Bitrate");
        ui.label(
            RichText::new(format!("{} Mb/s", next.bitrate_mbps))
                .font(kit::mono(12.0))
                .color(kit::ink(p, kit::ROSE)),
        );
        ui.add_space(4.0);
        let chips: [(&str, &str, &mut bool); 11] = [
            (
                "Nav Bar",
                "Navigation bar at the bottom of the mirror window",
                &mut next.navbar,
            ),
            (
                "Screen Off",
                "Turn the phone screen off while mirroring (saves battery; Alt+O anytime)",
                &mut next.screen_off,
            ),
            (
                "PC Sound",
                "Play the phone's sound on this PC (Android 11+)",
                &mut next.audio,
            ),
            (
                "Keep Awake",
                "Keep the phone awake while plugged in",
                &mut next.stay_awake,
            ),
            (
                "HID Kbd",
                "Act as a physical keyboard (games, keyboard layouts)",
                &mut next.physical_keyboard,
            ),
            (
                "Touches",
                "Show touches on the phone",
                &mut next.show_touches,
            ),
            (
                "On Top",
                "Keep the mirror window on top",
                &mut next.always_on_top,
            ),
            ("Fullscreen", "Start in fullscreen", &mut next.fullscreen),
            (
                "Auto-Lock",
                "Lock the phone when the window closes",
                &mut next.power_off_on_close,
            ),
            (
                "Record",
                "Record to a video in Downloads/Extendo/Recordings",
                &mut next.record,
            ),
            (
                "Camera",
                "Show the phone's camera instead of its screen (Android 12+)",
                &mut next.camera,
            ),
        ];
        for (label, tip, value) in chips {
            kit::toggle_chip(ui, p, label, value, grad).on_hover_text(tip);
        }
        if next != m {
            self.update_setting(|s| s.mirror = next);
        }
    }

    fn m_ribbon_storage(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let status = self.shared.status().clone();
        let address = self.storage_address.trim().to_string();
        if !address.is_empty() {
            kit::badge(ui, p, &format!("ftp://{address}"), kit::ORANGE);
        }
        ui.label(RichText::new("Login:").size(12.0).color(p.muted));
        ui.label(RichText::new("extendo").font(kit::mono(12.0)).color(p.text));
        kit::note(ui, p, "· password shown on the phone");
        ui.add_space(8.0);
        if status.storage_searching {
            ui.spinner();
        } else if kit::link_button(ui, p, "⟳ Scan LAN", kit::ORANGE)
            .on_hover_text("Find phones sharing their storage")
            .clicked()
        {
            storage::search(self.shared.clone());
        }
        if grad_pill(
            ui,
            "Open in File Manager",
            kit::M_STORAGE_GRAD,
            !address.is_empty(),
        )
        .on_hover_text("Opens ftp:// in your file manager")
        .clicked()
        {
            self.update_setting(|s| s.storage_address = address.clone());
            storage::open(&address);
        }
    }

    fn m_ribbon_keys(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let all = all_shortcuts();
        let count = all.len();
        fit(ui, 210.0);
        kit::text_field(
            ui,
            p,
            &mut self.key_filter,
            &format!("Filter {count} shortcuts…"),
            200.0,
        );
        let filter = self.key_filter.trim().to_lowercase();
        for g in KeyGroup::LIST {
            let n = all
                .iter()
                .filter(|(gg, k, w)| (g == KeyGroup::All || *gg == g) && key_matches(&filter, k, w))
                .count();
            let label = format!("{} ({n})", g.label());
            if kit::chip(ui, p, &label, g.color(), self.key_group == g, None).clicked() {
                self.key_group = g;
            }
        }
    }

    // ------------------------------------------------------------ stage

    fn m_stage(&mut self, ui: &mut Ui, view_h: f32) {
        let ctx = ui.ctx().clone();
        let last = mem(&ctx, "m_stage_h", 0.0);
        ui.add_space(((view_h - last) / 2.0).max(0.0));
        let full = ui.available_width();
        let width = full.min(1240.0);
        let pad = ((full - width) / 2.0).max(0.0);
        let r = ui.horizontal_top(|ui| {
            ui.add_space(pad);
            ui.vertical(|ui| {
                ui.set_width(width);
                match self.mtab {
                    MTab::Screen => self.m_screen(ui),
                    MTab::Mirror => self.m_mirror(ui),
                    MTab::Storage => self.m_storage(ui),
                    MTab::Keys => self.m_keys(ui),
                }
            });
        });
        remember(&ctx, "m_stage_h", r.response.rect.height() + 4.0);
    }

    /// Lays out `[left, middle, right]` columns, centred.
    fn columns3(&mut self, ui: &mut Ui, widths: [f32; 3], gap: f32, add: [&mut Column<'_>; 3]) {
        let total = widths.iter().sum::<f32>() + gap * 2.0;
        let pad = ((ui.available_width() - total) / 2.0).max(0.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.add_space((pad - gap).max(0.0));
            for (w, f) in widths.into_iter().zip(add) {
                ui.allocate_ui_with_layout(Vec2::new(w, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(w);
                    f(self, ui);
                });
            }
        });
    }

    /// One centred column `width` wide.
    fn centred(&mut self, ui: &mut Ui, width: f32, add: impl FnOnce(&mut App, &mut Ui)) {
        let width = width.min(ui.available_width());
        let pad = ((ui.available_width() - width) / 2.0).max(0.0);
        ui.horizontal_top(|ui| {
            ui.add_space(pad);
            ui.allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(width);
                add(self, ui);
            });
        });
    }

    // ------------------------------------------------------------ screen

    fn m_screen(&mut self, ui: &mut Ui) {
        let full = ui.available_width();
        let live = self.shared.status().viewer.is_some();
        if full >= 860.0 {
            let link = 230.0;
            let card = ((full - link - 32.0) / 2.0).min(450.0);
            let link_top = (mem(ui.ctx(), "m_laptop_h", 250.0) / 2.0 - 70.0).max(0.0);
            self.columns3(
                ui,
                [card, link, card],
                16.0,
                [
                    &mut |app, ui| app.m_laptop(ui),
                    &mut |app, ui| {
                        ui.add_space(link_top);
                        app.m_link(ui, false, live);
                    },
                    &mut |app, ui| {
                        ui.add_space(14.0);
                        app.m_tablet(ui);
                    },
                ],
            );
        } else {
            self.centred(ui, 500.0, |app, ui| {
                app.m_laptop(ui);
                ui.add_space(10.0);
                app.m_link(ui, true, live);
                ui.add_space(6.0);
                app.m_tablet(ui);
            });
        }
        ui.add_space(22.0);
        self.centred(ui, 900.0, |app, ui| app.m_screen_extras(ui));
    }

    fn m_laptop(&mut self, ui: &mut Ui) {
        let s = self.shared.settings();
        let status = self.shared.status().clone();
        let monitor = s
            .monitor
            .as_ref()
            .and_then(|m| self.monitors.iter().find(|(v, _)| v == m))
            .map(|(_, l)| l.clone())
            .unwrap_or_else(|| "Automatic".into());
        let w = ui.available_width();
        let r = hw_frame(12).show(ui, |ui| {
            ui.set_width(w - 24.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                for c in [kit::RED, kit::AMBER, kit::GREEN] {
                    kit::dot(ui, c, 4.5);
                }
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("Primary display · {monitor}"))
                            .font(kit::mono(11.5))
                            .color(HW_ACCENT),
                    )
                    .truncate(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    hw_chip(ui, &format!("{:.0}%", s.scale * 100.0), HW_MUTED);
                });
            });
            ui.add_space(8.0);
            Frame::default()
                .fill(HW_INNER)
                .stroke(Stroke::new(1.0, Color32::from_white_alpha(14)))
                .corner_radius(CornerRadius::same(16))
                .inner_margin(Margin::same(14))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            let os = if cfg!(windows) { "Windows" } else { "Linux" };
                            ui.label(
                                RichText::new(format!("{os} workstation ({})", self.host_name))
                                    .font(kit::semi(14.0))
                                    .color(Color32::WHITE),
                            );
                            ui.label(RichText::new(session_name()).size(11.5).color(HW_MUTED));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            let usb: Vec<&AdbDevice> =
                                status.usb_devices.iter().filter(|d| !d.wifi).collect();
                            if !s.usb {
                                hw_pill(ui, "🔌 USB: off in Options", kit::AMBER);
                            } else if status.adb_path.is_none() {
                                hw_pill(ui, "🔌 adb not found", kit::AMBER);
                            } else if let Some(d) = usb.first() {
                                hw_pill(ui, &format!("🔌 {}", d.model), kit::GREEN);
                            } else {
                                hw_pill(ui, "🔌 USB ready", kit::BLUE);
                            }
                        });
                    });
                    ui.add_space(10.0);
                    self.m_received(ui);
                });
        });
        let rect = r.response.rect;
        remember(ui.ctx(), "m_laptop_h", rect.height());
        // Aluminium lip.
        let lip = Rect::from_min_size(
            Pos2::new(rect.left() - 10.0, rect.bottom() - 2.0),
            Vec2::new(rect.width() + 20.0, 9.0),
        );
        let p = self.palette();
        ui.painter().rect_filled(
            lip,
            CornerRadius {
                nw: 2,
                ne: 2,
                sw: 8,
                se: 8,
            },
            if p.dark {
                Color32::from_rgb(0x1e, 0x29, 0x3b)
            } else {
                Color32::from_rgb(0xcb, 0xd5, 0xe1)
            },
        );
        ui.add_space(10.0);
    }

    /// Received folder and transfers, inside the laptop.
    fn m_received(&mut self, ui: &mut Ui) {
        let status = self.shared.status().clone();
        let settings = self.shared.settings();
        let received = status
            .transfers
            .iter()
            .filter(|t| t.incoming && t.state == TransferState::Done)
            .count();
        Frame::default()
            .fill(HW_DEEP)
            .stroke(Stroke::new(1.0, Color32::from_white_alpha(12)))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("RECEIVED FILES")
                            .font(kit::mono(11.0))
                            .color(HW_ACCENT),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{received} received"))
                                .font(kit::mono(11.0))
                                .color(kit::lerp(kit::GREEN, Color32::WHITE, 0.2)),
                        );
                    });
                });
                let dir = settings.save_dir.display().to_string();
                ui.add(
                    egui::Label::new(RichText::new(&dir).font(kit::mono(11.5)).color(HW_TEXT))
                        .truncate(),
                )
                .on_hover_text(dir);
                for t in status.transfers.iter().rev().take(4) {
                    ui.horizontal(|ui| {
                        let arrow = if t.incoming { "⬇" } else { "⬆" };
                        ui.label(RichText::new(arrow).color(HW_ACCENT));
                        ui.add(
                            egui::Label::new(RichText::new(&t.name).size(12.0).color(HW_TEXT))
                                .truncate(),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &t.state {
                            TransferState::Running => {
                                let frac = if t.size > 0 {
                                    t.done as f32 / t.size as f32
                                } else {
                                    0.0
                                };
                                ui.add(
                                    egui::ProgressBar::new(frac)
                                        .desired_width(90.0)
                                        .fill(kit::BLUE)
                                        .show_percentage(),
                                );
                            }
                            TransferState::Done => {
                                if t.incoming {
                                    if let Some(path) = &t.path {
                                        if hw_button(ui, "Open").clicked() {
                                            crate::desktop::open_file(path);
                                        }
                                    }
                                }
                                ui.label(
                                    RichText::new(format!("✔ {}", human_size(t.size)))
                                        .size(11.5)
                                        .color(kit::lerp(kit::GREEN, Color32::WHITE, 0.2)),
                                );
                            }
                            TransferState::Failed(e) => {
                                ui.label(
                                    RichText::new(format!("✖ {e}")).size(11.5).color(kit::lerp(
                                        kit::RED,
                                        Color32::WHITE,
                                        0.25,
                                    )),
                                );
                            }
                        });
                    });
                }
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if grad_pill(ui, "📂 Open folder", kit::M_SCREEN_GRAD, true).clicked() {
                        let _ = std::fs::create_dir_all(&settings.save_dir);
                        crate::desktop::open_in_file_manager(
                            &settings.save_dir.display().to_string(),
                        );
                    }
                    if hw_button(ui, "Change…").clicked() {
                        let shared = self.shared.clone();
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
                        && hw_button(ui, "Clear finished").clicked()
                    {
                        self.shared.update_status(|s| {
                            s.transfers.retain(|t| t.state == TransferState::Running)
                        });
                    }
                });
            });
    }

    /// Mode switch, the data line and its numbers.
    fn m_link(&mut self, ui: &mut Ui, vertical: bool, live: bool) {
        let p = self.palette();
        let s = self.shared.settings();
        let viewer = self.shared.status().viewer.clone();
        ui.vertical_centered(|ui| {
            if kit::soft_button(ui, p, "Extend ↔ Mirror", Some("Ctrl+Alt+E"))
                .on_hover_text("Switch the mode; a connected tablet reconnects with it")
                .clicked()
            {
                crate::hotkeys::run(crate::hotkeys::Action::ToggleMode);
            }
            ui.add_space(6.0);
            let size = if vertical {
                Vec2::new(60.0, 56.0)
            } else {
                Vec2::new(ui.available_width().min(170.0), 44.0)
            };
            fiber(ui, size, kit::M_SCREEN_GRAD, live, vertical);
            ui.add_space(6.0);
            let mode = match s.mode {
                Mode::Extend => "Extend",
                Mode::Mirror => "Mirror",
            };
            let text = match &viewer {
                Some(v) => format!("{mode} · {:.0} fps · {:.1} Mbit/s", v.fps, v.mbps),
                None => format!("{mode} · {} Mbit/s · {} Hz", s.bitrate_mbps, s.fps),
            };
            kit::island(p, 255, Margin::symmetric(12, 4)).show(ui, |ui| {
                ui.label(
                    RichText::new(text)
                        .font(kit::mono(11.5))
                        .color(kit::ink(p, kit::BLUE)),
                );
            });
        });
    }

    fn m_tablet(&mut self, ui: &mut Ui) {
        let s = self.shared.settings();
        let viewer = self.shared.status().viewer.clone();
        let connected = viewer.is_some();
        let monitor = s
            .monitor
            .as_ref()
            .and_then(|m| self.monitors.iter().find(|(v, _)| v == m))
            .map(|(_, l)| l.clone())
            .unwrap_or_else(|| "the main display".into());
        let w = ui.available_width();
        hw_frame(12).show(ui, |ui| {
            ui.set_width(w - 24.0);
            ui.horizontal(|ui| {
                kit::dot(ui, kit::BLUE, 4.0);
                let name = viewer
                    .as_ref()
                    .map(|v| v.name.clone())
                    .unwrap_or_else(|| "Extendo tablet".into());
                ui.add(
                    egui::Label::new(RichText::new(name).font(kit::mono(11.5)).color(HW_TEXT))
                        .truncate(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let text = match &viewer {
                        Some(v) => format!("{:.0} fps · {:.1} Mbit/s", v.fps, v.mbps),
                        None => format!(
                            "{:.0}% · {} Hz · {} Mbit/s",
                            s.scale * 100.0,
                            s.fps,
                            s.bitrate_mbps
                        ),
                    };
                    hw_chip(ui, &text, HW_ACCENT);
                });
            });
            ui.add_space(8.0);
            let bg = ui.painter().add(egui::Shape::Noop);
            let inner = Frame::default()
                .inner_margin(Margin::same(14))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            let (title, sub) = match s.mode {
                                Mode::Extend => (
                                    "Extended desktop",
                                    format!("A new screen next to {monitor}"),
                                ),
                                Mode::Mirror => ("Mirrored display", format!("Shows {monitor}")),
                            };
                            ui.label(
                                RichText::new(title)
                                    .font(kit::bold(16.0))
                                    .color(Color32::WHITE),
                            );
                            ui.label(
                                RichText::new(sub)
                                    .size(12.0)
                                    .color(Color32::from_white_alpha(200)),
                            );
                            if let Some(v) = &viewer {
                                let mut parts = Vec::new();
                                if v.width > 0 {
                                    parts.push(format!("{} × {}", v.width, v.height));
                                }
                                if let Some(d) = v.device_status {
                                    if let Some(b) = d.battery {
                                        parts.push(format!(
                                            "🔋 {b}%{}",
                                            if d.charging { " ⚡" } else { "" }
                                        ));
                                    }
                                    if let Some(t) = d.temperature {
                                        parts.push(format!("🌡 {t:.1} °C"));
                                    }
                                }
                                if !parts.is_empty() {
                                    ui.label(
                                        RichText::new(parts.join("  ·  "))
                                            .font(kit::mono(11.0))
                                            .color(Color32::from_white_alpha(220)),
                                    );
                                }
                            }
                        });
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            let (r, _) =
                                ui.allocate_exact_size(Vec2::new(60.0, 42.0), Sense::hover());
                            ui.painter().rect(
                                r,
                                CornerRadius::same(10),
                                Color32::from_white_alpha(36),
                                Stroke::new(1.0, Color32::from_white_alpha(60)),
                                egui::StrokeKind::Inside,
                            );
                            let a = Rect::from_min_size(
                                r.min + Vec2::new(8.0, 8.0),
                                Vec2::new(20.0, 26.0),
                            );
                            let b = Rect::from_min_size(
                                r.min + Vec2::new(32.0, 8.0),
                                Vec2::new(20.0, 26.0),
                            );
                            ui.painter().rect_filled(
                                a,
                                CornerRadius::same(5),
                                Color32::from_rgb(0xe0, 0xe7, 0xff),
                            );
                            ui.painter()
                                .rect_filled(b, CornerRadius::same(5), kit::AMBER);
                        });
                    });
                    ui.add_space(12.0);
                    let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
                    let (rect, resp) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 58.0),
                        if connected {
                            Sense::click()
                        } else {
                            Sense::hover()
                        },
                    );
                    let lit = connected && (resp.hovered() || hovering);
                    ui.painter().rect_filled(
                        rect,
                        CornerRadius::same(12),
                        Color32::from_black_alpha(if lit { 30 } else { 55 }),
                    );
                    let rr = rect.shrink(1.0);
                    ui.painter().extend(egui::Shape::dashed_line(
                        &[
                            rr.left_top(),
                            rr.right_top(),
                            rr.right_bottom(),
                            rr.left_bottom(),
                            rr.left_top(),
                        ],
                        Stroke::new(1.2, Color32::from_white_alpha(if lit { 230 } else { 130 })),
                        6.0,
                        4.0,
                    ));
                    ui.painter().text(
                        rect.left_center() + Vec2::new(14.0, -9.0),
                        Align2::LEFT_CENTER,
                        if connected {
                            "📤 Drop or click to send files to the tablet"
                        } else {
                            "📤 Connect a tablet to send files"
                        },
                        kit::semi(12.5),
                        Color32::WHITE,
                    );
                    ui.painter().text(
                        rect.left_center() + Vec2::new(14.0, 10.0),
                        Align2::LEFT_CENTER,
                        if connected {
                            "They go to the tablet's Downloads"
                        } else {
                            "Then drop files anywhere on this window"
                        },
                        kit::mono(10.5),
                        Color32::from_white_alpha(190),
                    );
                    if connected {
                        let b = Rect::from_center_size(
                            Pos2::new(rect.right() - 34.0, rect.center().y),
                            Vec2::new(48.0, 24.0),
                        );
                        ui.painter()
                            .rect_filled(b, CornerRadius::same(255), Color32::WHITE);
                        ui.painter().text(
                            b.center(),
                            Align2::CENTER_CENTER,
                            "Send",
                            kit::semi(11.5),
                            kit::INDIGO,
                        );
                    }
                    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        self.pick_and_send();
                    }
                });
            ui.painter().set(
                bg,
                kit::gradient_shape(ui, inner.response.rect, 16.0, kit::BLUE, kit::FUCHSIA),
            );
        });
    }

    /// Under the stage: the live connection, or how to connect; display notes.
    fn m_screen_extras(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let s = self.shared.settings();
        let status = self.shared.status().clone();
        let connected = status.viewer.is_some();
        kit::island(p, 22, Margin::same(16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if let Some(err) = &status.server_error {
                ui.label(
                    RichText::new(format!("Extendo isn't running: {err}"))
                        .color(kit::ink(p, kit::RED)),
                );
                ui.add_space(6.0);
            }
            match &status.viewer {
                Some(v) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(&v.name).font(kit::bold(15.0)).color(p.text));
                        let via = if v.usb {
                            "USB cable".to_string()
                        } else {
                            format!("WiFi · {}", v.address)
                        };
                        kit::badge(ui, p, &via, kit::GREEN);
                    });
                    match v.pen {
                        PenSupport::Ready => {
                            ui.label(
                                RichText::new("✏ Pen pressure on").color(kit::ink(p, kit::GREEN)),
                            )
                            .on_hover_text(
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
                    if let Some(d) = v.device_status {
                        let low = d.battery.is_some_and(|b| b <= 15) && !d.charging;
                        let hot = d.temperature.is_some_and(|t| t >= 42.0);
                        if low || hot {
                            ui.label(
                                RichText::new(if low { "Tablet battery is low" } else { "Tablet is hot" })
                                    .color(kit::ink(p, kit::AMBER)),
                            );
                        }
                    }
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        stat(ui, p, &format!("{:.0}", v.fps), "fps", kit::BLUE);
                        stat(ui, p, &format!("{:.1}", v.mbps), "Mbit/s", kit::INDIGO);
                        stat(ui, p, &format!("{:.0} ms", v.encode_ms), "encode", kit::FUCHSIA);
                        ui.add_space(8.0);
                        let reconnect = kit::link_button(ui, p, "⟳  Reconnect", kit::BLUE)
                            .on_hover_text("Apply changed settings now")
                            .clicked();
                        let disconnect = kit::link_button(ui, p, "✖  Disconnect", kit::RED)
                            .on_hover_text("Ctrl+Alt+X")
                            .clicked();
                        if reconnect || disconnect {
                            self.shared.command(SessionCommand::Disconnect);
                        }
                    });
                }
                None => {
                    ui.label(
                        RichText::new("Open Extendo on your tablet and tap this PC, or plug it in with a USB cable.")
                            .font(kit::semi(13.5))
                            .color(p.text),
                    );
                    ui.horizontal_wrapped(|ui| {
                        if kit::link_button(ui, p, "📶 WiFi & USB details", kit::BLUE).clicked() {
                            self.drawer = Some(Drawer::Wifi);
                        }
                        ui.hyperlink_to("Get the tablet app", ANDROID_APP_URL);
                    });
                }
            }
            self.virtual_display_notes(ui, &s);
            ui.add_space(6.0);
            let owner = match &status.viewer {
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
            ui.horizontal_wrapped(|ui| {
                let (text, color) = match backend::virtual_display_installed() {
                    None => ("Virtual display · built in", kit::BLUE),
                    Some(true) => ("Virtual Display Driver · ready", kit::BLUE),
                    Some(false) => ("Virtual Display Driver · not installed", kit::AMBER),
                };
                kit::dot(ui, color, 3.5);
                ui.label(RichText::new(text).font(kit::mono(11.0)).color(kit::ink(p, color)));
                ui.label(RichText::new("·  TLS · PIN pairing").font(kit::mono(11.0)).color(p.muted));
            });
            ui.horizontal(|ui| {
                kit::note(ui, p, "Display changes in the bar below apply on the next connection.");
                if connected && kit::link_button(ui, p, "Apply now", kit::BLUE).clicked() {
                    self.shared.command(SessionCommand::Disconnect);
                }
            });
        });
    }

    // ------------------------------------------------------------ mirror

    fn m_mirror(&mut self, ui: &mut Ui) {
        let status = self.shared.status().clone();
        let settings = self.shared.settings();
        let engine = status.scrcpy.clone();
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
        let live = !status.mirroring.is_empty();
        let p = self.palette();
        let full = ui.available_width();
        if full >= 820.0 {
            let left = (full - 140.0 - 230.0 - 32.0).min(470.0);
            self.columns3(
                ui,
                [left, 140.0, 230.0],
                16.0,
                [
                    &mut |app, ui| app.m_mirror_cards(ui),
                    &mut |_, ui| {
                        ui.add_space(170.0);
                        ui.vertical_centered(|ui| {
                            fiber(ui, Vec2::new(120.0, 40.0), kit::M_MIRROR_GRAD, live, false);
                            ui.add_space(6.0);
                            let audio = if settings.mirror.audio {
                                " · Audio"
                            } else {
                                ""
                            };
                            ui.label(
                                RichText::new(format!("scrcpy{audio}"))
                                    .font(kit::mono(11.0))
                                    .color(kit::ink(p, kit::ROSE)),
                            );
                        });
                    },
                    &mut |_, ui| {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| paint_phone(ui, p, &phone, 1.0));
                    },
                ],
            );
        } else {
            ui.vertical_centered(|ui| paint_phone(ui, p, &phone, 0.8));
            ui.add_space(12.0);
            self.centred(ui, 560.0, |app, ui| app.m_mirror_cards(ui));
        }
    }

    fn m_mirror_cards(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let status = self.shared.status().clone();
        let m = self.shared.settings().mirror;
        let engine = status.scrcpy.clone();
        let ready = engine.is_some() && status.adb_path.is_some();
        // Engine.
        kit::island(p, 22, Margin::same(16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if engine.is_none() {
                ui.label(
                    RichText::new("⚠  Mirroring engine not found — reinstall Extendo")
                        .font(kit::semi(14.0))
                        .color(kit::ink(p, kit::ROSE)),
                );
                kit::note(
                    ui,
                    p,
                    if cfg!(windows) {
                        "The mirror engine is missing next to extendo.exe; reinstall Extendo."
                    } else {
                        "The mirror engine is missing; reinstall the Extendo package."
                    },
                );
            } else if status.adb_path.is_none() {
                ui.label(
                    RichText::new("⚠  adb not found: install Android platform-tools.")
                        .font(kit::semi(14.0))
                        .color(kit::ink(p, kit::AMBER)),
                );
            } else {
                ui.horizontal(|ui| {
                    kit::dot(ui, kit::GREEN, 4.0);
                    ui.label(
                        RichText::new("Mirroring engine ready")
                            .font(kit::semi(14.0))
                            .color(p.text),
                    );
                    if let Some((_, v)) = &engine {
                        ui.label(RichText::new(v).font(kit::mono(11.0)).color(p.faint));
                    }
                });
            }
            kit::note(
                ui,
                p,
                "See and control your phone on this PC, with sound. Works over a cable or WiFi.",
            );
        });
        ui.add_space(12.0);
        // Hardware controls for the first phone (the mirrored one first).
        let target = status
            .usb_devices
            .iter()
            .find(|d| status.mirroring.contains(&d.serial))
            .or(status.usb_devices.first())
            .cloned();
        kit::island(p, 22, Margin::same(16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("HARDWARE CONTROLS")
                        .font(kit::semi(11.0))
                        .color(p.muted),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let res = match m.max_size {
                        0 => "Full".to_string(),
                        n => format!("{n} px"),
                    };
                    ui.label(
                        RichText::new(format!(
                            "{res} · {} Hz · {} Mbit/s",
                            m.max_fps, m.bitrate_mbps
                        ))
                        .font(kit::mono(11.0))
                        .color(kit::ink(p, kit::ROSE)),
                    );
                });
            });
            if let Some(d) = &target {
                ui.label(
                    RichText::new(format!("📱 {}", d.model))
                        .size(12.0)
                        .color(p.muted),
                );
            }
            ui.add_space(6.0);
            use mirror::PhoneAction as A;
            let keys = [
                ("🔒", "Power", A::Power),
                ("🔊", "Vol +", A::VolumeUp),
                ("🔉", "Vol −", A::VolumeDown),
                ("🔔", "Notify", A::Notifications),
                ("◁", "Back", A::Back),
                ("○", "Home", A::Home),
                ("□", "Recents", A::Recents),
                ("📷", "Shot", A::Screenshot),
            ];
            let (cols, w) = kit::grid_columns(ui.available_width(), 74.0, 4, 8.0);
            let enabled = ready && target.is_some();
            let mut pressed = None;
            kit::flow(ui, keys, cols, 8.0, |ui, (icon, label, action)| {
                let (rect, resp) = ui.allocate_exact_size(
                    Vec2::new(w, 54.0),
                    if enabled {
                        Sense::click()
                    } else {
                        Sense::hover()
                    },
                );
                let hovered = enabled && resp.hovered();
                ui.painter().rect(
                    rect,
                    CornerRadius::same(14),
                    if hovered {
                        kit::tint(p, kit::ROSE)
                    } else if p.dark {
                        p.inset
                    } else {
                        Color32::WHITE
                    },
                    Stroke::new(
                        1.0,
                        if hovered {
                            kit::alpha(kit::ROSE, 0.4)
                        } else {
                            p.inset_border
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                let fg = if enabled { p.text } else { p.faint };
                ui.painter().text(
                    rect.center() - Vec2::new(0.0, 9.0),
                    Align2::CENTER_CENTER,
                    icon,
                    FontId::proportional(14.0),
                    fg,
                );
                ui.painter().text(
                    rect.center() + Vec2::new(0.0, 11.0),
                    Align2::CENTER_CENTER,
                    label,
                    kit::semi(11.5),
                    fg,
                );
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    pressed = Some(action);
                }
            });
            if let (Some(action), Some(d)) = (pressed, &target) {
                mirror::phone_action(self.shared.clone(), d.serial.clone(), action);
            }
            ui.add_space(4.0);
            let running = target
                .as_ref()
                .is_some_and(|d| status.mirroring.contains(&d.serial));
            let w = ui.available_width();
            if running {
                if wide_button(ui, "■  Stop mirroring", (kit::RED, kit::ROSE), true, w).clicked()
                {
                    if let Some(d) = &target {
                        mirror::stop(&d.serial);
                    }
                }
            } else if wide_button(ui, "Open mirror window", kit::M_MIRROR_GRAD, enabled, w)
                .on_disabled_hover_text(
                    "Plug in a phone with USB debugging on, or connect one over WiFi",
                )
                .clicked()
            {
                if let Some(d) = target.clone() {
                    mirror::start(self.shared.clone(), d);
                }
            }
        });
        ui.add_space(12.0);
        kit::island(p, 22, Margin::same(16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("PHONES").font(kit::semi(11.0)).color(p.muted));
            ui.add_space(4.0);
            if ready {
                self.mirror_devices(ui);
            } else {
                kit::note(
                    ui,
                    p,
                    "Phones show up here once the engine and adb are available.",
                );
            }
        });
    }

    // ------------------------------------------------------------ storage

    fn m_storage(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let status = self.shared.status().clone();
        if self.storage_address.is_empty() {
            self.storage_address = self.shared.settings().storage_address;
        }
        self.centred(ui, 1120.0, |app, ui| {
            let w = ui.available_width();
            kit::island(p, 22, Margin::same(16)).show(ui, |ui| {
                ui.set_width(w - 32.0);
                let intro = |ui: &mut Ui| {
                    ui.horizontal_wrapped(|ui| {
                        let font = kit::mono(10.5);
                        let text = "WIRELESS DRIVE BRIDGE";
                        let (r, _) = ui.allocate_exact_size(
                            Vec2::new(text_w(ui, text, &font) + 18.0, 20.0),
                            Sense::hover(),
                        );
                        kit::gradient_rect(ui, r, 10.0, kit::AMBER, kit::ORANGE);
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, text, font, Color32::WHITE);
                        ui.label(RichText::new("User:").size(12.0).color(p.muted));
                        ui.label(RichText::new("\"extendo\"").font(kit::mono(12.0)).color(p.text));
                        ui.label(RichText::new("· password shown on the phone").size(12.0).color(p.muted));
                    });
                    ui.label(
                        RichText::new("Browse and copy your phone's files from this PC, like a USB drive (ftp://).")
                            .font(kit::semi(13.5))
                            .color(p.text),
                    );
                };
                let controls = |app: &mut App, ui: &mut Ui| {
                    if status.storage_searching {
                        ui.spinner();
                    } else if grad_pill(ui, "⟳ Find phones", kit::M_STORAGE_GRAD, true).clicked() {
                        storage::search(app.shared.clone());
                    }
                    kit::text_field(ui, p, &mut app.storage_address, "192.168.1.50", 170.0);
                    let address = app.storage_address.trim().to_string();
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(96.0, 32.0), Sense::click());
                    let fill = if p.dark { Color32::WHITE } else { HW_BODY };
                    ui.painter().rect_filled(
                        r,
                        CornerRadius::same(10),
                        if resp.hovered() { kit::lerp(fill, kit::ORANGE, 0.2) } else { fill },
                    );
                    ui.painter().text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        "Open drive",
                        kit::semi(12.5),
                        if p.dark { HW_BODY } else { Color32::WHITE },
                    );
                    if resp
                        .on_hover_text("Open the phone in your file manager (ftp://)")
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .clicked()
                        && !address.is_empty()
                    {
                        app.update_setting(|s| s.storage_address = address.clone());
                        storage::open(&address);
                    }
                };
                if ui.available_width() > 860.0 {
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(
                            Vec2::new(ui.available_width() - 420.0, 0.0),
                            Layout::top_down(Align::Min),
                            intro,
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            // Right to left: reverse order.
                            ui.scope(|ui| {
                                ui.with_layout(Layout::left_to_right(Align::Center), |ui| controls(app, ui));
                            });
                        });
                    });
                } else {
                    intro(ui);
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| controls(app, ui));
                }
            });
            ui.add_space(12.0);
            let (cols, cw) = kit::grid_columns(ui.available_width(), 320.0, 2, 12.0);
            let usb: Vec<AdbDevice> =
                status.usb_devices.iter().filter(|d| !d.wifi).cloned().collect();
            let cards: [u8; 2] = [0, 1];
            kit::flow(ui, cards, cols, 12.0, |ui, which| {
                ui.allocate_ui_with_layout(Vec2::new(cw, 0.0), Layout::top_down(Align::Min), |ui| {
                    kit::island(p, 20, Margin::same(14)).show(ui, |ui| {
                        ui.set_width(cw - 28.0);
                        if which == 0 {
                            ui.label(RichText::new("📶  Over WiFi").font(kit::semi(14.0)).color(p.text));
                            if status.storage_phones.is_empty() {
                                kit::note(
                                    ui,
                                    p,
                                    "1. Open Extendo on the phone → Phone storage → Start.  2. Press Find phones, or type its address above.",
                                );
                            }
                            for ph in &status.storage_phones {
                                ui.horizontal(|ui| {
                                    kit::dot(ui, kit::GREEN, 4.0);
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(format!("{} · {}", ph.name, ph.address))
                                                .font(kit::semi(13.0))
                                                .color(p.text),
                                        )
                                        .truncate(),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if grad_pill(ui, "Open files", kit::M_STORAGE_GRAD, true).clicked() {
                                            storage::open(&format!("{}:{}", ph.address, ph.port));
                                        }
                                        kit::badge(ui, p, "READY", kit::GREEN);
                                    });
                                });
                            }
                        } else {
                            ui.label(RichText::new("🔌  Over USB cable").font(kit::semi(14.0)).color(p.text));
                            if usb.is_empty() {
                                kit::note(ui, p, "No phone plugged in with USB debugging on.");
                            }
                            for d in &usb {
                                ui.horizontal(|ui| {
                                    kit::dot(ui, kit::GREEN, 4.0);
                                    ui.label(RichText::new(&d.model).font(kit::semi(13.0)).color(p.text));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if grad_pill(ui, "Open files", kit::M_STORAGE_GRAD, true).clicked() {
                                            storage::open_usb(app.shared.clone(), d.serial.clone());
                                        }
                                    });
                                });
                            }
                        }
                    });
                });
            });
            kit::note(
                ui,
                p,
                "When asked, log in as \"extendo\" with the password shown on the phone. Only use WiFi storage on networks you trust.",
            );
        });
    }

    // ------------------------------------------------------------ keys

    fn m_keys(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let all = all_shortcuts();
        let filter = self.key_filter.trim().to_lowercase();
        let group = self.key_group;
        let shown: Vec<(KeyGroup, &str, &str)> = all
            .iter()
            .copied()
            .filter(|(g, k, w)| {
                (group == KeyGroup::All || *g == group) && key_matches(&filter, k, w)
            })
            .collect();
        self.centred(ui, 1180.0, |app, ui| {
            let w = ui.available_width();
            kit::island(p, 22, Margin::same(16)).show(ui, |ui| {
                ui.set_width(w - 32.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("{} ({})", group.label(), shown.len()))
                            .font(kit::bold(16.0))
                            .color(p.text),
                    );
                    ui.label(
                        RichText::new("Mirror window: Alt can be replaced by Super (Windows) · click a card to copy")
                            .size(11.5)
                            .color(p.muted),
                    );
                });
                ui.add_space(10.0);
                if shown.is_empty() {
                    kit::note(ui, p, "No shortcut matches.");
                }
                let (cols, cw) = kit::grid_columns(ui.available_width(), 250.0, 3, 10.0);
                let mut copied = None;
                kit::flow(ui, shown, cols, 10.0, |ui, (g, keys, what)| {
                    let color = g.color();
                    let keys = keys.replace("  ", " ");
                    let key_font = kit::mono(11.0);
                    let key_w = (text_w(ui, &keys, &key_font) + 18.0).min(cw - 24.0);
                    let galley = ui.painter().layout(
                        what.to_string(),
                        FontId::proportional(12.5),
                        p.text,
                        cw - 24.0,
                    );
                    let h = 12.0 + 22.0 + 8.0 + galley.size().y + 12.0;
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(cw, h), Sense::click());
                    ui.painter().rect(
                        rect,
                        CornerRadius::same(14),
                        if resp.hovered() {
                            kit::tint(p, color)
                        } else if p.dark {
                            p.inset
                        } else {
                            Color32::WHITE
                        },
                        Stroke::new(1.0, if resp.hovered() { kit::alpha(color, 0.4) } else { p.inset_border }),
                        egui::StrokeKind::Inside,
                    );
                    let kr = Rect::from_min_size(rect.min + Vec2::new(12.0, 12.0), Vec2::new(key_w, 22.0));
                    ui.painter().rect(
                        kr,
                        CornerRadius::same(255),
                        kit::tint(p, color),
                        Stroke::new(1.0, kit::alpha(color, 0.4)),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(kr.center(), Align2::CENTER_CENTER, &keys, key_font, kit::ink(p, color));
                    ui.painter().galley(
                        Pos2::new(rect.left() + 12.0, kr.bottom() + 8.0),
                        galley,
                        p.text,
                    );
                    if resp
                        .on_hover_text("Click to copy")
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .clicked()
                    {
                        copied = Some(keys.clone());
                    }
                });
                if let Some(k) = copied {
                    app.copy(ui.ctx(), &k, &k);
                }
                if matches!(group, KeyGroup::All | KeyGroup::Pc) {
                    ui.add_space(4.0);
                    kit::note(
                        ui,
                        p,
                        if cfg!(windows) {
                            "Ctrl+Alt+E and Ctrl+Alt+X work in any app; turn them on or off under Options."
                        } else {
                            "Ctrl+Alt+E and Ctrl+Alt+X: add them under Options (GNOME), or bind the commands in your desktop's keyboard settings."
                        },
                    );
                }
            });
        });
    }
}

/// Every shortcut with its group: mirror keys, tablet gestures, PC keys.
pub(super) fn all_shortcuts() -> Vec<(KeyGroup, &'static str, &'static str)> {
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
    all
}

pub(super) fn key_matches(filter: &str, k: &str, w: &str) -> bool {
    filter.is_empty() || k.to_lowercase().contains(filter) || w.to_lowercase().contains(filter)
}
