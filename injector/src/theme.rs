use egui::Color32;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{OnceLock, RwLock};

// ============================================================
// THEME MODE
// ============================================================

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeMode {
    Dark,
    Darker,
    Black,
    Midnight,
    Slate,
    Ash,
    Warm,
}

impl ThemeMode {
    pub fn label(&self) -> &'static str {
        match self {
            ThemeMode::Dark => "Dark",
            ThemeMode::Darker => "Darker",
            ThemeMode::Black => "Black",
            ThemeMode::Midnight => "Midnight",
            ThemeMode::Slate => "Slate",
            ThemeMode::Ash => "Ash",
            ThemeMode::Warm => "Warm",
        }
    }

    pub fn all() -> [ThemeMode; 7] {
        [
            ThemeMode::Dark,
            ThemeMode::Darker,
            ThemeMode::Black,
            ThemeMode::Midnight,
            ThemeMode::Slate,
            ThemeMode::Ash,
            ThemeMode::Warm,
        ]
    }

    fn background(&self) -> (Color32, Color32, Color32) {
        match self {
            ThemeMode::Dark => (
                Color32::from_rgb(15, 17, 22),
                Color32::from_rgb(24, 27, 34),
                Color32::from_rgb(36, 40, 50),
            ),
            ThemeMode::Darker => (
                Color32::from_rgb(8, 9, 12),
                Color32::from_rgb(16, 18, 23),
                Color32::from_rgb(28, 31, 38),
            ),
            ThemeMode::Black => (
                Color32::from_rgb(0, 0, 0),
                Color32::from_rgb(12, 12, 14),
                Color32::from_rgb(24, 24, 28),
            ),
            ThemeMode::Midnight => (
                Color32::from_rgb(10, 12, 24),
                Color32::from_rgb(18, 22, 38),
                Color32::from_rgb(32, 38, 60),
            ),
            ThemeMode::Slate => (
                Color32::from_rgb(40, 44, 52),
                Color32::from_rgb(52, 57, 66),
                Color32::from_rgb(68, 74, 86),
            ),
            ThemeMode::Ash => (
                Color32::from_rgb(60, 62, 68),
                Color32::from_rgb(76, 79, 86),
                Color32::from_rgb(94, 98, 108),
            ),
            ThemeMode::Warm => (
                Color32::from_rgb(58, 54, 48),
                Color32::from_rgb(74, 69, 62),
                Color32::from_rgb(92, 86, 78),
            ),
        }
    }
}

// ============================================================
// DENSITY
// ============================================================

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Density {
    Comfortable,
    Compact,
    Spacious,
}

impl Density {
    pub fn label(&self) -> &'static str {
        match self {
            Density::Comfortable => "Comfortable",
            Density::Compact => "Compact",
            Density::Spacious => "Spacious",
        }
    }

    pub fn all() -> [Density; 3] {
        [Density::Comfortable, Density::Compact, Density::Spacious]
    }

    pub fn metrics(&self) -> (f32, (f32, f32), f32) {
        match self {
            Density::Comfortable => (10.0, (14.0, 8.0), 16.0),
            Density::Compact => (6.0, (10.0, 5.0), 10.0),
            Density::Spacious => (14.0, (18.0, 11.0), 22.0),
        }
    }
}

// ============================================================
// ACCENT
// ============================================================

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Accent {
    Violet,
    Cyan,
    Magenta,
    Green,
    Amber,
    Red,
}

impl Accent {
    pub fn label(&self) -> &'static str {
        match self {
            Accent::Violet => "Violet",
            Accent::Cyan => "Cyan",
            Accent::Magenta => "Magenta",
            Accent::Green => "Green",
            Accent::Amber => "Amber",
            Accent::Red => "Red",
        }
    }

    pub fn all() -> [Accent; 6] {
        [
            Accent::Violet,
            Accent::Cyan,
            Accent::Magenta,
            Accent::Green,
            Accent::Amber,
            Accent::Red,
        ]
    }

    pub fn rgb(&self) -> (u8, u8, u8) {
        match self {
            Accent::Violet => (130, 100, 255),
            Accent::Cyan => (90, 200, 230),
            Accent::Magenta => (230, 100, 200),
            Accent::Green => (95, 210, 145),
            Accent::Amber => (245, 175, 100),
            Accent::Red => (240, 105, 120),
        }
    }
}

// ============================================================
// THEME STRUCT
// ============================================================

#[derive(Clone, Copy)]
pub struct Theme {
    pub mode: ThemeMode,
    pub accent: Accent,
    pub density: Density,
    pub corner_radius: f32,
    pub font_scale: f32,

    pub bg: Color32,
    pub panel: Color32,
    pub panel_hover: Color32,
    pub accent_fill: Color32,
    pub accent_dim: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,

    pub sent: Color32,
    pub recv: Color32,
    pub error: Color32,
    pub warn: Color32,
    pub info: Color32,
    pub debug: Color32,
    pub trace: Color32,
    pub ok: Color32,
    pub blocked: Color32,
}

impl Theme {
    pub fn from_parts(
        mode: ThemeMode,
        accent: Accent,
        density: Density,
        corner_radius: f32,
        font_scale: f32,
    ) -> Self {
        let (bg, panel, panel_hover) = mode.background();
        let (ar, ag, ab) = accent.rgb();
        let accent_fill = Color32::from_rgb(ar, ag, ab);
        let accent_dim = Color32::from_rgb(
            (ar as u16 * 60 / 100) as u8,
            (ag as u16 * 60 / 100) as u8,
            (ab as u16 * 60 / 100) as u8,
        );

        Self {
            mode,
            accent,
            density,
            corner_radius,
            font_scale,
            bg,
            panel,
            panel_hover,
            accent_fill,
            accent_dim,
            text: Color32::from_rgb(235, 238, 245),
            text_dim: Color32::from_rgb(150, 158, 175),
            text_faint: Color32::from_rgb(100, 108, 125),
            sent: Color32::from_rgb(110, 210, 140),
            recv: Color32::from_rgb(110, 170, 240),
            error: Color32::from_rgb(240, 110, 120),
            warn: Color32::from_rgb(245, 200, 110),
            info: Color32::from_rgb(190, 205, 225),
            debug: Color32::from_rgb(150, 158, 175),
            trace: Color32::from_rgb(110, 118, 135),
            ok: Color32::from_rgb(110, 210, 140),
            blocked: Color32::from_rgb(245, 170, 110),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::from_parts(
            ThemeMode::Dark,
            Accent::Violet,
            Density::Comfortable,
            12.0,
            1.0,
        )
    }
}

// ============================================================
// GLOBAL STATE
// ============================================================

static CURRENT: OnceLock<RwLock<Theme>> = OnceLock::new();

fn cell() -> &'static RwLock<Theme> {
    CURRENT.get_or_init(|| RwLock::new(Theme::default()))
}

pub fn install(theme: Theme) {
    if let Ok(mut g) = cell().write() {
        *g = theme;
    }
}

pub fn update(f: impl FnOnce(&mut Theme)) {
    if let Ok(mut g) = cell().write() {
        let mut copy = *g;
        f(&mut copy);
        *g = Theme::from_parts(
            copy.mode,
            copy.accent,
            copy.density,
            copy.corner_radius,
            copy.font_scale,
        );
    }
}

pub fn with<R>(f: impl FnOnce(&Theme) -> R) -> R {
    let guard = cell().read().expect("theme poisoned");
    f(&guard)
}

// ============================================================
// ACCESSORS
// ============================================================

pub fn bg() -> Color32 { with(|t| t.bg) }
pub fn panel() -> Color32 { with(|t| t.panel) }
pub fn panel_hover() -> Color32 { with(|t| t.panel_hover) }
pub fn accent() -> Color32 { with(|t| t.accent_fill) }
pub fn accent_dim() -> Color32 { with(|t| t.accent_dim) }
pub fn text() -> Color32 { with(|t| t.text) }
pub fn text_dim() -> Color32 { with(|t| t.text_dim) }
pub fn text_faint() -> Color32 { with(|t| t.text_faint) }
pub fn sent() -> Color32 { with(|t| t.sent) }
pub fn recv() -> Color32 { with(|t| t.recv) }
pub fn error() -> Color32 { with(|t| t.error) }
pub fn warn() -> Color32 { with(|t| t.warn) }
pub fn info() -> Color32 { with(|t| t.info) }
pub fn debug() -> Color32 { with(|t| t.debug) }
pub fn trace() -> Color32 { with(|t| t.trace) }
pub fn ok() -> Color32 { with(|t| t.ok) }
pub fn blocked() -> Color32 { with(|t| t.blocked) }
pub fn corner() -> f32 { with(|t| t.corner_radius) }
pub fn font_scale() -> f32 { with(|t| t.font_scale) }

/// Multiplies a base font size by the user's font scale.
pub fn sz(base: f32) -> f32 {
    base * font_scale()
}

// ============================================================
// STYLE HASH
// ============================================================

static STYLE_HASH: AtomicU64 = AtomicU64::new(0);

fn style_hash(t: &Theme) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut mix = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x100000001b3);
    };

    mix(t.corner_radius.to_bits() as u64);
    mix(match t.mode {
        ThemeMode::Dark => 1,
        ThemeMode::Darker => 2,
        ThemeMode::Black => 3,
        ThemeMode::Midnight => 4,
        ThemeMode::Slate => 5,
        ThemeMode::Ash => 6,
        ThemeMode::Warm => 7,
    });
    mix(match t.accent {
        Accent::Violet => 1,
        Accent::Cyan => 2,
        Accent::Magenta => 3,
        Accent::Green => 4,
        Accent::Amber => 5,
        Accent::Red => 6,
    });
    mix(match t.density {
        Density::Comfortable => 1,
        Density::Compact => 2,
        Density::Spacious => 3,
    });

    // font_scale is deliberately NOT hashed. Widgets pass their own
    // size through `theme::sz(...)`, so the egui style does not need
    // to change when font_scale changes.
    h
}

// ============================================================
// APPLY TO EGUI
// ============================================================

pub fn apply_theme(ctx: &egui::Context) {
    let (mode, accent, density, corner_radius) = with(|t| {
        (t.mode, t.accent, t.density, t.corner_radius)
    });

    let t = Theme::from_parts(mode, accent, density, corner_radius, 1.0);

    let h = style_hash(&t);
    if STYLE_HASH.swap(h, AtomicOrdering::Relaxed) == h {
        return;
    }

    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(t.text);
    visuals.panel_fill = t.bg;
    visuals.window_fill = t.bg;
    visuals.extreme_bg_color = t.panel;
    visuals.faint_bg_color = t.panel;
    visuals.widgets.noninteractive.bg_fill = t.panel;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, t.text_dim);
    visuals.widgets.inactive.bg_fill = t.panel;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, t.text);
    visuals.widgets.hovered.bg_fill = t.panel_hover;
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, t.text);
    visuals.widgets.active.bg_fill = t.accent_fill;
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, Color32::WHITE);
    visuals.selection.bg_fill = t.accent_fill;
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, Color32::WHITE);
    visuals.window_rounding = egui::Rounding::same(corner_radius);

    let mut style = egui::Style::default();
    let (spacing, padding, margin) = density.metrics();
    style.spacing.item_spacing = egui::vec2(spacing, spacing);
    style.spacing.button_padding = egui::vec2(padding.0, padding.1);
    style.spacing.window_margin = egui::Margin::same(margin);

    style.interaction.tooltip_delay = 0.8;
    style.interaction.show_tooltips_only_when_still = true;

    ctx.set_visuals(visuals);
    ctx.set_style(style);
}

// ============================================================
// SMALL HELPERS
// ============================================================

pub fn card<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::none()
        .fill(panel())
        .rounding(egui::Rounding::same(corner()))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, add_contents)
        .inner
}