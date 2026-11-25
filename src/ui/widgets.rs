//! Shared pure-Rust egui design system widgets for Listen.
//!
//! Matches Listen's visual grammar across all 12 WCAG AA themes:
//! - Rounded bordered cards (`12 px` radius)
//! - Border-inline legend frames (`frame_titled` left-aligned & `hotkey_card` centered)
//! - Crisp pill toggles, unified dropdown combos, stat cards with Painter sparklines,
//!   compact 6×2 bold 3-band theme tiles, and miniature layout thumbnails.

use crate::mark;
use crate::theme::{blend, LayoutId, Palette, ThemeId};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, ComboBox, CornerRadius, FontFamily, FontId, Frame, Margin,
    Pos2, Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};
use std::collections::VecDeque;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Mic,
    Lab,
    Meetings,
    History,
    Settings,
    Power,
    Search,
}

pub fn page_icon(page: crate::app::Page) -> Icon {
    match page {
        crate::app::Page::Dictate => Icon::Mic,
        crate::app::Page::Lab => Icon::Lab,
        crate::app::Page::Meetings => Icon::Meetings,
        crate::app::Page::History => Icon::History,
        crate::app::Page::Settings => Icon::Settings,
    }
}

pub fn draw_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) / 16.0;
    let st = Stroke::new((1.5 * s).clamp(1.2, 2.2), color);
    match icon {
        Icon::Mic => {
            let cap = Rect::from_center_size(pos2(c.x, c.y - 2.2 * s), vec2(4.8 * s, 7.2 * s));
            painter.rect_stroke(cap, CornerRadius::same((2.4 * s).round() as u8), st, StrokeKind::Inside);
            let pts: Vec<Pos2> = (0..=10)
                .map(|i| {
                    let a = std::f32::consts::PI * (i as f32 / 10.0);
                    pos2(c.x - a.cos() * 4.5 * s, c.y - 0.6 * s + a.sin() * 4.2 * s)
                })
                .collect();
            painter.add(Shape::line(pts, st));
            painter.line_segment([pos2(c.x, c.y + 3.6 * s), pos2(c.x, c.y + 6.2 * s)], st);
            painter.line_segment([pos2(c.x - 2.6 * s, c.y + 6.2 * s), pos2(c.x + 2.6 * s, c.y + 6.2 * s)], st);
        }
        Icon::Lab => {
            painter.line_segment([pos2(c.x - 2.4 * s, c.y - 6.0 * s), pos2(c.x + 2.4 * s, c.y - 6.0 * s)], st);
            let body = vec![
                pos2(c.x - 1.4 * s, c.y - 5.8 * s),
                pos2(c.x - 1.4 * s, c.y - 1.4 * s),
                pos2(c.x - 5.2 * s, c.y + 5.2 * s),
                pos2(c.x + 5.2 * s, c.y + 5.2 * s),
                pos2(c.x + 1.4 * s, c.y - 1.4 * s),
                pos2(c.x + 1.4 * s, c.y - 5.8 * s),
            ];
            painter.add(Shape::line(body, st));
        }
        Icon::Meetings => {
            let box_r = Rect::from_center_size(pos2(c.x, c.y + 0.5 * s), vec2(11.0 * s, 9.0 * s));
            painter.rect_stroke(box_r, CornerRadius::same((2.0 * s).round() as u8), st, StrokeKind::Inside);
            painter.line_segment([pos2(box_r.left() + 2.2 * s, c.y - 1.0 * s), pos2(box_r.right() - 2.2 * s, c.y - 1.0 * s)], st);
            painter.line_segment([pos2(box_r.left() + 2.2 * s, c.y + 2.0 * s), pos2(box_r.right() - 4.0 * s, c.y + 2.0 * s)], st);
        }
        Icon::History => {
            painter.circle_stroke(c, 5.8 * s, st);
            painter.line_segment([c, pos2(c.x, c.y - 3.3 * s)], st);
            painter.line_segment([c, pos2(c.x + 2.6 * s, c.y + 1.4 * s)], st);
        }
        Icon::Settings => {
            painter.circle_stroke(c, 2.4 * s, st);
            painter.circle_stroke(c, 5.2 * s, st);
            for i in 0..8 {
                let a = (i as f32) * std::f32::consts::FRAC_PI_4;
                let (sn, cs) = a.sin_cos();
                painter.line_segment(
                    [pos2(c.x + cs * 5.0 * s, c.y + sn * 5.0 * s), pos2(c.x + cs * 6.8 * s, c.y + sn * 6.8 * s)],
                    st,
                );
            }
        }
        Icon::Power => {
            let pts: Vec<Pos2> = (0..=14)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + 0.65 + (i as f32 / 14.0) * (std::f32::consts::TAU - 1.3);
                    pos2(c.x + a.cos() * 5.2 * s, c.y + a.sin() * 5.2 * s)
                })
                .collect();
            painter.add(Shape::line(pts, st));
            painter.line_segment([pos2(c.x, c.y - 6.2 * s), pos2(c.x, c.y - 0.8 * s)], st);
        }
        Icon::Search => {
            painter.circle_stroke(pos2(c.x - 1.2 * s, c.y - 1.2 * s), 4.2 * s, st);
            painter.line_segment([pos2(c.x + 1.9 * s, c.y + 1.9 * s), pos2(c.x + 5.6 * s, c.y + 5.6 * s)], st);
        }
    }
}

/// Draw the Listen 2B Voice-Bars-to-I-Beam brand mark directly on GPU using `mark.rs` geometry.
pub fn brand_mark(ui: &mut Ui, p: &Palette, size: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let painter = ui.painter();
    let rad = (size * 0.22).round() as u8;
    let badge_bg = Color32::from_rgb(0x23, 0x1E, 0x1A);
    let gold = Color32::from_rgb(mark::IDLE[0], mark::IDLE[1], mark::IDLE[2]);
    painter.rect_filled(r, CornerRadius::same(rad), badge_bg);
    painter.rect_stroke(r, CornerRadius::same(rad), Stroke::new(1.0_f32, p.line), StrokeKind::Inside);

    let u = |v: f32| v * (size / 64.0);
    let bar = |x: f32, y: f32, w: f32, h: f32, alpha: f32| {
        let rr = Rect::from_min_size(pos2(r.left() + u(x), r.top() + u(y)), vec2(u(w), u(h)));
        painter.rect_filled(rr, CornerRadius::same((u(w) * 0.5).max(1.0) as u8), gold.gamma_multiply(alpha));
    };
    bar(11.0, 26.0, 4.5, 12.0, 0.55);
    bar(19.0, 19.0, 4.5, 26.0, 0.85);
    bar(27.0, 23.0, 4.5, 18.0, 0.65);
    // Crisp I-beam cursor on right
    bar(42.5, 15.0, 4.5, 34.0, 1.0);
    bar(36.0, 15.0, 17.5, 4.0, 1.0);
    bar(36.0, 45.0, 17.5, 4.0, 1.0);
}

pub fn card_frame(p: &Palette) -> Frame {
    Frame::default()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(16, 14))
}

pub fn hline(ui: &mut Ui, p: &Palette) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0_f32, p.line));
}

pub fn primary_btn(ui: &mut Ui, p: &Palette, text: &str, enabled: bool) -> Response {
    let fill = if enabled { p.accent } else { p.surface2 };
    let fg = if enabled { p.accent_fg } else { p.faint };
    let stroke = if enabled { Stroke::NONE } else { Stroke::new(1.0_f32, p.line) };
    let b = egui::Button::new(RichText::new(text).size(13.5).strong().color(fg))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(8))
        .min_size(vec2(0.0, 34.0));
    ui.add_enabled(enabled, b)
}

pub fn secondary_btn(ui: &mut Ui, p: &Palette, text: &str, enabled: bool) -> Response {
    let fg = if enabled { p.fg } else { p.faint };
    let b = egui::Button::new(RichText::new(text).size(13.0).color(fg))
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(CornerRadius::same(8))
        .min_size(vec2(0.0, 32.0));
    ui.add_enabled(enabled, b)
}

pub fn ghost_btn(ui: &mut Ui, p: &Palette, text: &str, enabled: bool) -> Response {
    let fg = if enabled { p.muted } else { p.faint.gamma_multiply(0.6) };
    let b = egui::Button::new(RichText::new(text).size(13.0).color(fg))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(8))
        .min_size(vec2(0.0, 32.0));
    ui.add_enabled(enabled, b)
}

pub fn danger_btn(ui: &mut Ui, p: &Palette, text: &str, enabled: bool) -> Response {
    let fill = if enabled { blend(p.surface, p.rec, 0.16) } else { p.surface2 };
    let fg = if enabled { p.rec } else { p.faint };
    let b = egui::Button::new(RichText::new(text).size(13.0).color(fg))
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, if enabled { p.rec.gamma_multiply(0.6) } else { p.line }))
        .corner_radius(CornerRadius::same(8))
        .min_size(vec2(0.0, 32.0));
    ui.add_enabled(enabled, b)
}

pub fn badge(ui: &mut Ui, p: &Palette, text: &str, tint: Option<Color32>) {
    let fg = tint.unwrap_or(p.muted);
    let fill = tint.map_or(p.surface2, |c| blend(p.surface, c, 0.16));
    let stroke = tint.map_or(p.line, |c| blend(p.line, c, 0.45));
    let font = FontId::new(11.5, FontFamily::Name("semibold".into()));
    let galley = ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, fg));
    let desired = vec2(galley.size().x + 18.0, (galley.size().y + 6.0).max(20.0));
    let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(99), fill);
    painter.rect_stroke(rect, CornerRadius::same(99), Stroke::new(1.0_f32, stroke), StrokeKind::Inside);
    painter.galley(
        pos2(rect.left() + 9.0, rect.center().y - galley.size().y * 0.5),
        galley,
        fg,
    );
}

/// Crisp On/Off pill switch matching Listen's Settings toggle design.
pub fn pill_toggle(ui: &mut Ui, p: &Palette, value: &mut bool) -> Response {
    let desired = vec2(60.0, 28.0);
    let (rect, mut resp) = ui.allocate_exact_size(desired, Sense::click());
    if resp.clicked() {
        *value = !*value;
        resp.mark_changed();
    }
    let on = *value;
    let bg = if on { p.accent } else { p.surface2 };
    let stroke = if resp.hovered() {
        Stroke::new(1.2_f32, p.accent)
    } else {
        Stroke::new(1.0_f32, if on { p.accent } else { p.line })
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), bg);
    painter.rect_stroke(rect, CornerRadius::same(8), stroke, StrokeKind::Inside);

    let knob_w = 20.0;
    let knob_h = 20.0;
    let pad = 4.0;
    let knob_x = if on { rect.right() - pad - knob_w } else { rect.left() + pad };
    let knob = Rect::from_min_size(pos2(knob_x, rect.top() + pad), vec2(knob_w, knob_h));
    let knob_fill = if on { p.accent_fg } else { p.fg };
    painter.rect_filled(knob, CornerRadius::same(5), knob_fill);

    let lbl = if on { "On" } else { "Off" };
    let lbl_color = if on { p.accent_fg } else { p.muted };
    let lbl_pos = if on {
        pos2(rect.left() + 14.0, rect.center().y)
    } else {
        pos2(rect.right() - 16.0, rect.center().y)
    };
    painter.text(lbl_pos, Align2::CENTER_CENTER, lbl, FontId::proportional(11.5), lbl_color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Bordered card whose title cuts cleanly through the top border like `<fieldset><legend>`.
/// Seamlessly masks the border with `p.bg` above and `p.surface` below.
pub fn frame_titled(
    ui: &mut Ui,
    p: &Palette,
    title: &str,
    badge_text: Option<&str>,
    focused: bool,
    add: impl FnOnce(&mut Ui),
) {
    ui.add_space(6.0);
    let border = if focused { p.accent } else { p.line };
    let bg_idx = ui.painter().add(Shape::Noop);
    let avail = ui.available_width();
    let inner = Frame::default()
        .inner_margin(Margin { left: 16, right: 16, top: 18, bottom: 14 })
        .show(ui, |ui| {
            let content_w = (avail - 32.0).max(120.0);
            ui.set_width(content_w);
            ui.set_max_width(content_w);
            add(ui);
        });
    let rect = inner.response.rect;
    let painter = ui.painter();
    painter.set(
        bg_idx,
        Shape::Vec(vec![
            Shape::rect_filled(rect, CornerRadius::same(12), p.surface),
            Shape::rect_stroke(rect, CornerRadius::same(12), Stroke::new(1.0_f32, border), StrokeKind::Inside),
        ]),
    );
    let font = FontId::new(15.5, FontFamily::Name("humanist".into()));
    let galley = painter.layout_no_wrap(title.to_owned(), font, p.fg);
    let tl = pos2(rect.left() + 14.0, rect.top() - galley.size().y * 0.5);
    let mask_w = galley.size().x + 12.0;
    // Two-tone mask so top half matches page bg and bottom half matches card surface
    let top_mask = Rect::from_min_size(pos2(tl.x - 6.0, rect.top() - galley.size().y * 0.55), vec2(mask_w, galley.size().y * 0.55));
    let bot_mask = Rect::from_min_size(pos2(tl.x - 6.0, rect.top()), vec2(mask_w, galley.size().y * 0.55));
    painter.rect_filled(top_mask, CornerRadius::ZERO, p.bg);
    painter.rect_filled(bot_mask, CornerRadius::ZERO, p.surface);
    painter.galley(tl, galley, p.fg);

    if let Some(b) = badge_text {
        let bf = FontId::proportional(11.5);
        let bgal = painter.layout_no_wrap(b.to_owned(), bf, p.muted);
        let bx = tl.x + mask_w + 2.0;
        let b_rect = Rect::from_min_size(
            pos2(bx, rect.top() - (bgal.size().y + 6.0) * 0.5),
            vec2(bgal.size().x + 14.0, bgal.size().y + 6.0),
        );
        painter.rect_filled(b_rect, CornerRadius::same(99), p.surface2);
        painter.rect_stroke(b_rect, CornerRadius::same(99), Stroke::new(1.0_f32, p.line), StrokeKind::Inside);
        painter.galley(pos2(b_rect.left() + 7.0, b_rect.top() + 3.0), bgal, p.muted);
    }
}

/// Interactive Dictation Hotkey card with **"Hotkey" centered inline with the top border**
/// and individual rounded keycap pills inside (e.g. `[Ctrl] + [→]` or `[Right Ctrl]`).
pub fn hotkey_card(ui: &mut Ui, p: &Palette, parts: &[String], active: bool) -> Response {
    let display_parts: Vec<&str> = if parts.is_empty() {
        vec!["Right Ctrl"]
    } else {
        parts.iter().map(String::as_str).collect()
    };

    // Measure keycaps width so card hugs content cleanly
    let key_font = FontId::new(16.0, FontFamily::Proportional);
    let plus_font = FontId::proportional(13.0);
    let mut inner_w = 0.0_f32;
    let mut cap_widths = Vec::with_capacity(display_parts.len());
    for (idx, part) in display_parts.iter().enumerate() {
        let tw = ui.fonts(|f| f.layout_no_wrap((*part).to_owned(), key_font.clone(), p.fg).size().x);
        let cw = (tw + 18.0).max(38.0);
        cap_widths.push(cw);
        inner_w += cw;
        if idx + 1 < display_parts.len() {
            inner_w += 20.0; // space for '+'
        }
    }
    let card_w = (inner_w + 28.0).clamp(112.0, 240.0);
    let card_h = 68.0;

    let (outer, resp) = ui.allocate_exact_size(vec2(card_w, card_h + 7.0), Sense::click());
    let rect = Rect::from_min_size(pos2(outer.left(), outer.top() + 7.0), vec2(card_w, card_h));
    let fill = if resp.hovered() { p.surface2 } else { p.surface };
    let border = if active || resp.hovered() { p.accent } else { p.line };

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(12), fill);
    painter.rect_stroke(rect, CornerRadius::same(12), Stroke::new(1.0_f32, border), StrokeKind::Inside);

    // Centered border-inline "Hotkey" label with two-tone seamless seam mask
    let title_font = FontId::proportional(12.0);
    let title_col = if resp.hovered() { p.fg } else { p.muted };
    let title_gal = painter.layout_no_wrap("Hotkey".into(), title_font, title_col);
    let tw = title_gal.size().x;
    let th = title_gal.size().y;
    let cx = rect.center().x;
    let mask_rect_top = Rect::from_min_size(pos2(cx - tw * 0.5 - 7.0, rect.top() - th * 0.55), vec2(tw + 14.0, th * 0.55));
    let mask_rect_bot = Rect::from_min_size(pos2(cx - tw * 0.5 - 7.0, rect.top()), vec2(tw + 14.0, th * 0.55));
    painter.rect_filled(mask_rect_top, CornerRadius::ZERO, p.bg);
    painter.rect_filled(mask_rect_bot, CornerRadius::ZERO, fill);
    painter.galley(pos2(cx - tw * 0.5, rect.top() - th * 0.5), title_gal, title_col);

    // Draw centered keycaps
    let mut x = rect.center().x - inner_w * 0.5;
    let cy = rect.center().y + 3.0;
    let cap_h = 34.0;
    for (idx, part) in display_parts.iter().enumerate() {
        let cw = cap_widths[idx];
        let cap = Rect::from_min_size(pos2(x, cy - cap_h * 0.5), vec2(cw, cap_h));
        painter.rect_filled(cap, CornerRadius::same(7), p.bg);
        painter.rect_stroke(cap, CornerRadius::same(7), Stroke::new(1.0_f32, p.line), StrokeKind::Inside);
        // Subtle tactile bottom keycap edge
        painter.line_segment(
            [pos2(cap.left() + 4.0, cap.bottom() - 1.0), pos2(cap.right() - 4.0, cap.bottom() - 1.0)],
            Stroke::new(2.0_f32, p.line),
        );
        painter.text(cap.center(), Align2::CENTER_CENTER, *part, key_font.clone(), p.fg);
        x += cw;
        if idx + 1 < display_parts.len() {
            painter.text(pos2(x + 10.0, cy), Align2::CENTER_CENTER, "+", plus_font.clone(), p.muted);
            x += 20.0;
        }
    }

    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Click to change dictation hotkey")
}

/// Animated 5-bar waveform instrument (idle dots -> live bars while listening).
pub fn quiet_instrument(ui: &mut Ui, p: &Palette, recording: bool, t: f64) {
    let (r, _) = ui.allocate_exact_size(vec2(30.0, 26.0), Sense::hover());
    let painter = ui.painter();
    let heights = [0.45_f32, 0.85, 1.0, 0.72, 0.4];
    for (i, &base) in heights.iter().enumerate() {
        let x = r.left() + 3.0 + (i as f32) * 6.0;
        if recording {
            let wave = ((t * 5.2 + i as f64 * 0.9).sin() as f32 * 0.35 + 0.65).clamp(0.25, 1.0);
            let h = (20.0 * base * wave).max(4.0);
            let rr = Rect::from_center_size(pos2(x, r.center().y), vec2(3.5, h));
            painter.rect_filled(rr, CornerRadius::same(2), p.accent);
        } else {
            painter.circle_filled(pos2(x, r.center().y), 2.0, p.faint);
        }
    }
}

/// Live audio meter bars on the right side of the Dictate hero.
pub fn hero_bars(ui: &mut Ui, p: &Palette, levels: &VecDeque<f32>, active: bool, t: f64) {
    let n = 16;
    let (r, _) = ui.allocate_exact_size(vec2(n as f32 * 5.0, 24.0), Sense::hover());
    if !active {
        return;
    }
    let painter = ui.painter();
    for i in 0..n {
        let lvl = levels.get(levels.len().saturating_sub(n) + i).copied().unwrap_or(0.0);
        let fallback = ((t * 4.5 + i as f64 * 0.55).sin() as f32 * 0.25 + 0.35).clamp(0.15, 0.7);
        let v = lvl.max(fallback * 0.5).clamp(0.12, 1.0);
        let h = (v * 22.0).max(4.0);
        let bar = Rect::from_center_size(pos2(r.left() + i as f32 * 5.0 + 2.0, r.center().y), vec2(2.6, h));
        painter.rect_filled(bar, CornerRadius::same(1), p.accent.gamma_multiply(0.8));
    }
}

/// Compact bold 3-band color swatch tile for the 6×2 Appearance theme grid.
pub fn theme_tile_3band(ui: &mut Ui, cur: &Palette, theme: ThemeId, selected: bool, width: f32) -> Response {
    let pal = theme.palette();
    let h = 64.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, h), Sense::click());
    let painter = ui.painter();
    let fill = if selected {
        blend(cur.surface, cur.accent, 0.14)
    } else if resp.hovered() {
        cur.surface2
    } else {
        cur.surface
    };
    let border = if selected {
        Stroke::new(1.8_f32, cur.accent)
    } else if resp.hovered() {
        Stroke::new(1.0_f32, cur.accent.gamma_multiply(0.7))
    } else {
        Stroke::new(1.0_f32, cur.line)
    };
    painter.rect_filled(rect, CornerRadius::same(10), fill);
    painter.rect_stroke(rect, CornerRadius::same(10), border, StrokeKind::Inside);

    // 3 bold horizontal swatch segments (bg, accent, fg)
    let pad = 8.0;
    let gap = 4.0;
    let sw_w = ((width - pad * 2.0 - gap * 2.0) / 3.0).max(12.0);
    let sw_h = 24.0;
    for (i, col) in [pal.bg, pal.accent, pal.fg].iter().enumerate() {
        let sx = rect.left() + pad + i as f32 * (sw_w + gap);
        let sr = Rect::from_min_size(pos2(sx, rect.top() + pad), vec2(sw_w, sw_h));
        painter.rect_filled(sr, CornerRadius::same(5), *col);
        painter.rect_stroke(sr, CornerRadius::same(5), Stroke::new(1.0_f32, cur.line.gamma_multiply(0.65)), StrokeKind::Inside);
    }

    let lbl_col = if selected { cur.fg } else { cur.muted };
    painter.text(
        pos2(rect.left() + pad, rect.bottom() - 14.0),
        Align2::LEFT_CENTER,
        theme.label(),
        FontId::proportional(12.0),
        lbl_col,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Miniature layout schematic thumbnail tile (`Writer's desk`, `Sidebar`, `Compact`).
pub fn layout_tile(ui: &mut Ui, p: &Palette, layout: LayoutId, selected: bool, width: f32) -> Response {
    let h = 112.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, h), Sense::click());
    let painter = ui.painter();
    let fill = if selected { blend(p.surface, p.accent, 0.12) } else if resp.hovered() { p.surface2 } else { p.surface };
    let border = if selected { Stroke::new(1.8_f32, p.accent) } else { Stroke::new(1.0_f32, p.line) };
    painter.rect_filled(rect, CornerRadius::same(10), fill);
    painter.rect_stroke(rect, CornerRadius::same(10), border, StrokeKind::Inside);

    // Miniature window schematic
    let preview = Rect::from_min_size(pos2(rect.left() + 12.0, rect.top() + 10.0), vec2(width - 24.0, 52.0));
    painter.rect_filled(preview, CornerRadius::same(6), p.bg);
    painter.rect_stroke(preview, CornerRadius::same(6), Stroke::new(1.0_f32, p.line), StrokeKind::Inside);

    match layout {
        LayoutId::Desk => {
            painter.rect_filled(Rect::from_min_size(pos2(preview.left() + 6.0, preview.top() + 6.0), vec2(18.0, 5.0)), CornerRadius::same(2), p.accent);
            for i in 0..3 {
                painter.rect_filled(Rect::from_min_size(pos2(preview.left() + 28.0 + i as f32 * 15.0, preview.top() + 6.0), vec2(11.0, 4.0)), CornerRadius::same(2), p.surface2);
            }
            painter.rect_filled(Rect::from_min_size(pos2(preview.left() + 8.0, preview.top() + 18.0), vec2(preview.width() - 16.0, 12.0)), CornerRadius::same(3), p.surface);
            painter.rect_filled(Rect::from_min_size(pos2(preview.left() + 8.0, preview.top() + 34.0), vec2(preview.width() - 16.0, 10.0)), CornerRadius::same(3), p.surface);
        }
        LayoutId::Sidebar => {
            let rail = Rect::from_min_size(pos2(preview.left() + 4.0, preview.top() + 4.0), vec2(24.0, preview.height() - 8.0));
            painter.rect_filled(rail, CornerRadius::same(4), p.surface);
            painter.rect_filled(Rect::from_min_size(pos2(rail.left() + 4.0, rail.top() + 5.0), vec2(16.0, 5.0)), CornerRadius::same(2), p.accent);
            painter.rect_filled(Rect::from_min_size(pos2(rail.left() + 4.0, rail.top() + 14.0), vec2(14.0, 4.0)), CornerRadius::same(2), p.surface2);
            painter.rect_filled(Rect::from_min_size(pos2(preview.left() + 34.0, preview.top() + 10.0), vec2(preview.width() - 42.0, 16.0)), CornerRadius::same(3), p.surface);
            painter.rect_filled(Rect::from_min_size(pos2(preview.left() + 34.0, preview.top() + 30.0), vec2(preview.width() - 42.0, 14.0)), CornerRadius::same(3), p.surface);
        }
        LayoutId::Compact => {
            let col = Rect::from_center_size(preview.center(), vec2(44.0, preview.height() - 8.0));
            painter.rect_filled(col, CornerRadius::same(4), p.surface);
            painter.rect_filled(Rect::from_min_size(pos2(col.left() + 5.0, col.top() + 5.0), vec2(14.0, 4.0)), CornerRadius::same(2), p.accent);
            painter.rect_filled(Rect::from_min_size(pos2(col.left() + 5.0, col.top() + 14.0), vec2(34.0, 11.0)), CornerRadius::same(2), p.surface2);
            painter.rect_filled(Rect::from_min_size(pos2(col.left() + 5.0, col.top() + 28.0), vec2(34.0, 10.0)), CornerRadius::same(2), p.surface2);
        }
    }

    painter.text(
        pos2(rect.left() + 12.0, rect.bottom() - 32.0),
        Align2::LEFT_CENTER,
        layout.label(),
        FontId::proportional(13.0),
        p.fg,
    );
    painter.text(
        pos2(rect.left() + 12.0, rect.bottom() - 15.0),
        Align2::LEFT_CENTER,
        layout.blurb(),
        FontId::proportional(11.0),
        p.muted,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Miniature trend sparkline rendered natively via `egui::Painter` with hover tooltip.
pub fn sparkline(ui: &mut Ui, p: &Palette, values: &[Option<f64>], size: Vec2) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let nums: Vec<(usize, f64)> = values.iter().enumerate().filter_map(|(i, v)| v.map(|x| (i, x))).collect();
    if nums.len() < 2 {
        return;
    }
    let min_v = nums.iter().map(|(_, v)| *v).fold(f64::INFINITY, f64::min);
    let max_v = nums.iter().map(|(_, v)| *v).fold(f64::NEG_INFINITY, f64::max);
    let span = (max_v - min_v).max(1.0);
    let n = (values.len().saturating_sub(1)).max(1) as f32;
    let pts: Vec<Pos2> = nums
        .iter()
        .map(|(i, v)| {
            let x = rect.left() + 3.0 + (*i as f32 / n) * (rect.width() - 6.0);
            let norm = ((*v - min_v) / span) as f32;
            let y = rect.bottom() - 3.0 - norm * (rect.height() - 6.0);
            pos2(x, y)
        })
        .collect();
    ui.painter().add(Shape::line(pts.clone(), Stroke::new(1.6_f32, p.accent)));
    for (idx, &pt) in pts.iter().enumerate() {
        let is_last = idx + 1 == pts.len();
        ui.painter().circle_filled(pt, if is_last { 2.8 } else { 1.8 }, p.accent.gamma_multiply(if is_last { 1.0 } else { 0.65 }));
    }
    if resp.hovered() {
        let labels = ["3 wks ago", "2 wks ago", "Last wk", "This wk"];
        let summary: Vec<String> = nums
            .iter()
            .map(|(i, v)| {
                let lbl = labels.get(*i).copied().unwrap_or("Period");
                if (*v - v.round()).abs() < 0.05 {
                    format!("{lbl}: {:.0}", v)
                } else {
                    format!("{lbl}: {:.1}", v)
                }
            })
            .collect();
        resp.on_hover_text(summary.join("  ·  "));
    }
}

/// Crisp GPU-painted disclosure triangle + label (never falls back to a missing Unicode glyph box).
pub fn disclosure_btn(
    ui: &mut Ui,
    p: &Palette,
    label: &str,
    open: bool,
    size: f32,
    strong: bool,
    color: Color32,
) -> Response {
    let font = if strong {
        FontId::new(size, FontFamily::Name("semibold".into()))
    } else {
        FontId::proportional(size)
    };
    let galley = ui.fonts(|f| f.layout_no_wrap(label.to_owned(), font, color));
    let total_w = 16.0 + galley.size().x + 4.0;
    let total_h = galley.size().y.max(22.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(total_w, total_h), Sense::click());
    let painter = ui.painter();
    let cx = rect.left() + 6.0;
    let cy = rect.center().y;
    let tri_color = if resp.hovered() { p.accent } else { color };
    let pts = if open {
        vec![pos2(cx - 4.0, cy - 2.0), pos2(cx + 4.0, cy - 2.0), pos2(cx, cy + 3.0)]
    } else {
        vec![pos2(cx - 2.5, cy - 4.0), pos2(cx + 3.0, cy), pos2(cx - 2.5, cy + 4.0)]
    };
    painter.add(Shape::convex_polygon(pts, tri_color, Stroke::NONE));
    painter.galley(pos2(rect.left() + 16.0, rect.center().y - galley.size().y * 0.5), galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Consistent unified dropdown combo box helper matching all Settings selects.
pub fn styled_combo<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: &str,
    current: &mut T,
    selected_text: &str,
    width: f32,
    options: &[(T, &str)],
) -> bool {
    let mut changed = false;
    ComboBox::from_id_salt(id)
        .width(width)
        .selected_text(selected_text)
        .show_ui(ui, |ui| {
            for &(val, label) in options {
                if ui.selectable_value(current, val, label).changed() {
                    changed = true;
                }
            }
        });
    changed
}
