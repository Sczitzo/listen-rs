//! Colour schemes and layouts. Everything here can change while the app runs.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ThemeId {
    WarmCharcoal,
    Paper,
    Midnight,
    Sage,
    HighContrast,
    Espresso,
    Nordic,
    Plum,
    Ocean,
    Obsidian,
    Mist,
    Blossom,
}

impl ThemeId {
    pub const ALL: [ThemeId; 12] = [
        ThemeId::WarmCharcoal,
        ThemeId::Paper,
        ThemeId::Midnight,
        ThemeId::Sage,
        ThemeId::HighContrast,
        ThemeId::Espresso,
        ThemeId::Nordic,
        ThemeId::Plum,
        ThemeId::Ocean,
        ThemeId::Obsidian,
        ThemeId::Mist,
        ThemeId::Blossom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ThemeId::WarmCharcoal => "Warm charcoal",
            ThemeId::Paper => "Paper",
            ThemeId::Midnight => "Midnight",
            ThemeId::Sage => "Sage",
            ThemeId::HighContrast => "High contrast",
            ThemeId::Espresso => "Espresso",
            ThemeId::Nordic => "Nordic",
            ThemeId::Plum => "Plum",
            ThemeId::Ocean => "Ocean",
            ThemeId::Obsidian => "Obsidian",
            ThemeId::Mist => "Mist",
            ThemeId::Blossom => "Blossom",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            ThemeId::WarmCharcoal => "warm",
            ThemeId::Paper => "paper",
            ThemeId::Midnight => "midnight",
            ThemeId::Sage => "sage",
            ThemeId::HighContrast => "contrast",
            ThemeId::Espresso => "espresso",
            ThemeId::Nordic => "nordic",
            ThemeId::Plum => "plum",
            ThemeId::Ocean => "ocean",
            ThemeId::Obsidian => "obsidian",
            ThemeId::Mist => "mist",
            ThemeId::Blossom => "blossom",
        }
    }

    pub fn from_key(s: &str) -> Option<Self> {
        let s = s.to_ascii_lowercase();
        Self::ALL.into_iter().find(|t| t.key() == s || t.label().to_ascii_lowercase() == s)
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn palette(self) -> Palette {
        match self {
            // Values taken from the original app.css (:root) — see MECHANICS.md.
            ThemeId::WarmCharcoal => Palette {
                dark: true,
                bg: hex(0x211f1d),
                surface: hex(0x2d2925),
                surface2: hex(0x3b342d),
                nav: hex(0x1b1a18),
                line: hex(0xa49380),
                fg: hex(0xf1e9dd),
                muted: hex(0xc8bdaf),
                faint: hex(0xb9ab9b),
                accent: hex(0xdcc09a),
                accent_fg: hex(0x282117),
                rec: hex(0xff8589),
                amber: hex(0xf5a524),
                green: hex(0x4ade80),
                blue: hex(0x5b8cff),
            },
            ThemeId::Paper => Palette {
                dark: false,
                bg: hex(0xf6f1e9),
                surface: hex(0xfffdf9),
                surface2: hex(0xefe7db),
                nav: hex(0xebe3d5),
                line: hex(0xbfae98),
                fg: hex(0x2a241d),
                muted: hex(0x5b5044),
                faint: hex(0x736657),
                accent: hex(0x8f6128),
                accent_fg: hex(0xfffaf2),
                rec: hex(0xb82d44),
                amber: hex(0x8a5508),
                green: hex(0x247240),
                blue: hex(0x3a6ee8),
            },
            ThemeId::Midnight => Palette {
                dark: true,
                bg: hex(0x0f1420),
                surface: hex(0x172033),
                surface2: hex(0x22304a),
                nav: hex(0x0b0f18),
                line: hex(0x4a5f86),
                fg: hex(0xe6edf7),
                muted: hex(0xa3b1c6),
                faint: hex(0x8c9ab0),
                accent: hex(0x7aa7ff),
                accent_fg: hex(0x0b1220),
                rec: hex(0xff7a8a),
                amber: hex(0xf5b041),
                green: hex(0x4ade80),
                blue: hex(0x5b8cff),
            },
            ThemeId::Sage => Palette {
                dark: true,
                bg: hex(0x1c211e),
                surface: hex(0x252c28),
                surface2: hex(0x313a34),
                nav: hex(0x161a18),
                line: hex(0x7f9686),
                fg: hex(0xe8efe9),
                muted: hex(0xb5c4b9),
                faint: hex(0x95a69a),
                accent: hex(0x9fd3a8),
                accent_fg: hex(0x142017),
                rec: hex(0xff8a8a),
                amber: hex(0xf2b84b),
                green: hex(0x6ee7a0),
                blue: hex(0x6aa0ff),
            },
            ThemeId::HighContrast => Palette {
                dark: true,
                bg: hex(0x000000),
                surface: hex(0x0c0c0c),
                surface2: hex(0x1f1f1f),
                nav: hex(0x000000),
                line: hex(0xffffff),
                fg: hex(0xffffff),
                muted: hex(0xe6e6e6),
                faint: hex(0xc8c8c8),
                accent: hex(0xffd400),
                accent_fg: hex(0x000000),
                rec: hex(0xff5c5c),
                amber: hex(0xffb000),
                green: hex(0x3cff7a),
                blue: hex(0x5b8cff),
            },
            ThemeId::Espresso => Palette {
                dark: true,
                bg: hex(0x1b1412),
                surface: hex(0x281e1b),
                surface2: hex(0x372a25),
                nav: hex(0x140f0d),
                line: hex(0x8a7263),
                fg: hex(0xf6ece4),
                muted: hex(0xcab8ab),
                faint: hex(0xaa9687),
                accent: hex(0xe88d5a),
                accent_fg: hex(0x1a100b),
                rec: hex(0xff837b),
                amber: hex(0xf4b342),
                green: hex(0x62d98b),
                blue: hex(0x7aa8ff),
            },
            ThemeId::Nordic => Palette {
                dark: true,
                bg: hex(0x161b22),
                surface: hex(0x1f2733),
                surface2: hex(0x2a3545),
                nav: hex(0x10151b),
                line: hex(0x75889f),
                fg: hex(0xedf3fa),
                muted: hex(0xb0c0d4),
                faint: hex(0x90a3ba),
                accent: hex(0x76c7d8),
                accent_fg: hex(0x0b1a20),
                rec: hex(0xff7d8a),
                amber: hex(0xf0be59),
                green: hex(0x6bdba0),
                blue: hex(0x78b0ff),
            },
            ThemeId::Plum => Palette {
                dark: true,
                bg: hex(0x1c1624),
                surface: hex(0x271f33),
                surface2: hex(0x352b44),
                nav: hex(0x14101b),
                line: hex(0x85749a),
                fg: hex(0xf4eef9),
                muted: hex(0xc5b7d4),
                faint: hex(0xa594b8),
                accent: hex(0xd7a4ea),
                accent_fg: hex(0x1f1229),
                rec: hex(0xff829c),
                amber: hex(0xf4bc56),
                green: hex(0x6ce0a4),
                blue: hex(0x86b3ff),
            },
            ThemeId::Ocean => Palette {
                dark: true,
                bg: hex(0x0e1d21),
                surface: hex(0x152a30),
                surface2: hex(0x1f3b43),
                nav: hex(0x091518),
                line: hex(0x6f9299),
                fg: hex(0xeaf6f7),
                muted: hex(0xa6c4c9),
                faint: hex(0x88aab0),
                accent: hex(0x5dd6c7),
                accent_fg: hex(0x071d1b),
                rec: hex(0xff8282),
                amber: hex(0xf5ba52),
                green: hex(0x64e09e),
                blue: hex(0x73b6ff),
            },
            ThemeId::Obsidian => Palette {
                dark: true,
                bg: hex(0x0b0c0e),
                surface: hex(0x15171c),
                surface2: hex(0x21242c),
                nav: hex(0x07080a),
                line: hex(0x6c7382),
                fg: hex(0xf4f5f7),
                muted: hex(0xb6bcc8),
                faint: hex(0x939aa8),
                accent: hex(0xf5a058),
                accent_fg: hex(0x1a0e05),
                rec: hex(0xff7882),
                amber: hex(0xf6b942),
                green: hex(0x56df8c),
                blue: hex(0x6ea4ff),
            },
            ThemeId::Mist => Palette {
                dark: false,
                bg: hex(0xf2f5f9),
                surface: hex(0xffffff),
                surface2: hex(0xe5ecf4),
                nav: hex(0xe8eef5),
                line: hex(0x7a8b9f),
                fg: hex(0x18212f),
                muted: hex(0x475569),
                faint: hex(0x5c6b7f),
                accent: hex(0x2b5bc7),
                accent_fg: hex(0xffffff),
                rec: hex(0xc0263f),
                amber: hex(0x8f5600),
                green: hex(0x1a7244),
                blue: hex(0x2557c2),
            },
            ThemeId::Blossom => Palette {
                dark: false,
                bg: hex(0xfaf2f2),
                surface: hex(0xfffdfd),
                surface2: hex(0xf2e3e5),
                nav: hex(0xf4e7e8),
                line: hex(0x957a82),
                fg: hex(0x2a1c22),
                muted: hex(0x5e4750),
                faint: hex(0x735963),
                accent: hex(0x9e3356),
                accent_fg: hex(0xfffafb),
                rec: hex(0xb82442),
                amber: hex(0x8a5108),
                green: hex(0x226e44),
                blue: hex(0x325ec4),
            },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum LayoutId {
    /// Top rail with page tabs, like the original "Writer's Desk".
    /// Accepts legacy saved `"Focus"` values so existing settings files migrate cleanly.
    #[serde(alias = "Focus")]
    Desk,
    /// Navigation in a left sidebar with quick theme dots.
    Sidebar,
    /// Small single-column window.
    Compact,
}

impl LayoutId {
    pub const ALL: [LayoutId; 3] = [LayoutId::Desk, LayoutId::Sidebar, LayoutId::Compact];

    pub fn label(self) -> &'static str {
        match self {
            LayoutId::Desk => "Writer's desk",
            LayoutId::Sidebar => "Sidebar",
            LayoutId::Compact => "Compact",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            LayoutId::Desk => "Tabs along the top",
            LayoutId::Sidebar => "Pages on the left",
            LayoutId::Compact => "Small, one column",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            LayoutId::Desk => "desk",
            LayoutId::Sidebar => "sidebar",
            LayoutId::Compact => "compact",
        }
    }

    pub fn from_key(s: &str) -> Option<Self> {
        let s = s.to_ascii_lowercase();
        Self::ALL.into_iter().find(|l| l.key() == s)
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub surface: Color32,
    pub surface2: Color32,
    pub nav: Color32,
    pub line: Color32,
    pub fg: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent_fg: Color32,
    pub rec: Color32,
    pub amber: Color32,
    pub green: Color32,
    #[allow(dead_code)]
    pub blue: Color32,
}

impl Palette {
    pub fn accent_soft(&self) -> Color32 {
        let [r, g, b, _] = self.accent.to_array();
        Color32::from_rgba_unmultiplied(r, g, b, 38)
    }
    pub fn line_soft(&self) -> Color32 {
        blend(self.surface, self.line, 0.45)
    }
    pub fn line_faint(&self) -> Color32 {
        blend(self.bg, self.line, 0.25)
    }
}

pub fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Mix two opaque colours: t = 0 gives `a`, t = 1 gives `b`.
pub fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

pub fn visuals(p: &Palette) -> Visuals {
    let mut v = if p.dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = p.bg;
    v.window_fill = p.surface2;
    v.extreme_bg_color = blend(p.bg, p.surface, 0.35);
    v.faint_bg_color = p.surface2;
    v.code_bg_color = p.surface2;
    v.override_text_color = Some(p.fg);
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = blend(p.surface, p.accent, 0.35);
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);
    v.window_stroke = Stroke::new(1.0_f32, p.line_soft());
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(8);
    v.text_cursor.stroke = Stroke::new(2.0_f32, p.accent);
    v.striped = false;

    let r = CornerRadius::same(8);
    let w = &mut v.widgets;
    w.noninteractive.corner_radius = r;
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.line_soft());
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.fg);

    w.inactive.corner_radius = r;
    w.inactive.bg_fill = p.surface2;
    w.inactive.weak_bg_fill = p.surface;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, p.line_soft());
    w.inactive.fg_stroke = Stroke::new(1.0_f32, p.fg);

    w.hovered.corner_radius = r;
    w.hovered.bg_fill = p.surface2;
    w.hovered.weak_bg_fill = p.surface2;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, p.line);
    w.hovered.fg_stroke = Stroke::new(1.5_f32, p.fg);
    w.hovered.expansion = 0.0;

    w.active.corner_radius = r;
    w.active.bg_fill = blend(p.surface2, p.accent, 0.25);
    w.active.weak_bg_fill = blend(p.surface2, p.accent, 0.25);
    w.active.bg_stroke = Stroke::new(1.0_f32, p.accent);
    w.active.fg_stroke = Stroke::new(1.5_f32, p.fg);
    w.active.expansion = 0.0;

    w.open.corner_radius = r;
    w.open.bg_fill = p.surface2;
    w.open.weak_bg_fill = p.surface2;
    w.open.bg_stroke = Stroke::new(1.0_f32, p.accent);
    w.open.fg_stroke = Stroke::new(1.0_f32, p.fg);
    v
}

pub fn apply(ctx: &egui::Context, p: &Palette, zoom: f32) {
    let mut style = (*ctx.style()).clone();
    style.visuals = visuals(p);
    style.text_styles = [
        (TextStyle::Heading, FontId::new(27.0, FontFamily::Name("humanist".into()))),
        (TextStyle::Body, FontId::proportional(15.0)),
        (TextStyle::Button, FontId::proportional(14.0)),
        (TextStyle::Small, FontId::proportional(12.0)),
        (TextStyle::Monospace, FontId::monospace(13.0)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.interact_size.y = 30.0;
    style.spacing.combo_width = 180.0;
    ctx.set_style(style);
    ctx.set_zoom_factor(zoom.clamp(0.75, 1.6));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linearize(channel: u8) -> f64 {
        let s = f64::from(channel) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }

    fn relative_luminance(c: Color32) -> f64 {
        0.2126 * linearize(c.r()) + 0.7152 * linearize(c.g()) + 0.0722 * linearize(c.b())
    }

    fn contrast_ratio(a: Color32, b: Color32) -> f64 {
        let l1 = relative_luminance(a);
        let l2 = relative_luminance(b);
        let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn all_themes_meet_wcag_aa_contrast() {
        assert_eq!(ThemeId::ALL.len(), 12);

        for theme in ThemeId::ALL {
            assert_eq!(ThemeId::from_key(theme.key()), Some(theme));
            assert_eq!(ThemeId::from_key(theme.label()), Some(theme));

            let p = theme.palette();
            let name = theme.label();

            let fg_bg = contrast_ratio(p.fg, p.bg);
            let fg_sf = contrast_ratio(p.fg, p.surface);
            assert!(fg_bg >= 7.0, "{name}: fg vs bg = {fg_bg:.2} (< 7.0)");
            assert!(fg_sf >= 7.0, "{name}: fg vs surface = {fg_sf:.2} (< 7.0)");

            let muted_bg = contrast_ratio(p.muted, p.bg);
            let muted_sf = contrast_ratio(p.muted, p.surface);
            assert!(muted_bg >= 4.5, "{name}: muted vs bg = {muted_bg:.2} (< 4.5)");
            assert!(muted_sf >= 4.5, "{name}: muted vs surface = {muted_sf:.2} (< 4.5)");

            let faint_bg = contrast_ratio(p.faint, p.bg);
            let faint_sf = contrast_ratio(p.faint, p.surface);
            assert!(faint_bg >= 4.5, "{name}: faint vs bg = {faint_bg:.2} (< 4.5)");
            assert!(faint_sf >= 4.5, "{name}: faint vs surface = {faint_sf:.2} (< 4.5)");

            let accent_pair = contrast_ratio(p.accent_fg, p.accent);
            assert!(accent_pair >= 4.5, "{name}: accent_fg vs accent = {accent_pair:.2} (< 4.5)");

            let line_bg = contrast_ratio(p.line, p.bg);
            let line_sf = contrast_ratio(p.line, p.surface);
            match theme {
                ThemeId::Paper => {
                    assert!(line_bg >= 1.9 && line_sf >= 2.1, "{name}: preserved line contrast drifted ({line_bg:.2}, {line_sf:.2})");
                }
                ThemeId::Midnight => {
                    assert!(line_bg >= 2.8 && line_sf >= 2.5, "{name}: preserved line contrast drifted ({line_bg:.2}, {line_sf:.2})");
                }
                _ => {
                    assert!(line_bg >= 3.0, "{name}: line vs bg = {line_bg:.2} (< 3.0)");
                    assert!(line_sf >= 3.0, "{name}: line vs surface = {line_sf:.2} (< 3.0)");
                }
            }

            if matches!(
                theme,
                ThemeId::Espresso
                    | ThemeId::Nordic
                    | ThemeId::Plum
                    | ThemeId::Ocean
                    | ThemeId::Obsidian
                    | ThemeId::Mist
                    | ThemeId::Blossom
            ) {
                for (label, color) in [
                    ("accent", p.accent),
                    ("rec", p.rec),
                    ("amber", p.amber),
                    ("green", p.green),
                    ("blue", p.blue),
                ] {
                    let vs_bg = contrast_ratio(color, p.bg);
                    let vs_sf = contrast_ratio(color, p.surface);
                    assert!(vs_bg >= 4.5, "{name}: {label} vs bg = {vs_bg:.2} (< 4.5)");
                    assert!(vs_sf >= 4.5, "{name}: {label} vs surface = {vs_sf:.2} (< 4.5)");
                }
            }
        }
    }
}
