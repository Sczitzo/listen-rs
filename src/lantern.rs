//! Compact recording card shared by the floating window and its sample capture.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, FontFamily, FontId, Rect, Stroke, StrokeKind,
};

pub const WIDTH: f32 = 380.0;
pub const SAMPLE_TEXT: &str = "A short synthetic thought. Synthetic thought 0: keep every earlier word.\nSynthetic thought 1: keep every earlier word.\nSynthetic thought 2: keep every earlier word.";
const HEADER: f32 = 96.0;
const FOOTER: f32 = 48.0;
const MARGIN: f32 = 16.0;
const BG: Color32 = Color32::from_rgb(45, 41, 37);
const BORDER: Color32 = Color32::from_rgb(164, 147, 128);
const TEXT: Color32 = Color32::from_rgb(241, 233, 221);
const MUTED: Color32 = Color32::from_rgb(200, 189, 175);
const ACCENT: Color32 = Color32::from_rgb(220, 192, 154);

pub struct Card<'a> {
    pub generation: u64,
    pub recording: bool,
    pub caption: &'a str,
    pub text: &'a str,
    pub status: &'a str,
}

#[derive(Default)]
pub struct State {
    generation: Option<u64>,
    levels: [f32; 5],
    energy: f32,
    last_frame: Option<f64>,
    height: f32,
    #[cfg(windows)]
    shadow: native_shadow::Shadow,
}

impl State {
    #[cfg(windows)]
    pub fn native_check(&self) -> bool {
        self.shadow.visible_and_inactive()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn update(&mut self, card: &Card<'_>, t: f64, level: f32, animated: bool) {
        if self.generation != Some(card.generation) {
            *self = Self {
                generation: Some(card.generation),
                ..Self::default()
            };
        }
        if !card.recording || !animated {
            self.energy = 0.0;
            self.levels = [0.0; 5];
            self.last_frame = None;
            return;
        }
        self.levels.rotate_left(1);
        self.levels[4] = if level.is_finite() {
            level.max(0.0)
        } else {
            0.0
        };
        let elapsed = self
            .last_frame
            .replace(t)
            .map_or(0.04, |last| (t - last).clamp(0.0, 0.1)) as f32;
        self.energy = smooth_voice(self.energy, &self.levels, elapsed);
    }

    pub fn size(&mut self, ctx: &egui::Context, work: egui::Vec2, card: &Card<'_>) -> egui::Vec2 {
        let width = WIDTH.min((work.x - MARGIN * 2.0).max(1.0));
        let available = (work.y - MARGIN * 2.0).max(1.0);
        let header = if card.recording { HEADER } else { 0.0 };
        let height = if card.text.is_empty() {
            (header + FOOTER).min(available)
        } else {
            let max_width = (width - 38.0).max(1.0);
            let text_height = ctx.fonts(|fonts| {
                fonts
                    .layout(card.text.to_owned(), transcript_font(), TEXT, max_width)
                    .size()
                    .y
            });
            fitted_height(
                text_height,
                available,
                if card.recording { self.height } else { 0.0 },
                header,
            )
        };
        self.height = height;
        vec2(width, height)
    }

    pub fn draw(&self, ctx: &egui::Context, card: &Card<'_>, t: f64, demo: bool) -> bool {
        let mut cancel = false;
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
            let rect = ui.max_rect().shrink(1.0);
            let painter = ui.painter().clone();
            painter.rect_filled(rect, 24.0, BG);
            painter.rect_stroke(rect, 24.0, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);

            let header = if card.recording { HEADER } else { 0.0 };
            let footer_top = rect.bottom() - FOOTER;
            if card.recording {
                let center = rect.left_top() + vec2(14.0 + 42.0, 6.0 + 42.0);
                draw_voice(&painter, center, self.energy, t as f32);
                let x = center.x + 54.0;
                painter.text(pos2(x, center.y - 6.0), Align2::LEFT_BOTTOM, "Listening", FontId::new(16.0, FontFamily::Name("humanist".into())), TEXT);
                painter.text(pos2(x, center.y - 2.0), Align2::LEFT_TOP, card.caption, FontId::proportional(13.0), MUTED);
            }
            if !card.text.is_empty() && footer_top > rect.top() + header + 10.0 {
                painter.hline(rect.x_range(), footer_top, Stroke::new(1.0_f32, BORDER));
                let body = Rect::from_min_max(rect.left_top() + vec2(16.0, header + 6.0), pos2(rect.right() - 16.0, footer_top - 8.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
                    ui.set_clip_rect(body);
                    ui.set_height(body.height());
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.spacing_mut().scroll.bar_width = 4.0;
                    egui::ScrollArea::vertical()
                        .id_salt(("lantern-text", card.generation))
                        .max_height(body.height())
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(egui::RichText::new(card.text).font(transcript_font()).color(TEXT)).wrap().selectable(false));
                        });
                });
            }

            let footer_y = rect.bottom() - 24.0;
            painter.text(pos2(rect.left() + 14.0, footer_y), Align2::LEFT_CENTER, "Listen", FontId::new(13.0, FontFamily::Name("humanist".into())), MUTED);
            if card.recording {
                let button_rect = Rect::from_min_size(pos2(rect.right() - 80.0, footer_y - 14.0), vec2(66.0, 28.0));
                let button = ui.put(button_rect, egui::Button::new(egui::RichText::new("Cancel").size(13.0).color(TEXT)).frame(false));
                if !demo {
                    let (pressed_here, released) = ui.input(|i| {
                        let pressed = i.events.iter().any(|event| matches!(event,
                            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. }
                                if button_rect.contains(*pos)));
                        (pressed, i.pointer.button_released(egui::PointerButton::Primary))
                    });
                    let key = button.id.with("recording-at-press");
                    let clicked = button.clicked_by(egui::PointerButton::Primary);
                    cancel = ctx.data_mut(|data| {
                        if pressed_here { data.insert_temp(key, card.generation); }
                        let matched = clicked && data.get_temp::<u64>(key) == Some(card.generation);
                        if released { data.remove::<u64>(key); }
                        matched
                    });
                }
            } else {
                painter.text(pos2(rect.right() - 14.0, footer_y), Align2::RIGHT_CENTER, card.status, FontId::proportional(13.0), MUTED);
            }
        });
        cancel
    }

    pub fn show_without_focus(&mut self, _title: &str, _anchor: isize, _scale: f32) -> bool {
        #[cfg(windows)]
        {
            if let Some(hwnd) = protected_window(_title, _anchor, _scale) {
                self.shadow.update(hwnd);
                return true;
            }
            self.shadow = native_shadow::Shadow::default();
        }
        false
    }
}

fn fitted_height(text: f32, available: f32, previous: f32, header: f32) -> f32 {
    let limit = 520.0_f32.min((available * 0.55).floor().max(1.0));
    (text.ceil() + 62.0 + header)
        .max(90.0)
        .max(previous)
        .min(limit)
}

fn transcript_font() -> FontId {
    FontId::new(16.0, FontFamily::Name("lantern-transcript".into()))
}

fn smooth_voice(energy: f32, recent: &[f32], elapsed: f32) -> f32 {
    let peak = recent
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0_f32, f32::max);
    let target = (peak * 6.0).min(1.0).sqrt();
    let response = if target > energy { 0.055 } else { 0.14 };
    let next = energy + (target - energy) * (1.0 - (-elapsed / response).exp());
    if next < 0.001 {
        0.0
    } else {
        next
    }
}

fn draw_voice(painter: &egui::Painter, center: egui::Pos2, energy: f32, t: f32) {
    let points = (0..=48)
        .map(|step| {
            let angle = step as f32 / 48.0 * std::f32::consts::TAU;
            let wobble = energy
                * 0.75
                * (2.3 * (3.0 * angle + t * 4.4).sin() + 1.1 * (5.0 * angle - t * 3.2).sin());
            let radius = 31.0 + energy * 1.3 + wobble;
            center + vec2(angle.cos(), angle.sin()) * radius
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.8_f32, ACCENT)));
    for (i, size) in [6.0, 11.0, 15.0, 11.0, 6.0].into_iter().enumerate() {
        let x = center.x - 14.0 + i as f32 * 7.0;
        let half = 2.0 + energy * size * (0.85 + 0.15 * (t * 12.0 - i as f32 * 0.8).sin());
        painter.rect_filled(
            Rect::from_center_size(pos2(x, center.y), vec2(4.0, 2.0 * half + 4.0)),
            2.0,
            ACCENT,
        );
    }
}

pub fn work_area_size(ctx: &egui::Context, _anchor: isize) -> egui::Vec2 {
    #[cfg(windows)]
    if let Some(rect) = native_work_area(_anchor) {
        let scale = ctx.pixels_per_point();
        return vec2((rect.right - rect.left) as f32 / scale, (rect.bottom - rect.top) as f32 / scale);
    }
    ctx.input(|i| i.viewport().monitor_size)
        .unwrap_or(vec2(1920.0, 1080.0))
}

#[cfg(windows)]
fn native_work_area(anchor: isize) -> Option<windows_sys::Win32::Foundation::RECT> {
    use windows_sys::Win32::{
        Foundation::{HWND, POINT},
        Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO,
            MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
        },
        UI::WindowsAndMessaging::{GetForegroundWindow, IsWindow},
    };
    unsafe {
        let anchor = anchor as HWND;
        let window = if IsWindow(anchor) != 0 { anchor } else { GetForegroundWindow() };
        let monitor = if window.is_null() {
            MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY)
        } else {
            MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST)
        };
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        (!monitor.is_null() && GetMonitorInfoW(monitor, &mut info) != 0).then_some(info.rcWork)
    }
}

pub fn animations_enabled() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,
        };
        let mut enabled = 0_i32;
        return unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&mut enabled as *mut i32).cast(),
                0,
            ) != 0
                && enabled != 0
        };
    }
    #[cfg(not(windows))]
    false
}

#[cfg(windows)]
fn protected_window(title: &str, anchor: isize, scale: f32) -> Option<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        System::Threading::GetCurrentThreadId,
        UI::WindowsAndMessaging::*,
    };

    fn property() -> *const u16 {
        windows_sys::w!("ListenRsLanternOriginalWndProc")
    }

    unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_MOUSEACTIVATE {
            return MA_NOACTIVATE as LRESULT;
        }
        let previous = GetPropW(hwnd, property());
        if previous.is_null() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        if msg == WM_NCDESTROY {
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, previous as isize);
            RemovePropW(hwnd, property());
        }
        let previous: WNDPROC = std::mem::transmute(previous);
        CallWindowProcW(previous, hwnd, msg, wp, lp)
    }

    let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        let mut owner = 0;
        if hwnd.is_null()
            || GetWindowThreadProcessId(hwnd, &mut owner) != GetCurrentThreadId()
            || owner != std::process::id()
        {
            return None;
        }
        if GetPropW(hwnd, property()).is_null() {
            let old_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let style = (old_style | (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW) as isize)
                & !(WS_EX_APPWINDOW as isize);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
            if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) != style {
                return None;
            }
            let previous = GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
            if previous == 0 || SetPropW(hwnd, property(), previous as _) == 0 {
                return None;
            }
            if SetWindowLongPtrW(hwnd, GWLP_WNDPROC, window_proc as *const () as isize) == 0 {
                RemovePropW(hwnd, property());
                return None;
            }
        }
        let work = native_work_area(anchor)?;
        let mut card: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut card) == 0 {
            return None;
        }
        // Position in desktop pixels after Windows has sized the card for its display.
        let width = card.right - card.left;
        let height = card.bottom - card.top;
        let margin = (MARGIN * scale).round() as i32;
        let x = (work.left + (work.right - work.left - width) / 2).max(work.left);
        let y = (work.bottom - margin - height).max(work.top);
        if IsWindowVisible(hwnd) == 0 || (card.left, card.top) != (x, y) {
            let flags = SWP_NOACTIVATE | SWP_NOSIZE | SWP_SHOWWINDOW;
            if SetWindowPos(hwnd, HWND_TOPMOST, x, y, 0, 0, flags) == 0 {
                return None;
            }
        }
        Some(hwnd)
    }
}

#[cfg(windows)]
mod native_shadow {
    use std::{mem, ptr};
    use windows_sys::Win32::{
        Foundation::{HWND, POINT, RECT, SIZE},
        Graphics::Gdi::*,
        UI::WindowsAndMessaging::*,
    };

    #[derive(Default)]
    pub(super) struct Shadow {
        hwnd: HWND,
        layout: Option<(HWND, i32, i32, i32, i32)>,
    }

    impl Shadow {
        pub(super) fn visible_and_inactive(&self) -> bool {
            let Some((card, _, _, _, _)) = self.layout else { return false };
            unsafe {
                IsWindowVisible(card) != 0 && IsWindowVisible(self.hwnd) != 0
                    && GetWindowLongPtrW(card, GWL_EXSTYLE) as u32 & WS_EX_NOACTIVATE != 0
                    && GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32 & WS_EX_TRANSPARENT != 0
                    && GetForegroundWindow() != card && GetForegroundWindow() != self.hwnd
            }
        }

        pub(super) fn update(&mut self, card: HWND) {
            let mut rect: RECT = unsafe { mem::zeroed() };
            if unsafe { GetWindowRect(card, &mut rect) } == 0 {
                return;
            }
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            if !(1..=2048).contains(&width) || !(1..=2048).contains(&height) {
                return;
            }
            let layout = (card, rect.left, rect.top, width, height);
            if self.layout == Some(layout) {
                return;
            }
            if self.hwnd.is_null() {
                // A layered transparent sibling cannot consume pointer input from the desktop.
                let style = WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST;
                self.hwnd = unsafe {
                    CreateWindowExW(
                        style,
                        windows_sys::w!("STATIC"),
                        ptr::null(),
                        WS_POPUP,
                        0,
                        0,
                        0,
                        0,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null(),
                    )
                };
                if self.hwnd.is_null() {
                    return;
                }
            }

            let scale = (width as f32 / super::WIDTH).clamp(0.75, 3.0);
            let padding = (24.0 * scale).ceil() as i32;
            let radius = (24.0 * scale).min(width.min(height) as f32 / 2.0);
            let origin = POINT {
                x: rect.left - padding,
                y: rect.top - padding,
            };
            let size = SIZE {
                cx: width + padding * 2,
                cy: height + padding * 2,
            };
            let mut pixels = Vec::with_capacity((size.cx * size.cy) as usize);
            for y in 0..size.cy {
                for x in 0..size.cx {
                    let x = (x - padding) as f32 - width as f32 / 2.0;
                    let y = (y - padding) as f32 - height as f32 / 2.0;
                    pixels.push(
                        u32::from(shadow_alpha(
                            x,
                            y,
                            width as f32,
                            height as f32,
                            radius,
                            scale,
                        )) << 24,
                    );
                }
            }
            let success = unsafe {
                upload(self.hwnd, origin, size, &pixels)
                    && SetWindowPos(
                        self.hwnd,
                        card,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    ) != 0
            };
            if success {
                self.layout = Some(layout);
            } else {
                *self = Self::default();
            }
        }
    }

    impl Drop for Shadow {
        fn drop(&mut self) {
            if !self.hwnd.is_null() {
                unsafe { DestroyWindow(self.hwnd) };
            }
        }
    }

    pub(super) fn shadow_alpha(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        scale: f32,
    ) -> u8 {
        let distance = |y: f32| {
            let dx = x.abs() - width / 2.0 + radius;
            let dy = y.abs() - height / 2.0 + radius;
            dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - radius
        };
        if distance(y) <= 0.0 {
            return 0;
        }
        let d = distance(y - 6.0 * scale) / (11.0 * scale);
        (42.5 * (1.0 - (0.8 * d).tanh())).round() as u8
    }

    unsafe fn upload(hwnd: HWND, origin: POINT, size: SIZE, pixels: &[u32]) -> bool {
        let screen = GetDC(ptr::null_mut());
        if screen.is_null() {
            return false;
        }
        let mem_dc = CreateCompatibleDC(screen);
        let ok = if mem_dc.is_null() {
            false
        } else {
            let mut info: BITMAPINFO = mem::zeroed();
            info.bmiHeader.biSize = mem::size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = size.cx;
            info.bmiHeader.biHeight = -size.cy;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            let mut bits = ptr::null_mut();
            let bitmap =
                CreateDIBSection(screen, &info, DIB_RGB_COLORS, &mut bits, ptr::null_mut(), 0);
            let painted = if bitmap.is_null() || bits.is_null() {
                false
            } else {
                let old = SelectObject(mem_dc, bitmap);
                if old.is_null() || old as isize == -1 {
                    false
                } else {
                    ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u32>(), pixels.len());
                    let blend = BLENDFUNCTION {
                        BlendOp: AC_SRC_OVER as u8,
                        BlendFlags: 0,
                        SourceConstantAlpha: 255,
                        AlphaFormat: AC_SRC_ALPHA as u8,
                    };
                    let src = POINT { x: 0, y: 0 };
                    let uploaded = UpdateLayeredWindow(
                        hwnd, screen, &origin, &size, mem_dc, &src, 0, &blend, ULW_ALPHA,
                    ) != 0;
                    SelectObject(mem_dc, old);
                    uploaded
                }
            };
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            DeleteDC(mem_dc);
            painted
        };
        ReleaseDC(ptr::null_mut(), screen);
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_click_is_bound_to_the_recording_at_press() {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        let proportional = fonts.families[&FontFamily::Proportional].clone();
        fonts.families.insert(FontFamily::Name("humanist".into()), proportional);
        ctx.set_fonts(fonts);
        let state = State::default();
        let (width, height) = (380.0, 144.0);
        let draw = |generation, pressed| {
            let mut cancelled = false;
            let events = match pressed {
                Some(pressed) => vec![egui::Event::PointerButton {
                    pos: pos2(width - 43.0, height - 25.0),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
                None => Vec::new(),
            };
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(width, height))),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                let card = Card { generation, recording: true, caption: "", text: "", status: "" };
                cancelled = state.draw(ctx, &card, 0.0, false);
            });
            cancelled
        };
        assert!(!draw(1, None));
        assert!(!draw(1, Some(true)));
        assert!(!draw(2, Some(false)));
        assert!(!draw(2, Some(true)));
        assert!(draw(2, Some(false)));
    }

    #[test]
    fn card_grows_within_work_area_and_voice_reacts_without_noise() {
        assert_eq!(fitted_height(36.0, 1_000.0, 0.0, HEADER), 194.0);
        assert_eq!(fitted_height(12.0, 1_000.0, 194.0, HEADER), 194.0);
        assert_eq!(fitted_height(900.0, 600.0, 0.0, HEADER), 330.0);
        let hot = smooth_voice(0.0, &[0.0, 0.2, f32::NAN], 0.04);
        assert!(hot > 0.4 && hot < 1.0);
        let cooled = smooth_voice(hot, &[0.0; 5], 0.04);
        assert!(cooled > 0.0 && cooled < hot);
        assert_eq!(smooth_voice(0.0, &[f32::NAN, -1.0], 0.04), 0.0);
    }

    #[cfg(windows)]
    #[test]
    fn native_card_moves_into_the_work_area_without_activation() {
        use windows_sys::Win32::{Foundation::{HWND, RECT}, UI::WindowsAndMessaging::*};

        struct TestWindow(HWND);
        impl Drop for TestWindow {
            fn drop(&mut self) {
                unsafe { DestroyWindow(self.0) };
            }
        }

        let title = format!("Listen Lantern position test ({})", std::process::id());
        let wide_title: Vec<_> = title.encode_utf16().chain(Some(0)).collect();
        let none = std::ptr::null_mut();
        unsafe {
            let anchor = GetDesktopWindow() as isize;
            let work = native_work_area(anchor).expect("test needs a Windows display");
            let card = TestWindow(CreateWindowExW(
                0, windows_sys::w!("STATIC"), wide_title.as_ptr(), WS_POPUP,
                work.right + 100, work.bottom + 100, 200, 100,
                none, none, none, std::ptr::null(),
            ));
            assert!(!card.0.is_null());
            assert_eq!(IsWindowVisible(card.0), 0);
            assert_eq!(protected_window(&title, anchor, 1.5), Some(card.0));
            let mut bounds: RECT = std::mem::zeroed();
            assert_ne!(GetWindowRect(card.0, &mut bounds), 0);
            // An odd work-area width can leave one extra pixel on either side.
            assert!((bounds.left + bounds.right - work.left - work.right).abs() <= 1);
            assert_eq!(work.bottom - bounds.bottom, 24);
            assert_ne!(IsWindowVisible(card.0), 0);
            assert_ne!(GetForegroundWindow(), card.0);
            assert_eq!(SendMessageW(card.0, WM_MOUSEACTIVATE, 0, 0), MA_NOACTIVATE as isize);
        }
    }

    #[cfg(windows)]
    #[test]
    fn shadow_is_empty_inside_the_card_and_feathers_outside() {
        let alpha = |y| native_shadow::shadow_alpha(0.0, y, 380.0, 200.0, 24.0, 1.0);
        assert_eq!(alpha(50.0), 0);
        assert!(alpha(101.0) > alpha(112.0));
        assert!(alpha(112.0) > alpha(123.0));
        assert!(alpha(102.0) > alpha(-102.0));
    }
}
