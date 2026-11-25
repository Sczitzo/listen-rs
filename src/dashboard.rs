use serde::Deserialize;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use wry::http::{Method, Request, Response, StatusCode, Uri};
use wry::{NewWindowResponse, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows, WebViewExtWindows};
use crate::storage::Settings;
use crate::theme::{LayoutId, ThemeId};

const ORIGIN: &str = "listen.localhost";
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";

#[derive(Deserialize)]
pub struct Call {
    pub id: u64,
    pub method: String,
    pub args: Vec<Value>,
}

pub struct Dashboard {
    view: WebView,
    _context: WebContext,
    pub requests: Receiver<Call>,
}

fn hex_color(color: eframe::egui::Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

pub fn appearance(settings: &Settings) -> Value {
    let p = settings.theme.palette();
    let colors = [
        ("bg", p.bg), ("surface", p.surface), ("surface-2", p.surface2), ("nav", p.nav),
        ("line", p.line), ("fg", p.fg), ("muted", p.muted), ("faint", p.faint),
        ("accent", p.accent), ("accent-fg", p.accent_fg), ("rec", p.rec), ("amber", p.amber),
        ("green", p.green), ("brand-blue", p.blue),
    ].map(|(name, color)| (name, hex_color(color)));
    let swatches = ThemeId::ALL
        .map(|theme| {
            let tp = theme.palette();
            (theme.key(), [hex_color(tp.bg), hex_color(tp.surface), hex_color(tp.accent), hex_color(tp.fg)])
        })
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    let layout_blurbs = LayoutId::ALL
        .map(|layout| (layout.key(), layout.blurb()))
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    json!({
        "theme": settings.theme.key(), "layout": settings.layout.key(), "dark": p.dark,
        "colors": colors.into_iter().collect::<std::collections::BTreeMap<_, _>>(),
        "themes": ThemeId::ALL.map(|theme| [theme.key(), theme.label()]),
        "layouts": LayoutId::ALL.map(|layout| [layout.key(), layout.label()]),
        "swatches": swatches,
        "layout_blurbs": layout_blurbs,
    })
}

fn local(uri: &Uri) -> bool {
    uri.scheme_str() == Some("https") && uri.authority().is_some_and(|value| value.as_str() == ORIGIN)
}

fn asset(request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    // Wry restores the original custom scheme before handing us an asset request.
    let uri = request.uri();
    let file: Option<(&str, &[u8])> = if uri.scheme_str() == Some("listen")
        && uri.authority().is_some_and(|value| value.as_str() == "localhost")
        && request.method() == Method::GET
    {
        match request.uri().path() {
            "/index.html" => Some(("text/html; charset=utf-8", include_bytes!("../ui/index.html"))),
            "/app.css" => Some(("text/css; charset=utf-8", include_bytes!("../ui/app.css"))),
            "/app.js" => Some(("text/javascript; charset=utf-8", include_bytes!("../ui/app.js"))),
            "/listen-rust.png" => Some(("image/png", include_bytes!(concat!(env!("OUT_DIR"), "/listen-rust.png")))),
            "/model-benchmarks.js" => Some(("text/javascript; charset=utf-8", include_bytes!("../ui/model-benchmarks.js"))),
            _ => None,
        }
    } else {
        None
    };
    let mut response = Response::builder()
        .header("Content-Security-Policy", CSP)
        .header("X-Content-Type-Options", "nosniff")
        .header("Cache-Control", "no-store");
    if let Some((content_type, bytes)) = file {
        response = response.header("Content-Type", content_type);
        response.body(Cow::Borrowed(bytes)).unwrap()
    } else {
        response.status(StatusCode::NOT_FOUND).body(Cow::Borrowed(&[][..])).unwrap()
    }
}

impl Dashboard {
    pub fn new(window: &impl raw_window_handle::HasWindowHandle, ctx: &eframe::egui::Context,
        profile: PathBuf, check: bool, preview: bool, initial: &Value, bg: (u8, u8, u8, u8)) -> wry::Result<Self> {
        let (tx, requests) = mpsc::sync_channel(128);
        let ctx = ctx.clone();
        let mut context = WebContext::new(Some(profile));
        let view = WebViewBuilder::new_with_web_context(&mut context)
            .with_https_scheme(true)
            .with_background_color(bg)
            .with_focused(!check && !preview)
            .with_devtools(false)
            .with_url("listen://localhost/index.html")
            .with_custom_protocol("listen".to_string(), |_, request| asset(request))
            .with_navigation_handler(|url| url.parse::<Uri>()
                .is_ok_and(|uri| local(&uri) && uri.path() == "/index.html" && uri.query().is_none()))
            .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
            .with_initialization_script(format!("window.listenDashboard = {initial};"))
            .with_initialization_script(include_str!("dashboard.js"))
            .with_initialization_script(if check { include_str!("dashboard_check.js") } else { "" })
            .with_ipc_handler(move |request| {
                let uri = request.uri();
                if local(uri) && uri.path() == "/index.html" && request.body().len() <= 1024 * 1024 {
                    if let Ok(call) = serde_json::from_str::<Call>(request.body()) {
                        if tx.try_send(call).is_ok() {
                            ctx.request_repaint();
                        }
                    }
                }
            })
            .build(window)?;
        Ok(Self { view, requests, _context: context })
    }

    pub fn respond(&self, id: u64, value: Value) {
        let reply = json!({"id": id, "value": value});
        let _ = self.view.evaluate_script(&format!("window.receiveListenResponse({reply});"));
    }

    pub fn evaluate(&self, script: &str) {
        let _ = self.view.evaluate_script(script);
    }

    pub(crate) fn request_quit(&self) -> wry::Result<()> {
        self.view.evaluate_script("window.requestDictationQuit()")
    }

    pub fn screenshot(&self, path: &Path, ctx: &eframe::egui::Context, hwnd: isize, comparison: bool) -> Result<Receiver<Result<(), String>>, String> {
        use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG;
        use windows::Win32::Foundation::HGLOBAL;
        use windows::Win32::System::Com::{StructuredStorage::CreateStreamOnHGlobal, STREAM_SEEK_END, STREAM_SEEK_SET};

        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let stream = unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) }.map_err(|error| error.to_string())?;
        let (sender, receiver) = mpsc::channel();
        let output = path.to_owned();
        let callback_stream = stream.clone();
        let ctx = ctx.clone();
        let title = capture_title(hwnd as _);
        let handler = webview2_com::CapturePreviewCompletedHandler::create(Box::new(move |status| {
            let result = (|| {
                status.map_err(|error| error.to_string())?;
                let mut size = 0;
                unsafe { callback_stream.Seek(0, STREAM_SEEK_END, Some(&mut size)) }.map_err(|error| error.to_string())?;
                if !(8..=64 * 1024 * 1024).contains(&size) { return Err("Invalid image size".into()); }
                unsafe { callback_stream.Seek(0, STREAM_SEEK_SET, None) }.map_err(|error| error.to_string())?;
                let mut bytes = vec![0u8; size as usize];
                let mut read = 0;
                unsafe { callback_stream.Read(bytes.as_mut_ptr().cast(), size as u32, Some(&mut read)) }
                    .ok().map_err(|error| error.to_string())?;
                if u64::from(read) != size || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                    return Err("WebView2 did not return a PNG".into());
                }
                if let Some(title) = &title {
                    let client = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
                        .map_err(|error| error.to_string())?.into_rgba8();
                    if client.width() == title.width() {
                        let window = compose_window(title, &client, comparison);
                        bytes.clear();
                        image::DynamicImage::ImageRgba8(window).write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
                            .map_err(|error| error.to_string())?;
                    }
                }
                let temporary = output.with_extension(format!("{}.png.partial", std::process::id()));
                std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, &output))
                    .map_err(|error| { let _ = std::fs::remove_file(temporary); error.to_string() })
            })();
            let _ = sender.send(result);
            ctx.request_repaint();
            Ok(())
        }));
        unsafe { self.view.webview().CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, &stream, &handler) }
            .map_err(|error| error.to_string())?;
        Ok(receiver)
    }
}

fn compose_window(title: &image::RgbaImage, client: &image::RgbaImage, comparison: bool) -> image::RgbaImage {
    // The reference includes space around its 1593 px window. Leave it transparent and scale it with the capture DPI.
    const REFERENCE_WINDOW_WIDTH: u32 = 1593;
    let [left, top, right, bottom] = if comparison {
        [11, 27, 31, 25].map(|pixels| {
            (pixels * client.width() + REFERENCE_WINDOW_WIDTH / 2) / REFERENCE_WINDOW_WIDTH
        })
    } else {
        [0; 4]
    };
    let mut window = image::RgbaImage::new(
        left + client.width() + right,
        top + title.height() + client.height() + bottom,
    );
    image::imageops::replace(&mut window, title, i64::from(left), i64::from(top));
    image::imageops::replace(&mut window, client, i64::from(left), i64::from(top + title.height()));
    window
}

fn capture_title(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<image::RgbaImage> {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::{Dwm::*, Gdi::*};
    use windows_sys::Win32::Storage::Xps::PrintWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect};

    unsafe {
        let mut window: RECT = std::mem::zeroed();
        let mut frame: RECT = std::mem::zeroed();
        let mut client: RECT = std::mem::zeroed();
        let mut origin = POINT { x: 0, y: 0 };
        if GetWindowRect(hwnd, &mut window) == 0 || GetClientRect(hwnd, &mut client) == 0
            || ClientToScreen(hwnd, &mut origin) == 0
            || DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS as u32, (&mut frame as *mut RECT).cast(), std::mem::size_of::<RECT>() as u32) < 0
        {
            return None;
        }
        let width = window.right - window.left;
        let height = origin.y - window.top;
        let top = frame.top - window.top;
        let left = origin.x - window.left;
        if !(1..=16384).contains(&width) || !(1..=512).contains(&height)
            || !(0..height).contains(&top) || left < 0 || !(1..=width - left).contains(&client.right)
        {
            return None;
        }
        // Keep the preview's native title legible without activating the window.
        let caption_text = 0x00ff_ffffu32;
        DwmSetWindowAttribute(hwnd, DWMWA_TEXT_COLOR as u32, (&caption_text as *const u32).cast(), std::mem::size_of_val(&caption_text) as u32);
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() { return None; }
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if bitmap.is_null() { DeleteDC(dc); return None; }
        let previous = SelectObject(dc, bitmap);
        // Ask Windows to draw this window alone, even when other windows cover it.
        let title = if !previous.is_null() && !bits.is_null() && PrintWindow(hwnd, dc, 2) != 0 {
            GdiFlush();
            let pixels = std::slice::from_raw_parts(bits.cast::<[u8; 4]>(), width as usize * height as usize);
            pixels.iter().any(|pixel| pixel[..3] != [0, 0, 0]).then(|| {
                image::RgbaImage::from_fn(client.right as u32, (height - top) as u32, |x, y| {
                    let [b, g, r, _] = pixels[(y as i32 + top) as usize * width as usize + (x as i32 + left) as usize];
                    image::Rgba([r, g, b, 255])
                })
            })
        } else { None };
        if !previous.is_null() { SelectObject(dc, previous); }
        DeleteObject(bitmap);
        DeleteDC(dc);
        title
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_keeps_the_existing_appearance_choices() {
        let mut settings = Settings::default();
        let choices = appearance(&settings);
        assert_eq!(choices["themes"], json!([
            ["warm", "Warm charcoal"], ["paper", "Paper"], ["midnight", "Midnight"],
            ["sage", "Sage"], ["contrast", "High contrast"],
            ["espresso", "Espresso"], ["nordic", "Nordic"], ["plum", "Plum"],
            ["ocean", "Ocean"], ["obsidian", "Obsidian"], ["mist", "Mist"],
            ["blossom", "Blossom"],
        ]));
        assert_eq!(choices["layouts"], json!([
            ["desk", "Writer's desk"], ["sidebar", "Sidebar"],
            ["compact", "Compact"],
        ]));
        let legacy: Settings = serde_json::from_str(r#"{"layout":"Focus"}"#).unwrap();
        assert_eq!(legacy.layout, LayoutId::Desk);

        for option in choices["themes"].as_array().unwrap() {
            let key = option[0].as_str().unwrap();
            settings.theme = ThemeId::from_key(key).unwrap();
            let saved = serde_json::to_vec(&settings).unwrap();
            let reopened: Settings = serde_json::from_slice(&saved).unwrap();
            let reopened_appearance = appearance(&reopened);
            assert_eq!(reopened_appearance["theme"], key);
            assert_eq!(reopened_appearance["dark"], settings.theme.palette().dark);
        }
    }

    #[test]
    fn comparison_matches_reference_framing_without_sampling_the_desktop() {
        let title = image::RgbaImage::from_pixel(1593, 2, image::Rgba([32, 32, 32, 255]));
        let client = image::RgbaImage::from_pixel(1593, 3, image::Rgba([33, 31, 29, 255]));
        let comparison = compose_window(&title, &client, true);
        assert_eq!(comparison.dimensions(), (1635, 57));
        for (x, y, pixel) in comparison.enumerate_pixels() {
            let expected = match (x, y) {
                (11..=1603, 27..=28) => title.get_pixel(x - 11, y - 27),
                (11..=1603, 29..=31) => client.get_pixel(x - 11, y - 29),
                _ => &image::Rgba([0, 0, 0, 0]),
            };
            assert_eq!(pixel, expected);
        }
        let standard = compose_window(&title, &client, false);
        assert_eq!(standard.dimensions(), (1593, 5));
        assert_eq!(standard.get_pixel(0, 2), client.get_pixel(0, 0));
    }

    #[test]
    fn embedded_assets_allow_only_the_app_origin_and_exact_files() {
        fn request(url: &str) -> Response<Cow<'static, [u8]>> {
            asset(Request::get(url).body(Vec::new()).unwrap())
        }
        assert_eq!(request("listen://localhost/app.js?v=release").status(), 200);
        for url in ["listen://localhost.evil.test/app.js", "http://listen.localhost/app.js",
            "listen://localhost/../app.js", "listen://localhost/settings.json"] {
            assert_eq!(request(url).status(), 404);
        }
    }
}
