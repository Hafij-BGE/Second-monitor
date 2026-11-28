//! Drawing helpers for the desktop window: colours, fonts, glass cards,
//! gradient buttons, segmented controls, sliders, option tiles and keycaps.

use egui::epaint::tessellator::{path::rounded_rectangle, Path};
use egui::epaint::{CornerRadiusF32, Mesh};
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame, Margin, Pos2, Rect,
    Response, RichText, Sense, Shadow, Stroke, Ui, Vec2,
};

pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    /// Frosted card fill.
    pub glass: Color32,
    pub glass_border: Color32,
    /// Boxes inside cards.
    pub inset: Color32,
    pub inset_border: Color32,
    /// Tracks of segmented controls, text fields.
    pub field: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub dots: Color32,
    pub shadow: Color32,
}

pub const VIOLET: Color32 = Color32::from_rgb(0x7c, 0x3a, 0xed);
pub const PURPLE: Color32 = Color32::from_rgb(0x8b, 0x5c, 0xf6);
pub const CYAN: Color32 = Color32::from_rgb(0x06, 0xb6, 0xd4);
pub const TEAL: Color32 = Color32::from_rgb(0x14, 0xb8, 0xa6);
pub const PINK: Color32 = Color32::from_rgb(0xec, 0x48, 0x99);
pub const ORANGE: Color32 = Color32::from_rgb(0xf9, 0x73, 0x16);
pub const GREEN: Color32 = Color32::from_rgb(0x10, 0xb9, 0x81);
pub const AMBER: Color32 = Color32::from_rgb(0xf5, 0x9e, 0x0b);
pub const RED: Color32 = Color32::from_rgb(0xef, 0x44, 0x44);
pub const INDIGO: Color32 = Color32::from_rgb(0x63, 0x66, 0xf1);

pub const SCREEN_GRAD: (Color32, Color32) = (VIOLET, CYAN);
pub const MIRROR_GRAD: (Color32, Color32) = (PINK, ORANGE);
pub const STORAGE_GRAD: (Color32, Color32) = (GREEN, TEAL);

pub const LIGHT: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(0xf5, 0xf7, 0xff),
    glass: Color32::from_rgba_premultiplied(214, 214, 214, 214),
    glass_border: Color32::from_rgba_premultiplied(20, 20, 30, 34),
    inset: Color32::from_rgba_premultiplied(236, 239, 245, 240),
    inset_border: Color32::from_rgb(0xe2, 0xe8, 0xf0),
    field: Color32::from_rgb(0xec, 0xf0, 0xf7),
    text: Color32::from_rgb(0x0f, 0x17, 0x2a),
    muted: Color32::from_rgb(0x64, 0x74, 0x8b),
    faint: Color32::from_rgb(0x94, 0xa3, 0xb8),
    dots: Color32::from_rgba_premultiplied(18, 18, 43, 46),
    shadow: Color32::from_rgba_premultiplied(0, 0, 0, 26),
};

pub const DARK: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(0x09, 0x0b, 0x12),
    glass: Color32::from_rgba_premultiplied(12, 16, 27, 199),
    glass_border: Color32::from_rgba_premultiplied(23, 23, 23, 23),
    inset: Color32::from_rgba_premultiplied(8, 8, 10, 10),
    inset_border: Color32::from_rgba_premultiplied(18, 18, 18, 18),
    field: Color32::from_rgba_premultiplied(13, 13, 15, 15),
    text: Color32::from_rgb(0xe2, 0xe8, 0xf0),
    muted: Color32::from_rgb(0x94, 0xa3, 0xb8),
    faint: Color32::from_rgb(0x64, 0x74, 0x8b),
    dots: Color32::from_rgba_premultiplied(21, 23, 26, 36),
    shadow: Color32::from_rgba_premultiplied(0, 0, 0, 150),
};

// ------------------------------------------------- modern look palette

pub const BLUE: Color32 = Color32::from_rgb(0x3b, 0x82, 0xf6);
pub const ROSE: Color32 = Color32::from_rgb(0xf4, 0x3f, 0x5e);
pub const FUCHSIA: Color32 = Color32::from_rgb(0xa8, 0x55, 0xf7);
pub const DEEP_TEAL: Color32 = Color32::from_rgb(0x0d, 0x94, 0x88);

pub const M_SCREEN_GRAD: (Color32, Color32) = (BLUE, INDIGO);
pub const M_MIRROR_GRAD: (Color32, Color32) = (ROSE, FUCHSIA);
pub const M_STORAGE_GRAD: (Color32, Color32) = (AMBER, ORANGE);
pub const M_KEYS_GRAD: (Color32, Color32) = (GREEN, DEEP_TEAL);

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const(r, g, b, a)
}

/// Modern look, light: ceramic canvas and frosted white glass.
pub const MODERN_LIGHT: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(0xf4, 0xf6, 0xfa),
    glass: rgba(255, 255, 255, 214),
    glass_border: rgba(15, 23, 42, 22),
    inset: rgba(241, 245, 249, 222),
    inset_border: rgba(15, 23, 42, 24),
    field: Color32::from_rgb(0xee, 0xf2, 0xf7),
    text: Color32::from_rgb(0x0f, 0x17, 0x2a),
    muted: Color32::from_rgb(0x47, 0x55, 0x69),
    faint: Color32::from_rgb(0x94, 0xa3, 0xb8),
    dots: rgba(15, 23, 42, 20),
    shadow: rgba(15, 23, 42, 22),
};

/// Modern look, dark: obsidian canvas and smoked glass.
pub const MODERN_DARK: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(0x09, 0x0a, 0x0f),
    glass: rgba(19, 22, 34, 200),
    glass_border: rgba(255, 255, 255, 24),
    inset: rgba(28, 32, 48, 196),
    inset_border: rgba(255, 255, 255, 24),
    field: rgba(28, 32, 48, 222),
    text: Color32::from_rgb(0xf8, 0xfa, 0xfc),
    muted: Color32::from_rgb(0x94, 0xa3, 0xb8),
    faint: Color32::from_rgb(0x64, 0x74, 0x8b),
    dots: rgba(255, 255, 255, 14),
    shadow: rgba(0, 0, 0, 170),
};

/// Glass look (PLANNER palette v3 "Planner Glass", shared with Harbor and the
/// user's other apps): lavender -> periwinkle backdrop, translucent white glass.
pub const PLANNER_LIGHT: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(220, 224, 255),
    glass: rgba(255, 255, 255, 150),
    glass_border: rgba(255, 255, 255, 217),
    inset: rgba(255, 255, 255, 120),
    inset_border: rgba(255, 255, 255, 230),
    field: rgba(255, 255, 255, 140),
    text: Color32::from_rgb(0x1e, 0x1b, 0x3a),
    muted: Color32::from_rgb(0x5b, 0x57, 0x78),
    faint: Color32::from_rgb(0x8f, 0x8b, 0xab),
    dots: rgba(0, 0, 0, 0),
    shadow: rgba(76, 61, 180, 60),
};

/// Glass effect Soft (light): flat frosted glass, faint rim, almost no shadow
/// (planner_glass.soften()).
pub const PLANNER_LIGHT_SOFT: Palette = Palette {
    glass_border: rgba(255, 255, 255, 102),
    shadow: rgba(76, 61, 180, 24),
    ..PLANNER_LIGHT
};

/// Glass look, dark: ink-indigo backdrop and smoky violet glass.
pub const PLANNER_DARK: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(27, 31, 69),
    glass: rgba(44, 39, 84, 160),
    glass_border: rgba(255, 255, 255, 34),
    inset: rgba(255, 255, 255, 16),
    inset_border: rgba(255, 255, 255, 34),
    field: rgba(255, 255, 255, 18),
    text: Color32::from_rgb(0xee, 0xed, 0xf9),
    muted: Color32::from_rgb(0xaa, 0xa7, 0xc8),
    faint: Color32::from_rgb(0x75, 0x72, 0x9a),
    dots: rgba(0, 0, 0, 0),
    shadow: rgba(0, 0, 0, 150),
};

/// Glass effect Soft (dark).
pub const PLANNER_DARK_SOFT: Palette = Palette {
    glass_border: rgba(255, 255, 255, 15),
    shadow: rgba(0, 0, 0, 60),
    ..PLANNER_DARK
};

static GLASS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Switches the modern layout to the Lumina Glass backdrop.
pub fn set_glass(on: bool) {
    GLASS.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_glass() -> bool {
    GLASS.load(std::sync::atomic::Ordering::Relaxed)
}

static LUMINA: std::sync::Mutex<Option<([[u8; 3]; 3], f32)>> = std::sync::Mutex::new(None);

/// Lumina Colourful: the backdrop's tint colours and intensity (None = Calm).
pub fn set_lumina(tint: Option<([[u8; 3]; 3], f32)>) {
    *LUMINA.lock().unwrap_or_else(|e| e.into_inner()) = tint;
}

/// The Planner Glass backdrop: a 150-degree lavender -> periwinkle gradient
/// (dark: ink indigo).
pub fn planner_backdrop(painter: &egui::Painter, rect: Rect, dark: bool) {
    let tint = *LUMINA.lock().unwrap_or_else(|e| e.into_inner());
    let (a, b, c) = if let Some((cols, k)) = tint {
        // Lumina Colourful: only a faint tint of the colours (same as planner_glass.py)
        let (base, amt) = if dark {
            ([20.0, 16.0, 40.0], [0.09, 0.08, 0.07])
        } else {
            ([250.0, 248.0, 255.0], [0.06, 0.045, 0.055])
        };
        let mix = |i: usize| {
            let m = |j: usize| (base[j] + (cols[i][j] as f32 - base[j]) * amt[i] * k).round() as u8;
            Color32::from_rgb(m(0), m(1), m(2))
        };
        (mix(0), mix(1), mix(2))
    } else if dark {
        (Color32::from_rgb(26, 21, 51), Color32::from_rgb(27, 31, 69), Color32::from_rgb(20, 26, 58))
    } else {
        (Color32::from_rgb(232, 218, 255), Color32::from_rgb(220, 224, 255), Color32::from_rgb(203, 220, 255))
    };
    let mut mesh = Mesh::default();
    // corners: top-left a, top-right / bottom-left b, bottom-right c
    mesh.colored_vertex(rect.left_top(), a);
    mesh.colored_vertex(rect.right_top(), b);
    mesh.colored_vertex(rect.right_bottom(), c);
    mesh.colored_vertex(rect.left_bottom(), b);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

static MODERN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Switches shared controls (option tiles) to the modern look.
pub fn set_modern(on: bool) {
    MODERN.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_modern() -> bool {
    MODERN.load(std::sync::atomic::Ordering::Relaxed)
}

// ---------------------------------------------------------------- fonts

const SEMI: &str = "inter-semibold";
const BOLD: &str = "inter-bold";

/// Inter for text (regular, semibold, bold from one variable font); the
/// default fonts stay behind it for symbols and emoji.
pub fn install_fonts(ctx: &egui::Context) {
    use egui::epaint::text::{FontData, FontDefinitions, FontTweak, VariationCoords};
    static INTER: &[u8] = include_bytes!("../../assets/Inter.ttf");
    let mut fonts = FontDefinitions::default();
    let weight = |w: f32| FontTweak {
        coords: VariationCoords::new([("wght", w), ("opsz", 14.0)]),
        ..Default::default()
    };
    for (name, w) in [("inter", 440.0), (SEMI, 600.0), (BOLD, 760.0)] {
        fonts.font_data.insert(
            name.into(),
            FontData::from_static(INTER).tweak(weight(w)).into(),
        );
    }
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".into());
    for name in [SEMI, BOLD] {
        let mut list = vec![name.to_string()];
        list.extend(fallback.iter().cloned());
        fonts.families.insert(FontFamily::Name(name.into()), list);
    }
    ctx.set_fonts(fonts);
}

pub fn semi(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMI.into()))
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD.into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

// ---------------------------------------------------------------- colour

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        m(a.r(), b.r()),
        m(a.g(), b.g()),
        m(a.b(), b.b()),
        m(a.a(), b.a()),
    )
}

/// `c` at `alpha` (0..1) opacity.
pub fn alpha(c: Color32, alpha: f32) -> Color32 {
    c.gamma_multiply(alpha)
}

/// A tinted background that reads on both themes.
pub fn tint(p: &Palette, c: Color32) -> Color32 {
    alpha(c, if p.dark { 0.16 } else { 0.11 })
}

/// Accent text that stays readable on light backgrounds.
pub fn ink(p: &Palette, c: Color32) -> Color32 {
    if p.dark {
        lerp(c, Color32::WHITE, 0.25)
    } else {
        lerp(c, Color32::BLACK, 0.18)
    }
}

// ---------------------------------------------------------------- painting

/// A rounded rectangle filled with a left-to-right gradient, anti-aliased.
pub fn gradient_rect(ui: &Ui, rect: Rect, radius: f32, from: Color32, to: Color32) {
    ui.painter().add(gradient_shape(ui, rect, radius, from, to));
}

/// The shape `gradient_rect` paints, for painting behind content later.
pub fn gradient_shape(ui: &Ui, rect: Rect, radius: f32, from: Color32, to: Color32) -> egui::Shape {
    let mut points = Vec::new();
    rounded_rectangle(&mut points, rect, CornerRadiusF32::same(radius));
    let mut path = Path::default();
    path.add_line_loop(&points);
    let mut mesh = Mesh::default();
    path.fill(1.0 / ui.ctx().pixels_per_point(), Color32::WHITE, &mut mesh);
    let w = rect.width().max(1.0);
    for v in &mut mesh.vertices {
        let cover = v.color.a() as f32 / 255.0;
        v.color = alpha(lerp(from, to, (v.pos.x - rect.left()) / w), cover);
    }
    egui::Shape::mesh(mesh)
}

/// A soft round glow fading to nothing at `radius`.
pub fn glow(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(center, color);
    let n = 48;
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        mesh.colored_vertex(
            center + Vec2::new(a.cos(), a.sin()) * radius,
            Color32::TRANSPARENT,
        );
        if i > 0 {
            mesh.add_triangle(0, i, i + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// The window backdrop: colour glows in the corners and a dot grid.
pub fn backdrop(painter: &egui::Painter, rect: Rect, p: &Palette) {
    painter.rect_filled(rect, CornerRadius::ZERO, p.bg);
    let r = rect.width().max(rect.height()) * 0.5;
    let k = if p.dark { 1.5 } else { 1.0 };
    for (fx, fy, c, a) in [
        (0.15, 0.12, PURPLE, 0.14),
        (0.85, 0.15, CYAN, 0.13),
        (0.83, 0.88, ORANGE, 0.10),
        (0.15, 0.87, GREEN, 0.10),
    ] {
        let center = Pos2::new(
            rect.left() + rect.width() * fx,
            rect.top() + rect.height() * fy,
        );
        glow(painter, center, r, alpha(c, a * k));
    }
    let step = 22.0;
    let mut y = rect.top() + 11.0;
    while y < rect.bottom() {
        let mut x = rect.left() + 11.0;
        while x < rect.right() {
            painter.rect_filled(
                Rect::from_center_size(Pos2::new(x, y), Vec2::splat(1.6)),
                CornerRadius::same(1),
                p.dots,
            );
            x += step;
        }
        y += step;
    }
}

/// A frosted card.
pub fn glass(p: &Palette, radius: u8, margin: i8) -> Frame {
    Frame::default()
        .fill(p.glass)
        .stroke(Stroke::new(1.0, p.glass_border))
        .corner_radius(CornerRadius::same(radius))
        .inner_margin(Margin::same(margin))
        .shadow(Shadow {
            offset: [0, 14],
            blur: 36,
            spread: 0,
            color: p.shadow,
        })
}

/// A box inside a card.
pub fn inset(p: &Palette, margin: i8) -> Frame {
    Frame::default()
        .fill(p.inset)
        .stroke(Stroke::new(1.0, p.inset_border))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::same(margin))
}

/// A tinted, rounded box (banners, notes).
pub fn tinted(p: &Palette, c: Color32, margin: i8) -> Frame {
    Frame::default()
        .fill(tint(p, c))
        .stroke(Stroke::new(1.0, alpha(c, 0.35)))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(margin))
}

// ---------------------------------------------------------------- text

/// "● SECTION TITLE  · subtitle"
pub fn section_title(ui: &mut Ui, p: &Palette, title: &str, color: Color32, subtitle: &str) {
    ui.horizontal_wrapped(|ui| {
        dot(ui, color, 4.0);
        ui.label(RichText::new(title).font(bold(13.0)).color(ink(p, color)));
        if !subtitle.is_empty() {
            ui.label(
                RichText::new(format!("·  {subtitle}"))
                    .size(12.0)
                    .color(p.faint),
            );
        }
    });
}

/// A big card heading with a one-line description.
pub fn heading(ui: &mut Ui, p: &Palette, title: &str, description: &str) {
    ui.label(RichText::new(title).font(bold(17.0)).color(p.text));
    if !description.is_empty() {
        ui.label(RichText::new(description).size(12.5).color(p.muted));
    }
}

/// Small uppercase label above a control.
pub fn field_label(ui: &mut Ui, p: &Palette, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .font(semi(11.0))
            .color(p.muted),
    );
}

pub fn note(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(12.0).color(p.muted)).wrap());
}

pub fn dot(ui: &mut Ui, color: Color32, radius: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(radius * 2.0 + 2.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), radius, color);
}

// ---------------------------------------------------------------- controls

fn text_size(ui: &Ui, text: &str, font: &FontId) -> Vec2 {
    ui.painter()
        .layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE)
        .size()
}

/// A rounded chip: dot, text, tinted background. Not clickable.
pub fn status_pill(ui: &mut Ui, p: &Palette, text: &str, color: Color32) -> Response {
    Frame::default()
        .fill(tint(p, color))
        .stroke(Stroke::new(1.0, alpha(color, 0.4)))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                dot(ui, color, 3.5);
                ui.add(
                    egui::Label::new(RichText::new(text).font(semi(13.0)).color(ink(p, color)))
                        .wrap(),
                );
            });
        })
        .response
}

/// A clickable rounded chip (header buttons, filters).
pub fn chip(
    ui: &mut Ui,
    p: &Palette,
    text: &str,
    color: Color32,
    selected: bool,
    badge: Option<&str>,
) -> Response {
    let font = semi(12.5);
    let size = text_size(ui, text, &font);
    let badge_w = badge
        .map(|b| text_size(ui, b, &mono(11.0)).x + 12.0)
        .unwrap_or(0.0);
    let extra = if badge.is_some() { 6.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(size.x + 28.0 + badge_w + extra, 32.0),
        Sense::click(),
    );
    let hovered = resp.hovered();
    let (fill, stroke, fg) = if selected {
        (color, color, Color32::WHITE)
    } else {
        let fill = if hovered { tint(p, color) } else { p.glass };
        (fill, alpha(color, 0.35), ink(p, color))
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(255),
        fill,
        Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    painter.text(
        Pos2::new(rect.left() + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        font,
        fg,
    );
    if let Some(b) = badge {
        let r = Rect::from_min_size(
            Pos2::new(rect.right() - 12.0 - badge_w, rect.center().y - 9.0),
            Vec2::new(badge_w, 18.0),
        );
        painter.rect_filled(
            r,
            CornerRadius::same(255),
            alpha(color, if selected { 0.0 } else { 0.18 }),
        );
        painter.text(r.center(), Align2::CENTER_CENTER, b, mono(11.0), fg);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// A filled gradient button.
pub fn gradient_button(
    ui: &mut Ui,
    text: &str,
    grad: (Color32, Color32),
    enabled: bool,
) -> Response {
    let font = semi(13.5);
    let size = text_size(ui, text, &font);
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(size.x + 34.0, 38.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let (a, b) = if !enabled {
        (alpha(grad.0, 0.4), alpha(grad.1, 0.4))
    } else if resp.hovered() {
        (
            lerp(grad.0, Color32::WHITE, 0.1),
            lerp(grad.1, Color32::WHITE, 0.1),
        )
    } else {
        grad
    };
    if enabled {
        ui.painter().rect_filled(
            rect.translate(Vec2::new(0.0, 3.0)).expand(1.0),
            CornerRadius::same(14),
            alpha(grad.0, 0.18),
        );
    }
    gradient_rect(ui, rect, 13.0, a, b);
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

/// A quiet bordered button; `keys` adds a keycap on the right.
pub fn soft_button(ui: &mut Ui, p: &Palette, text: &str, keys: Option<&str>) -> Response {
    let font = semi(13.0);
    let size = text_size(ui, text, &font);
    let kw = keys
        .map(|k| text_size(ui, k, &mono(10.5)).x + 14.0)
        .unwrap_or(0.0);
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(size.x + 30.0 + if kw > 0.0 { kw + 8.0 } else { 0.0 }, 38.0),
        Sense::click(),
    );
    let fill = if resp.hovered() {
        p.inset_border
    } else {
        p.field
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(13),
        fill,
        Stroke::new(1.0, p.inset_border),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 15.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        font,
        p.text,
    );
    if let Some(k) = keys {
        let r = Rect::from_min_size(
            Pos2::new(rect.right() - 12.0 - kw, rect.center().y - 10.0),
            Vec2::new(kw, 20.0),
        );
        ui.painter().rect(
            r,
            CornerRadius::same(6),
            p.glass,
            Stroke::new(1.0, p.inset_border),
            egui::StrokeKind::Inside,
        );
        ui.painter()
            .text(r.center(), Align2::CENTER_CENTER, k, mono(10.5), p.muted);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// A small text button for secondary actions inside cards.
pub fn link_button(ui: &mut Ui, p: &Palette, text: &str, color: Color32) -> Response {
    let font = semi(12.5);
    let size = text_size(ui, text, &font);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(size.x + 22.0, 30.0), Sense::click());
    let fill = if resp.hovered() {
        alpha(color, 0.22)
    } else {
        tint(p, color)
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(10),
        fill,
        Stroke::new(1.0, alpha(color, 0.3)),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        font,
        ink(p, color),
    );
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Equal-width segments; the selected one is filled with the gradient.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    p: &Palette,
    options: &[(T, &str)],
    current: T,
    grad: (Color32, Color32),
    width: f32,
) -> Option<T> {
    let height = 36.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    ui.painter().rect(
        rect,
        CornerRadius::same(12),
        p.field,
        Stroke::new(1.0, p.inset_border),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(3.0);
    let seg = inner.width() / options.len().max(1) as f32;
    let mut picked = None;
    for (i, (value, label)) in options.iter().enumerate() {
        let r = Rect::from_min_size(
            Pos2::new(inner.left() + seg * i as f32, inner.top()),
            Vec2::new(seg, inner.height()),
        );
        let id = ui
            .id()
            .with(("seg", rect.min.x as i32, rect.min.y as i32, i));
        let resp = ui
            .interact(r, id, Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand);
        let selected = *value == current;
        if selected {
            gradient_rect(ui, r, 9.0, grad.0, grad.1);
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(r, CornerRadius::same(9), alpha(grad.0, 0.10));
        }
        let fg = if selected { Color32::WHITE } else { p.muted };
        let font = if selected {
            semi(12.5)
        } else {
            FontId::proportional(12.5)
        };
        ui.painter()
            .text(r.center(), Align2::CENTER_CENTER, *label, font, fg);
        if resp.clicked() && !selected {
            picked = Some(*value);
        }
    }
    picked
}

/// A slider with a gradient fill and a white thumb.
pub fn gradient_slider(
    ui: &mut Ui,
    p: &Palette,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    grad: (Color32, Color32),
    width: f32,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::click_and_drag());
    let (lo, hi) = (*range.start() as f32, *range.end() as f32);
    let track = Rect::from_center_size(rect.center(), Vec2::new(rect.width() - 16.0, 6.0));
    let mut changed = false;
    if let Some(pos) = resp.interact_pointer_pos() {
        let t = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
        let v = (lo + t * (hi - lo)).round() as u32;
        if v != *value {
            *value = v;
            changed = true;
        }
    }
    if resp.hovered() || resp.has_focus() {
        let step = ui.input(|i| {
            i.key_pressed(egui::Key::ArrowRight) as i32 - i.key_pressed(egui::Key::ArrowLeft) as i32
        });
        if step != 0 && resp.hovered() {
            let v = (*value as i32 + step).clamp(lo as i32, hi as i32) as u32;
            if v != *value {
                *value = v;
                changed = true;
            }
        }
    }
    let t = ((*value as f32 - lo) / (hi - lo).max(1.0)).clamp(0.0, 1.0);
    ui.painter()
        .rect_filled(track, CornerRadius::same(255), p.inset_border);
    let filled = Rect::from_min_max(
        track.min,
        Pos2::new(track.left() + track.width() * t, track.bottom()),
    );
    if filled.width() > 1.0 {
        gradient_rect(ui, filled, 3.0, grad.0, lerp(grad.0, grad.1, t));
    }
    let thumb = Pos2::new(filled.right(), track.center().y);
    let r = if resp.hovered() || resp.dragged() {
        9.5
    } else {
        8.5
    };
    ui.painter().circle_filled(
        thumb + Vec2::new(0.0, 1.5),
        r + 1.0,
        alpha(Color32::BLACK, 0.18),
    );
    ui.painter().circle_filled(thumb, r, Color32::WHITE);
    ui.painter()
        .circle_stroke(thumb, r - 1.5, Stroke::new(3.0, lerp(grad.0, grad.1, t)));
    let mut resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if changed {
        resp.mark_changed();
    }
    resp
}

/// A small mono badge, e.g. "12 Mbit/s".
pub fn badge(ui: &mut Ui, p: &Palette, text: &str, color: Color32) {
    Frame::default()
        .fill(tint(p, color))
        .stroke(Stroke::new(1.0, alpha(color, 0.4)))
        .corner_radius(CornerRadius::same(7))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).font(mono(12.0)).color(ink(p, color)));
        });
}

/// A rounded checkbox square.
fn check_box(ui: &Ui, rect: Rect, on: bool, color: Color32, p: &Palette) {
    if on {
        ui.painter().rect_filled(rect, CornerRadius::same(5), color);
        let a = rect.left_center() + Vec2::new(4.0, 0.0);
        let b = rect.center_bottom() + Vec2::new(-1.0, -4.5);
        let c = rect.right_top() + Vec2::new(-4.0, 4.5);
        ui.painter()
            .line(vec![a, b, c], Stroke::new(2.0, Color32::WHITE));
    } else {
        ui.painter().rect(
            rect,
            CornerRadius::same(5),
            p.glass,
            Stroke::new(1.5, p.faint),
            egui::StrokeKind::Inside,
        );
    }
}

/// A clickable tile: icon square, text, checkbox on the right.
pub fn option_tile(
    ui: &mut Ui,
    p: &Palette,
    icon: &str,
    label: &str,
    value: &mut bool,
    color: Color32,
    width: f32,
) -> Response {
    if is_modern() {
        return switch_row(ui, p, icon, label, value, color, width);
    }
    let text_w = width - 40.0 - 44.0;
    let galley = ui.painter().layout(
        label.to_string(),
        FontId::proportional(13.0),
        p.text,
        text_w.max(40.0),
    );
    let height = (galley.size().y + 22.0).max(46.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let hovered = resp.hovered();
    ui.painter().rect(
        rect,
        CornerRadius::same(12),
        if hovered { tint(p, color) } else { p.inset },
        Stroke::new(
            1.0,
            if *value {
                alpha(color, 0.45)
            } else {
                p.inset_border
            },
        ),
        egui::StrokeKind::Inside,
    );
    let icon_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 10.0, rect.center().y - 13.0),
        Vec2::splat(26.0),
    );
    ui.painter()
        .rect_filled(icon_rect, CornerRadius::same(8), tint(p, color));
    ui.painter().text(
        icon_rect.center(),
        Align2::CENTER_CENTER,
        icon,
        FontId::proportional(13.0),
        ink(p, color),
    );
    ui.painter().galley(
        Pos2::new(
            icon_rect.right() + 10.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        p.text,
    );
    let box_rect = Rect::from_center_size(
        Pos2::new(rect.right() - 22.0, rect.center().y),
        Vec2::splat(18.0),
    );
    check_box(ui, box_rect, *value, color, p);
    let mut resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if resp.clicked() {
        *value = !*value;
        resp.mark_changed();
    }
    resp
}

/// A key label that looks like a keyboard key.
pub fn keycap(ui: &Ui, rect: Rect, text: &str, color: Color32, p: &Palette) {
    let shade = if p.dark {
        alpha(Color32::BLACK, 0.5)
    } else {
        alpha(color, 0.35)
    };
    ui.painter().rect_filled(
        rect.translate(Vec2::new(0.0, 2.5)),
        CornerRadius::same(7),
        shade,
    );
    let face = if p.dark {
        lerp(Color32::from_rgb(0x1a, 0x1f, 0x33), color, 0.18)
    } else {
        Color32::WHITE
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(7),
        face,
        Stroke::new(1.0, alpha(color, 0.55)),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        bold_mono(11.0),
        ink(p, color),
    );
}

fn bold_mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// A shortcut card: description on the left, keycap on the right.
pub fn keycap_card(
    ui: &mut Ui,
    p: &Palette,
    what: &str,
    keys: &str,
    color: Color32,
    width: f32,
) -> Response {
    let key_font = bold_mono(11.0);
    let key_w = (text_size(ui, keys, &key_font).x + 16.0).min(width * 0.55);
    let galley = ui.painter().layout(
        what.to_string(),
        FontId::proportional(12.5),
        p.text,
        (width - key_w - 34.0).max(40.0),
    );
    let height = (galley.size().y + 22.0).max(44.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let pressed = resp.is_pointer_button_down_on();
    ui.painter().rect(
        rect,
        CornerRadius::same(12),
        if resp.hovered() {
            tint(p, color)
        } else {
            p.inset
        },
        Stroke::new(1.0, p.inset_border),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        Pos2::new(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text,
    );
    let k = Rect::from_min_size(
        Pos2::new(
            rect.right() - 12.0 - key_w,
            rect.center().y - 12.0 + if pressed { 2.0 } else { 0.0 },
        ),
        Vec2::new(key_w, 22.0),
    );
    keycap(ui, k, keys, color, p);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// A text field styled like the rest of the window.
pub fn text_field(ui: &mut Ui, p: &Palette, text: &mut String, hint: &str, width: f32) -> Response {
    let _ = p;
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .font(mono(13.0))
            .margin(Margin::symmetric(12, 9))
            .desired_width(width),
    )
}

/// Lays out `n` equal columns of `min` width or more; returns (columns, width).
pub fn grid_columns(available: f32, min: f32, max_cols: usize, gap: f32) -> (usize, f32) {
    let mut cols = max_cols.max(1);
    while cols > 1 && (available - gap * (cols - 1) as f32) / (cols as f32) < min {
        cols -= 1;
    }
    (cols, (available - gap * (cols - 1) as f32) / cols as f32)
}

/// Places items in rows of `cols` items, each `width` wide.
pub fn flow<T>(
    ui: &mut Ui,
    items: impl IntoIterator<Item = T>,
    cols: usize,
    gap: f32,
    mut add: impl FnMut(&mut Ui, T),
) {
    let items: Vec<T> = items.into_iter().collect();
    let mut iter = items.into_iter().peekable();
    while iter.peek().is_some() {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for _ in 0..cols {
                match iter.next() {
                    Some(item) => add(ui, item),
                    None => break,
                }
            }
        });
        ui.add_space(gap - ui.spacing().item_spacing.y);
    }
}

// ---------------------------------------------------------------- modern look

fn paint_switch(ui: &Ui, p: &Palette, rect: Rect, on: bool, color: Color32, id: egui::Id) {
    let t = ui.ctx().animate_bool_with_time(id.with("sw"), on, 0.16);
    let track = lerp(p.field, color, t);
    ui.painter().rect(
        rect,
        CornerRadius::same(255),
        track,
        Stroke::new(1.0, lerp(p.inset_border, color, t)),
        egui::StrokeKind::Inside,
    );
    let x = rect.left() + 11.0 + t * 18.0;
    let c = Pos2::new(x, rect.center().y);
    ui.painter()
        .circle_filled(c + Vec2::new(0.0, 1.0), 8.5, alpha(Color32::BLACK, 0.18));
    ui.painter().circle_filled(c, 8.0, Color32::WHITE);
}

/// A settings row: small icon, label, switch on the right, hairline below.
fn switch_row(
    ui: &mut Ui,
    p: &Palette,
    icon: &str,
    label: &str,
    value: &mut bool,
    color: Color32,
    width: f32,
) -> Response {
    let text_w = width - 30.0 - 58.0;
    let galley = ui.painter().layout(
        label.to_string(),
        FontId::proportional(13.0),
        p.text,
        text_w.max(40.0),
    );
    let height = (galley.size().y + 18.0).max(40.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(10), alpha(color, 0.06));
    }
    ui.painter().text(
        Pos2::new(rect.left() + 13.0, rect.center().y),
        Align2::CENTER_CENTER,
        icon,
        FontId::proportional(13.0),
        ink(p, color),
    );
    ui.painter().galley(
        Pos2::new(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text,
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.inset_border),
    );
    let sw = Rect::from_center_size(
        Pos2::new(rect.right() - 26.0, rect.center().y),
        Vec2::new(40.0, 22.0),
    );
    paint_switch(ui, p, sw, *value, BLUE, resp.id);
    let mut resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if resp.clicked() {
        *value = !*value;
        resp.mark_changed();
    }
    resp
}

/// Canvas for the modern look: two soft aurora glows and a fine dot grid.
pub fn modern_backdrop(painter: &egui::Painter, rect: Rect, p: &Palette, a: Color32, b: Color32) {
    if is_glass() {
        planner_backdrop(painter, rect, p.dark);
        return;
    }
    painter.rect_filled(rect, CornerRadius::ZERO, p.bg);
    let k = if p.dark { 0.22 } else { 0.30 };
    let r = rect.width().max(rect.height()) * 0.42;
    glow(
        painter,
        rect.left_top() + Vec2::new(80.0, 60.0),
        r,
        alpha(a, k),
    );
    glow(
        painter,
        rect.right_bottom() - Vec2::new(80.0, 70.0),
        r,
        alpha(b, k),
    );
    let step = 22.0;
    let mut y = rect.top() + 11.0;
    while y < rect.bottom() {
        let mut x = rect.left() + 11.0;
        while x < rect.right() {
            painter.circle_filled(Pos2::new(x, y), 0.9, p.dots);
            x += step;
        }
        y += step;
    }
}

/// A floating glass island (header groups, ribbon, dock).
pub fn island(p: &Palette, radius: u8, margin: Margin) -> Frame {
    Frame::default()
        .fill(p.glass)
        .stroke(Stroke::new(1.0, p.glass_border))
        .corner_radius(CornerRadius::same(radius))
        .inner_margin(margin)
        .shadow(Shadow {
            offset: [0, 10],
            blur: 28,
            spread: 0,
            color: p.shadow,
        })
}

/// A neutral pill button with optional count badge (header actions).
pub fn pill(ui: &mut Ui, p: &Palette, text: &str, selected: bool, badge: Option<&str>) -> Response {
    let font = semi(12.5);
    let size = text_size(ui, text, &font);
    let badge_w = badge
        .map(|b| text_size(ui, b, &mono(10.5)).x + 10.0)
        .unwrap_or(0.0);
    let extra = if badge.is_some() { badge_w + 6.0 } else { 0.0 };
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(size.x + 26.0 + extra, 30.0), Sense::click());
    let lift = if resp.hovered() { -1.0 } else { 0.0 };
    let r = rect.translate(Vec2::new(0.0, lift));
    let fill = if selected {
        tint(p, BLUE)
    } else if p.dark {
        rgba(28, 32, 48, 220)
    } else {
        Color32::WHITE
    };
    ui.painter().rect(
        r,
        CornerRadius::same(255),
        fill,
        Stroke::new(
            1.0,
            if selected || resp.hovered() {
                alpha(BLUE, 0.4)
            } else {
                p.glass_border
            },
        ),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        Pos2::new(r.left() + 13.0, r.center().y),
        Align2::LEFT_CENTER,
        text,
        font,
        p.text,
    );
    if let Some(b) = badge {
        let br = Rect::from_min_size(
            Pos2::new(r.right() - 11.0 - badge_w, r.center().y - 8.0),
            Vec2::new(badge_w, 16.0),
        );
        ui.painter()
            .rect_filled(br, CornerRadius::same(255), p.field);
        ui.painter()
            .text(br.center(), Align2::CENTER_CENTER, b, mono(10.5), p.muted);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// A small toggle chip (mirror options in the ribbon).
pub fn toggle_chip(
    ui: &mut Ui,
    p: &Palette,
    text: &str,
    on: &mut bool,
    grad: (Color32, Color32),
) -> Response {
    let font = semi(11.5);
    let size = text_size(ui, text, &font);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(size.x + 22.0, 26.0), Sense::click());
    if *on {
        gradient_rect(ui, rect, 13.0, grad.0, grad.1);
    } else {
        ui.painter().rect(
            rect,
            CornerRadius::same(255),
            if resp.hovered() {
                tint(p, grad.0)
            } else {
                p.field
            },
            Stroke::new(1.0, p.inset_border),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        font,
        if *on { Color32::WHITE } else { p.muted },
    );
    let mut resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp
}
