use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::{os::windows::ffi::OsStrExt, ptr::null_mut};
use windows_sys::Win32::{
    Graphics::Gdi::{CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO, BI_RGB, DIB_RGB_COLORS},
    Storage::FileSystem::GetDriveTypeW,
    System::WindowsProgramming::DRIVE_FIXED,
    UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, PrivateExtractIconsW, DI_NORMAL, HICON},
};

const SIZE: i32 = 128;
const MAX_CACHED_ICONS: usize = 64;

fn icon_cache() -> &'static Mutex<HashMap<PathBuf, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn from_executable(path: &Path) -> Option<String> {
    if let Some(cached) = icon_cache().lock().unwrap().get(path) {
        return cached.clone();
    }
    let extracted = extract_from_executable(path);
    let mut cache = icon_cache().lock().unwrap();
    if cache.len() >= MAX_CACHED_ICONS {
        cache.clear();
    }
    cache.insert(path.to_path_buf(), extracted.clone());
    extracted
}

fn extract_from_executable(path: &Path) -> Option<String> {
    let drive = path.ancestors().last()?;
    let drive: Vec<u16> = drive.as_os_str().encode_wide().chain([0]).collect();
    if unsafe { GetDriveTypeW(drive.as_ptr()) } != DRIVE_FIXED { return None; }
    let name: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut icon = null_mut();
    let count = unsafe { PrivateExtractIconsW(name.as_ptr(), 0, SIZE, SIZE, &mut icon, null_mut(), 1, 0) };
    let pixels = (count == 1 && !icon.is_null()).then(|| draw(icon)).flatten();
    if !icon.is_null() { unsafe { DestroyIcon(icon) }; }
    let mut pixels = pixels?;
    if pixels.chunks_exact(4).all(|pixel| pixel[3] == 0) { return None; }
    for bgra in pixels.chunks_exact_mut(4) {
        bgra.swap(0, 2);
        let alpha = u32::from(bgra[3]);
        if (1..255).contains(&alpha) {
            for channel in &mut bgra[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    let mut png = Vec::new();
    PngEncoder::new(&mut png).write_image(&pixels, SIZE as u32, SIZE as u32, ExtendedColorType::Rgba8).ok()?;
    (png.len() <= 18_432).then(|| STANDARD.encode(png))
}

fn draw(icon: HICON) -> Option<Vec<u8>> {
    unsafe {
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() { return None; }
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of_val(&info.bmiHeader) as u32;
        info.bmiHeader.biWidth = SIZE;
        info.bmiHeader.biHeight = -SIZE;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut pixels = null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut pixels, null_mut(), 0);
        let data = if !bitmap.is_null() && !pixels.is_null() {
            let count = (SIZE * SIZE * 4) as usize;
            std::ptr::write_bytes(pixels.cast::<u8>(), 0, count);
            let old = SelectObject(dc, bitmap);
            let result = if !old.is_null() {
                let drawn = DrawIconEx(dc, 0, 0, icon, SIZE, SIZE, 0, null_mut(), DI_NORMAL) != 0;
                SelectObject(dc, old);
                drawn.then(|| std::slice::from_raw_parts(pixels.cast::<u8>(), count).to_vec())
            } else { None };
            result
        } else { None };
        DeleteDC(dc);
        if !bitmap.is_null() { DeleteObject(bitmap); }
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_executable_is_cached_without_error() {
        let missing = Path::new(r"C:\definitely-missing-listen-icon-test.exe");
        assert_eq!(from_executable(missing), None);
        assert!(icon_cache().lock().unwrap().contains_key(missing));
        assert_eq!(from_executable(missing), None);
    }
}
