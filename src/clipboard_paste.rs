//! Safe Windows clipboard paste transaction with delayed-render receipt and automatic
//! restoration of the user's previous clipboard. Modeled on the reference Listen app's
//! `paste_windows.py` and `multiline_delivery.py`.

use super::{Delivery, DeliveryError, Target};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{GetLastError, SetLastError, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VK_CONTROL, VK_V,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetAncestor, GetForegroundWindow,
    GetMessageW, GetWindowThreadProcessId, PostMessageW, PostQuitMessage, RegisterClassW,
    TranslateMessage, GA_ROOT, HWND_MESSAGE, MSG, WM_CLOSE, WM_DESTROY, WNDCLASSW,
};

const CF_TEXT: u32 = 1;
const CF_BITMAP: u32 = 2;
const CF_OEMTEXT: u32 = 7;
const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;
const CF_LOCALE: u32 = 16;
const CF_DIBV5: u32 = 17;

const WM_RENDERFORMAT: u32 = 0x0305;
const WM_RENDERALLFORMATS: u32 = 0x0306;
const GMEM_MOVEABLE: u32 = 0x0002;
const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;
const RECEIPT_TIMEOUT: Duration = Duration::from_millis(1000);
const NULL_READER_SETTLE: Duration = Duration::from_millis(25);
const CP_ACP: u32 = 0;
const CP_OEMCP: u32 = 1;

#[link(name = "user32")]
extern "system" {
    fn OpenClipboard(hwnd_new_owner: HWND) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn EnumClipboardFormats(format: u32) -> u32;
    fn GetClipboardData(format: u32) -> HGLOBAL;
    fn SetClipboardData(format: u32, mem: HGLOBAL) -> HGLOBAL;
    fn GetClipboardSequenceNumber() -> u32;
    fn GetClipboardOwner() -> HWND;
    fn GetOpenClipboardWindow() -> HWND;
    fn RegisterClipboardFormatW(name: *const u16) -> u32;
    fn GetClipboardFormatNameW(format: u32, name: *mut u16, max_count: i32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> HGLOBAL;
    fn GlobalLock(mem: HGLOBAL) -> *mut c_void;
    fn GlobalUnlock(mem: HGLOBAL) -> i32;
    fn GlobalSize(mem: HGLOBAL) -> usize;
    fn GlobalFree(mem: HGLOBAL) -> HGLOBAL;
    fn GetThreadLocale() -> u32;
    fn WideCharToMultiByte(
        code_page: u32,
        flags: u32,
        wide_str: *const u16,
        wide_len: i32,
        multi_byte_str: *mut u8,
        cb_multi_byte: i32,
        default_char: *const u8,
        used_default_char: *mut i32,
    ) -> i32;
}

type Snapshot = Vec<(u32, Vec<u8>)>;

/// Opaque snapshot of the user's clipboard taken right before Command Mode's `Ctrl+C` capture.
pub(crate) struct CommandClipboardSnapshot {
    saved: Snapshot,
}

#[derive(Clone)]
struct ActiveDelivery {
    utf16: Vec<u8>,
    ansi: Vec<u8>,
    oem: Vec<u8>,
    target_window: isize,
    target_pid: u32,
    focus_pid: u32,
    sent_paste: bool,
    target_read: bool,
    null_reader_read_at: Option<Instant>,
    render_error: bool,
    sequence: u32,
}

impl ActiveDelivery {
    fn format_bytes(&self, format: u32) -> Option<&[u8]> {
        match format {
            CF_UNICODETEXT => Some(&self.utf16),
            CF_TEXT => Some(&self.ansi),
            CF_OEMTEXT => Some(&self.oem),
            _ => None,
        }
    }
}

struct PendingRecovery {
    saved: Snapshot,
    delivery_seq: u32,
}

struct OwnerState {
    hwnd: isize,
    thread: Option<std::thread::JoinHandle<()>>,
}

static TRANSACTION_LOCK: Mutex<()> = Mutex::new(());
static ACTIVE: Mutex<Option<ActiveDelivery>> = Mutex::new(None);
static PENDING: Mutex<Option<PendingRecovery>> = Mutex::new(None);
static OWNER: Mutex<Option<OwnerState>> = Mutex::new(None);
static PRIVACY_FORMATS: OnceLock<Option<[u32; 3]>> = OnceLock::new();

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn privacy_formats() -> Option<[u32; 3]> {
    *PRIVACY_FORMATS.get_or_init(|| {
        let names = [
            "CanIncludeInClipboardHistory",
            "CanUploadToCloudClipboard",
            "ExcludeClipboardContentFromMonitorProcessing",
        ];
        let mut out = [0u32; 3];
        for (slot, name) in out.iter_mut().zip(names) {
            let w = wide(name);
            let id = unsafe { RegisterClipboardFormatW(w.as_ptr()) };
            if id == 0 {
                return None;
            }
            *slot = id;
        }
        Some(out)
    })
}

fn encode_multibyte(code_page: u32, wide_with_nul: &[u16]) -> Vec<u8> {
    let needed = unsafe {
        WideCharToMultiByte(
            code_page,
            0,
            wide_with_nul.as_ptr(),
            wide_with_nul.len() as i32,
            null_mut(),
            0,
            null(),
            null_mut(),
        )
    };
    if needed <= 0 {
        return vec![0];
    }
    let mut out = vec![0u8; needed as usize];
    let written = unsafe {
        WideCharToMultiByte(
            code_page,
            0,
            wide_with_nul.as_ptr(),
            wide_with_nul.len() as i32,
            out.as_mut_ptr(),
            needed,
            null(),
            null_mut(),
        )
    };
    if written <= 0 {
        return vec![0];
    }
    out.truncate(written as usize);
    if out.last() != Some(&0) {
        out.push(0);
    }
    out
}

fn should_attribute_reader(
    sent_paste: bool,
    reader_is_null: bool,
    reader_pid: u32,
    reader_root: isize,
    target_pid: u32,
    focus_pid: u32,
    target_window: isize,
) -> bool {
    if !sent_paste {
        return false;
    }
    // Apps using `arboard` / `clipboard-win` (such as egui / Craft Anything) call
    // `OpenClipboard(NULL)`, leaving `GetOpenClipboardWindow() == NULL`.
    // Electron / Chromium matches `target_pid`; WebView2 (`msedgewebview2.exe`) and UWP
    // (`ApplicationFrameHost.exe` child) match `focus_pid` or `target_window` ancestor.
    reader_is_null
        || (reader_pid != 0 && (reader_pid == target_pid || reader_pid == focus_pid))
        || (reader_root != 0 && reader_root == target_window)
}

fn should_skip_registered_format(format_name: &str) -> bool {
    // Skip only OLE-internal formats that cannot be round-tripped via HGLOBAL.
    // Never skip Windows privacy markers (CanIncludeInClipboardHistory, etc.) so password
    // manager secrets remain excluded from Clipboard History / Cloud Clipboard when restored.
    matches!(
        format_name,
        "DataObject"
            | "Ole Private Data"
            | "Object Descriptor"
            | "Link Source"
            | "Link Source Descriptor"
            | "OwnerLink"
            | "Embed Source"
            | "Embedded Object"
    )
}

fn alloc_global(bytes: &[u8]) -> Option<HGLOBAL> {
    if bytes.is_empty() {
        return None;
    }
    unsafe {
        let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if mem.is_null() {
            return None;
        }
        let ptr = GlobalLock(mem);
        if ptr.is_null() {
            GlobalFree(mem);
            return None;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
        GlobalUnlock(mem);
        Some(mem)
    }
}

unsafe extern "system" fn owner_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_RENDERFORMAT => {
            let format = wparam as u32;
            let data = {
                let guard = ACTIVE.lock().unwrap();
                guard.as_ref().and_then(|d| {
                    d.format_bytes(format).map(|b| {
                        (b.to_vec(), d.sent_paste, d.target_pid, d.focus_pid, d.target_window)
                    })
                })
            };
            if let Some((bytes, sent_paste, target_pid, focus_pid, target_window)) = data {
                let reader = unsafe { GetOpenClipboardWindow() };
                let mut reader_pid = 0u32;
                let reader_root = if !reader.is_null() {
                    unsafe {
                        GetWindowThreadProcessId(reader, &mut reader_pid);
                        GetAncestor(reader, GA_ROOT) as isize
                    }
                } else {
                    0
                };
                let ok = if let Some(mem) = alloc_global(&bytes) {
                    let set = unsafe { SetClipboardData(format, mem) };
                    if set.is_null() {
                        unsafe { GlobalFree(mem) };
                        false
                    } else {
                        true
                    }
                } else {
                    false
                };
                let seq = unsafe { GetClipboardSequenceNumber() };
                let attributed = should_attribute_reader(
                    sent_paste,
                    reader.is_null(),
                    reader_pid,
                    reader_root,
                    target_pid,
                    focus_pid,
                    target_window,
                );
                let mut guard = ACTIVE.lock().unwrap();
                if let Some(delivery) = guard.as_mut() {
                    if ok {
                        delivery.sequence = seq;
                        if attributed {
                            delivery.target_read = true;
                            if reader.is_null() && delivery.null_reader_read_at.is_none() {
                                delivery.null_reader_read_at = Some(Instant::now());
                            }
                        }
                    } else {
                        delivery.render_error = true;
                    }
                }
            }
            0
        }
        WM_RENDERALLFORMATS => {
            let payload = {
                let guard = ACTIVE.lock().unwrap();
                guard.as_ref().map(|d| (d.utf16.clone(), d.ansi.clone(), d.oem.clone()))
            };
            if let Some((utf16, ansi, oem)) = payload {
                if unsafe { OpenClipboard(hwnd) } != 0 {
                    if unsafe { GetClipboardOwner() } == hwnd {
                        for (fmt, bytes) in [(CF_UNICODETEXT, &utf16), (CF_TEXT, &ansi), (CF_OEMTEXT, &oem)] {
                            if let Some(mem) = alloc_global(bytes) {
                                if unsafe { SetClipboardData(fmt, mem) }.is_null() {
                                    unsafe { GlobalFree(mem) };
                                }
                            }
                        }
                    }
                    unsafe { CloseClipboard() };
                }
            }
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn ensure_owner() -> Option<HWND> {
    let mut guard = OWNER.lock().unwrap();
    if let Some(state) = guard.as_ref() {
        if state.hwnd != 0 {
            return Some(state.hwnd as HWND);
        }
    }
    let (tx, rx) = std::sync::mpsc::sync_channel::<isize>(1);
    let thread = std::thread::Builder::new()
        .name("clipboard-owner".into())
        .spawn(move || unsafe {
            let class_name = wide("ListenRsClipboardOwner");
            let instance = GetModuleHandleW(null());
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(owner_wnd_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: null_mut(),
                hCursor: null_mut(),
                hbrBackground: null_mut(),
                lpszMenuName: null(),
                lpszClassName: class_name.as_ptr(),
            };
            let _ = RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                wide("").as_ptr(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                null_mut(),
                instance,
                null(),
            );
            let _ = tx.send(hwnd as isize);
            if hwnd.is_null() {
                return;
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .ok()?;
    let hwnd = rx.recv_timeout(Duration::from_secs(2)).ok().filter(|&h| h != 0)?;
    *guard = Some(OwnerState { hwnd, thread: Some(thread) });
    Some(hwnd as HWND)
}

struct OpenClip;

impl OpenClip {
    fn acquire(owner: HWND) -> Option<Self> {
        for _ in 0..8 {
            if unsafe { OpenClipboard(owner) } != 0 {
                return Some(Self);
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        None
    }
}

impl Drop for OpenClip {
    fn drop(&mut self) {
        unsafe { CloseClipboard() };
    }
}

fn is_safe_standard_format(fmt: u32) -> bool {
    matches!(fmt, CF_TEXT | CF_OEMTEXT | CF_DIB | CF_UNICODETEXT | CF_HDROP | CF_LOCALE | CF_DIBV5)
}

fn snapshot_open_clipboard() -> Result<Snapshot, &'static str> {
    let mut formats = Vec::new();
    let mut fmt = 0u32;
    loop {
        unsafe { SetLastError(0) };
        fmt = unsafe { EnumClipboardFormats(fmt) };
        if fmt == 0 {
            if unsafe { GetLastError() } != 0 {
                return Err("Cannot enumerate clipboard formats.");
            }
            break;
        }
        formats.push(fmt);
    }
    let has_dib = formats.contains(&CF_DIB) || formats.contains(&CF_DIBV5);
    let mut total = 0usize;
    let mut out = Vec::with_capacity(formats.len());
    for fmt in formats {
        // Windows synthesizes CF_BITMAP from CF_DIB/CF_DIBV5; bitmap handles are not HGLOBAL.
        if fmt == CF_BITMAP && has_dib {
            continue;
        }
        let is_registered = !is_safe_standard_format(fmt);
        if is_registered {
            if fmt < 0xC000 {
                return Err("Unsupported non-memory clipboard format.");
            }
            let mut name = [0u16; 256];
            let len = unsafe { GetClipboardFormatNameW(fmt, name.as_mut_ptr(), name.len() as i32) };
            if len == 0 {
                return Err("Unknown clipboard format.");
            }
            let format_name = String::from_utf16_lossy(&name[..len as usize]);
            if should_skip_registered_format(&format_name) {
                continue;
            }
        }
        let handle = unsafe { GetClipboardData(fmt) };
        if handle.is_null() {
            if is_registered {
                continue;
            }
            return Err("Clipboard data unavailable.");
        }
        let size = unsafe { GlobalSize(handle) };
        if size == 0 {
            if is_registered {
                continue;
            }
            return Err("Clipboard data unavailable.");
        }
        if size > MAX_SNAPSHOT_BYTES || total.saturating_add(size) > MAX_SNAPSHOT_BYTES {
            return Err("Clipboard data exceeds restore limit.");
        }
        let ptr = unsafe { GlobalLock(handle) };
        if ptr.is_null() {
            if is_registered {
                continue;
            }
            return Err("Cannot lock clipboard data.");
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), size) }.to_vec();
        unsafe { GlobalUnlock(handle) };
        total += bytes.len();
        out.push((fmt, bytes));
    }
    Ok(out)
}

fn write_snapshot_open_clipboard(entries: &[(u32, Vec<u8>)]) -> Result<(), &'static str> {
    let mut allocated = Vec::with_capacity(entries.len());
    for (fmt, bytes) in entries {
        let Some(mem) = alloc_global(bytes) else {
            for (_, h) in allocated {
                unsafe { GlobalFree(h) };
            }
            return Err("Cannot allocate clipboard memory.");
        };
        allocated.push((*fmt, mem));
    }
    if unsafe { EmptyClipboard() } == 0 {
        for (_, h) in allocated {
            unsafe { GlobalFree(h) };
        }
        return Err("Cannot clear clipboard.");
    }
    let mut iter = allocated.into_iter();
    while let Some((fmt, mem)) = iter.next() {
        if unsafe { SetClipboardData(fmt, mem) }.is_null() {
            unsafe { GlobalFree(mem) };
            for (_, rest) in iter {
                unsafe { GlobalFree(rest) };
            }
            return Err("Cannot restore clipboard format.");
        }
    }
    Ok(())
}

/// Materializes all delayed text formats only while `owner` still owns the clipboard.
/// Returns `Some(updated_seq)` if we still owned the clipboard and materialized cleanly,
/// or `None` if another app already took ownership or materialization failed.
fn materialize_if_owned(owner: HWND, delivery: &ActiveDelivery) -> Option<u32> {
    if unsafe { GetClipboardOwner() } != owner {
        return None;
    }
    for (fmt, bytes) in [
        (CF_UNICODETEXT, &delivery.utf16),
        (CF_TEXT, &delivery.ansi),
        (CF_OEMTEXT, &delivery.oem),
    ] {
        let mem = alloc_global(bytes)?;
        if unsafe { SetClipboardData(fmt, mem) }.is_null() {
            unsafe { GlobalFree(mem) };
            return None;
        }
    }
    Some(unsafe { GetClipboardSequenceNumber() })
}

fn build_active(text: &str, target: Target) -> ActiveDelivery {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n");
    let wide_units: Vec<u16> = normalized.encode_utf16().chain(std::iter::once(0)).collect();
    let mut utf16 = Vec::with_capacity(wide_units.len() * 2);
    for unit in &wide_units {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    let ansi = encode_multibyte(CP_ACP, &wide_units);
    let oem = encode_multibyte(CP_OEMCP, &wide_units);
    ActiveDelivery {
        utf16,
        ansi,
        oem,
        target_window: target.window,
        target_pid: target.pid,
        focus_pid: target.focus_pid,
        sent_paste: false,
        target_read: false,
        null_reader_read_at: None,
        render_error: false,
        sequence: 0,
    }
}

fn send_ctrl_v(target_window: isize) -> u32 {
    let ctrl_scan = unsafe { MapVirtualKeyW(u32::from(VK_CONTROL), MAPVK_VK_TO_VSC) as u16 };
    let v_scan = unsafe { MapVirtualKeyW(u32::from(VK_V), MAPVK_VK_TO_VSC) as u16 };
    let key = |vk, scan, flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    };
    let keys = [
        key(VK_CONTROL, ctrl_scan, 0),
        key(VK_V, v_scan, 0),
        key(VK_V, v_scan, KEYEVENTF_KEYUP),
        key(VK_CONTROL, ctrl_scan, KEYEVENTF_KEYUP),
    ];
    // Final hardware-level guard right before SendInput.
    if unsafe { GetForegroundWindow() } != target_window as HWND {
        return 0;
    }
    let send = |slice: &[INPUT]| unsafe {
        SendInput(slice.len() as u32, slice.as_ptr(), std::mem::size_of::<INPUT>() as i32)
    };
    let count = send(&keys);
    if (1..4).contains(&count) {
        // Release only injected keys. Never retry the paste on partial input.
        send(if count == 2 { &keys[2..] } else { &keys[3..] });
    }
    count
}

fn down(vk: i32) -> bool {
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}

fn active_sequence() -> u32 {
    ACTIVE.lock().unwrap().as_ref().map_or(0, |d| d.sequence)
}

fn record_pending(saved: Option<Snapshot>, delivery_seq: u32) {
    if let Some(snap) = saved {
        if !snap.is_empty() {
            *PENDING.lock().unwrap() = Some(PendingRecovery { saved: snap, delivery_seq });
        }
    }
}

fn rollback_open_clipboard(owner: HWND, saved: Option<Snapshot>) {
    let restored = saved.as_ref().is_some_and(|snap| write_snapshot_open_clipboard(snap).is_ok());
    if restored || unsafe { GetClipboardOwner() } != owner {
        *ACTIVE.lock().unwrap() = None;
    } else {
        let seq = active_sequence();
        record_pending(saved, seq);
    }
}

/// Perform one verified Ctrl+V paste via delayed clipboard rendering and restore the user's
/// previous clipboard automatically once the target app finishes reading the text.
pub(super) fn deliver(
    text: &str,
    target: Target,
    restore_clipboard: bool,
    ready: impl Fn() -> Result<(), DeliveryError>,
) -> Result<Delivery, DeliveryError> {
    let _tx_guard = TRANSACTION_LOCK.lock().unwrap();
    if PENDING.lock().unwrap().is_some() {
        return Err(DeliveryError::NotSentKeepClipboard(
            "Previous clipboard recovery is pending. Resolve it in Listen before pasting again.".into(),
        ));
    }
    let owner = ensure_owner().ok_or_else(|| {
        DeliveryError::NotSent("Windows clipboard helper is unavailable. Your text is saved in Listen.".into())
    })?;
    let privacy = privacy_formats().ok_or_else(|| {
        DeliveryError::NotSent("Windows clipboard privacy formats are unavailable. Your text is saved in Listen.".into())
    })?;

    ready()?;
    let active = build_active(text, target);

    // Phase 1: snapshot existing clipboard and publish delayed text formats + privacy markers.
    let saved = {
        let Some(_clip) = OpenClip::acquire(owner) else {
            return Err(DeliveryError::NotSentKeepClipboard(
                "Clipboard is busy right now. Your text is saved in Listen.".into(),
            ));
        };
        let saved = match snapshot_open_clipboard() {
            Ok(snap) => Some(snap),
            Err(_) if !restore_clipboard => None,
            Err(_) => {
                // Keep the user's exotic/large clipboard untouched; do not let copy_fallback overwrite it.
                return Err(DeliveryError::NotSentKeepClipboard(
                    "Listen kept your current clipboard untouched. Copy your text from Listen.".into(),
                ));
            }
        };
        if ready().is_err() {
            return Err(DeliveryError::NotSent(
                "The original destination lost focus. Your text was kept in Listen.".into(),
            ));
        }

        let locale_bytes = unsafe { GetThreadLocale() }.to_le_bytes().to_vec();
        let mut metadata = vec![(CF_LOCALE, locale_bytes)];
        for fmt in privacy {
            metadata.push((fmt, vec![0, 0, 0, 0]));
        }
        *ACTIVE.lock().unwrap() = Some(active.clone());
        if write_snapshot_open_clipboard(&metadata).is_err() {
            // EmptyClipboard() may already have cleared the clipboard; restore saved immediately.
            rollback_open_clipboard(owner, saved);
            return Err(DeliveryError::NotSentKeepClipboard(
                "Listen could not stage your dictation on the clipboard. It is saved in Listen.".into(),
            ));
        }
        let mut publish_ok = true;
        for fmt in [CF_UNICODETEXT, CF_TEXT, CF_OEMTEXT] {
            unsafe { SetLastError(0) };
            let _ = unsafe { SetClipboardData(fmt, null_mut()) };
            if unsafe { GetLastError() } != 0 {
                publish_ok = false;
                break;
            }
        }
        if !publish_ok {
            rollback_open_clipboard(owner, saved);
            return Err(DeliveryError::NotSentKeepClipboard(
                "Listen could not stage your dictation on the clipboard. It is saved in Listen.".into(),
            ));
        }
        let seq = unsafe { GetClipboardSequenceNumber() };
        if let Some(d) = ACTIVE.lock().unwrap().as_mut() {
            d.sequence = seq;
        }
        saved
    }; // CloseClipboard drops here; because CF_UNICODETEXT/CF_TEXT/CF_OEMTEXT/CF_LOCALE are all set, seq stays stable.

    // Phase 2: re-check focus, mouse/paste keys, and clipboard ownership, then inject Ctrl+V once.
    let can_send = ![0x01, 0x02, 0x04, i32::from(VK_V)].iter().any(|&k| down(k))
        && ready().is_ok()
        && unsafe { GetClipboardOwner() } == owner
        && unsafe { GetClipboardSequenceNumber() } == active_sequence();

    let sent_count = if can_send {
        if let Some(d) = ACTIVE.lock().unwrap().as_mut() {
            d.sent_paste = true;
        }
        send_ctrl_v(target.window)
    } else {
        0
    };

    if sent_count == 0 {
        // Nothing was injected. Restore the user's clipboard immediately if we still own it.
        if let Some(_clip) = OpenClip::acquire(owner) {
            let expected = active_sequence();
            let still_ours = unsafe { GetClipboardOwner() } == owner
                && unsafe { GetClipboardSequenceNumber() } == expected;
            if still_ours {
                if let Some(ref snap) = saved {
                    if write_snapshot_open_clipboard(snap).is_ok() {
                        *ACTIVE.lock().unwrap() = None;
                    } else {
                        record_pending(saved, expected);
                    }
                } else {
                    // Nothing to restore; clear our unpublished delayed formats cleanly.
                    let _ = unsafe { EmptyClipboard() };
                    *ACTIVE.lock().unwrap() = None;
                }
            } else {
                // Another app already replaced the clipboard.
                *ACTIVE.lock().unwrap() = None;
            }
        } else {
            // Clipboard busy: keep ACTIVE alive so WM_RENDERFORMAT never serves NULL, and hold saved in PENDING.
            record_pending(saved, active_sequence());
        }
        return Err(DeliveryError::NotSentKeepClipboard(
            "Automatic paste could not fire because focus or keys changed. Your text is saved in Listen.".into(),
        ));
    }

    // Phase 3: wait for the target app to trigger WM_RENDERFORMAT and finish reading.
    let deadline = Instant::now() + RECEIPT_TIMEOUT;
    let consumed = loop {
        let (read, null_read_at, error) = {
            let guard = ACTIVE.lock().unwrap();
            guard
                .as_ref()
                .map_or((false, None, true), |d| (d.target_read, d.null_reader_read_at, d.render_error))
        };
        if error {
            break false;
        }
        if read && unsafe { GetOpenClipboardWindow() }.is_null() {
            // When the reader used OpenClipboard(NULL) (e.g., arboard/egui-winit), Windows reports
            // GetOpenClipboardWindow() == NULL even while the clipboard is open. Give a short settle
            // window so the receiver finishes reading and closes the clipboard before we restore.
            if null_read_at.is_none_or(|t| t.elapsed() >= NULL_READER_SETTLE) {
                break true;
            }
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(3));
    };

    // Phase 4: restore previous clipboard or preserve pending recovery if unconfirmed.
    if sent_count == 4 && consumed {
        if restore_clipboard {
            if let Some(snap) = saved {
                let mut restored_ok = false;
                let mut keep_active = false;
                let mut recovery_seq = active_sequence();
                if let Some(_clip) = OpenClip::acquire(owner) {
                    // Sample ACTIVE.sequence AFTER acquiring the clipboard lock so a concurrent
                    // multi-format WM_RENDERFORMAT cannot race the sequence comparison.
                    let expected = active_sequence();
                    recovery_seq = expected;
                    let still_ours = unsafe { GetClipboardOwner() } == owner
                        && unsafe { GetClipboardSequenceNumber() } == expected;
                    if !still_ours {
                        // A newer user copy superseded ours; preserve it without touching clipboard.
                        restored_ok = true;
                    } else if write_snapshot_open_clipboard(&snap).is_ok() {
                        restored_ok = true;
                    } else {
                        keep_active = true;
                    }
                } else {
                    keep_active = true;
                }
                if !keep_active {
                    *ACTIVE.lock().unwrap() = None;
                }
                if restored_ok || snap.is_empty() {
                    return Ok(Delivery::Confirmed);
                }
                record_pending(Some(snap), recovery_seq);
                return Err(DeliveryError::Uncertain(
                    "Text was pasted, but Listen could not restore your previous clipboard yet. Resolve clipboard recovery in Listen.".into(),
                ));
            }
            *ACTIVE.lock().unwrap() = None;
            return Ok(Delivery::Confirmed);
        } else {
            // restore_clipboard == false: materialize all formats so ACTIVE can be cleared safely.
            let mut cleared = false;
            if let Some(_clip) = OpenClip::acquire(owner) {
                if unsafe { GetClipboardOwner() } != owner {
                    cleared = true;
                } else if let Some(d) = ACTIVE.lock().unwrap().clone() {
                    cleared = materialize_if_owned(owner, &d).is_some();
                }
            }
            if cleared {
                *ACTIVE.lock().unwrap() = None;
            }
            return Ok(Delivery::Confirmed);
        }
    }

    // Unconfirmed paste (sent 4 without receipt, or partial 1..4): materialize our delayed text
    // ONLY while `owner` still owns the clipboard (never capturing a newer foreign item's sequence),
    // and hold the previous non-empty clipboard in RAM for explicit user recovery.
    let mut recovery_seq = active_sequence();
    let mut clear_active = false;
    if let Some(_clip) = OpenClip::acquire(owner) {
        recovery_seq = active_sequence();
        if unsafe { GetClipboardOwner() } != owner {
            // Another app already replaced the clipboard; keep our own `recovery_seq` so
            // `restore_pending` will see `current != recovery_seq` and never clobber the newer item.
            clear_active = true;
        } else if let Some(d) = ACTIVE.lock().unwrap().clone() {
            if let Some(materialized_seq) = materialize_if_owned(owner, &d) {
                recovery_seq = materialized_seq;
                clear_active = true;
            }
        }
    }
    if clear_active {
        *ACTIVE.lock().unwrap() = None;
    }
    if restore_clipboard {
        record_pending(saved, recovery_seq);
    }
    let reason = if sent_count != 4 {
        "Windows only sent part of the paste shortcut. Check the original app before pasting again."
    } else {
        "Paste was sent, but the target app did not confirm reading the clipboard. Check the app before pasting again."
    };
    Err(DeliveryError::Uncertain(reason.into()))
}

pub(crate) fn pending_status() -> serde_json::Value {
    let pending = PENDING.lock().unwrap();
    if pending.is_some() {
        serde_json::json!({
            "pending": true,
            "message": "Your previous clipboard item is held safely in memory until you restore or discard it."
        })
    } else {
        serde_json::json!({ "pending": false, "message": "" })
    }
}

pub(crate) fn has_pending() -> bool {
    PENDING.lock().unwrap().is_some()
}

/// Snapshot the current Windows clipboard before Command Mode invokes `Ctrl+C`.
/// Refuses to proceed if a prior paste recovery is pending, or if `restore_clipboard`
/// is enabled and the clipboard contains an unsnapshottable/oversized format.
pub(crate) fn snapshot_for_command(
    restore_clipboard: bool,
) -> Result<Option<CommandClipboardSnapshot>, String> {
    let _tx_guard = TRANSACTION_LOCK.lock().unwrap();
    if PENDING.lock().unwrap().is_some() {
        return Err(
            "Previous clipboard recovery is pending. Resolve it in Listen before using Command Mode."
                .into(),
        );
    }
    if !restore_clipboard {
        return Ok(None);
    }
    let owner = ensure_owner().ok_or_else(|| {
        "Windows clipboard helper is unavailable for Command Mode.".to_owned()
    })?;
    let _clip = OpenClip::acquire(owner)
        .ok_or_else(|| "Clipboard is busy right now. Try Command Mode again.".to_owned())?;
    match snapshot_open_clipboard() {
        Ok(saved) => Ok(Some(CommandClipboardSnapshot { saved })),
        Err(_) => Err(
            "Listen kept your current clipboard untouched because it cannot be restored safely."
                .into(),
        ),
    }
}

/// Restore the user's clipboard immediately after Command Mode reads the copied selection,
/// only if no paste recovery is pending and the clipboard sequence still matches `post_copy_seq`.
pub(crate) fn restore_after_command(
    snapshot: Option<CommandClipboardSnapshot>,
    post_copy_seq: Option<u32>,
) {
    let Some(snapshot) = snapshot else {
        return;
    };
    let _tx_guard = TRANSACTION_LOCK.lock().unwrap();
    if PENDING.lock().unwrap().is_some() {
        return;
    }
    let Some(owner) = ensure_owner() else {
        return;
    };
    let Some(_clip) = OpenClip::acquire(owner) else {
        return;
    };
    if let Some(expected) = post_copy_seq {
        if unsafe { GetClipboardSequenceNumber() } != expected {
            return;
        }
    }
    let _ = write_snapshot_open_clipboard(&snapshot.saved);
}

pub(crate) fn restore_pending(confirmed: bool) -> serde_json::Value {
    let Ok(_tx_guard) = TRANSACTION_LOCK.try_lock() else {
        return serde_json::json!({
            "status": "uncertain",
            "message": "A paste is currently in progress. Wait a moment and try again."
        });
    };
    if !confirmed {
        return serde_json::json!({
            "status": "uncertain",
            "message": "Confirm no paste is still waiting before restoring your previous clipboard."
        });
    }
    let mut pending = PENDING.lock().unwrap();
    let Some(item) = pending.as_ref() else {
        return serde_json::json!({
            "status": "retained",
            "message": "No clipboard recovery is pending."
        });
    };
    let Some(owner) = ensure_owner() else {
        return serde_json::json!({
            "status": "uncertain",
            "message": "Clipboard helper is unavailable; previous clipboard remains in memory."
        });
    };
    let Some(_clip) = OpenClip::acquire(owner) else {
        return serde_json::json!({
            "status": "uncertain",
            "message": "Clipboard is busy; previous clipboard remains in memory."
        });
    };
    let current = unsafe { GetClipboardSequenceNumber() };
    let owned_by_us = unsafe { GetClipboardOwner() } == owner;
    let active_seq = active_sequence();
    let seq_matches = current == item.delivery_seq || (active_seq != 0 && current == active_seq);
    if !owned_by_us || !seq_matches {
        return serde_json::json!({
            "status": "uncertain",
            "message": "A newer clipboard item is present; previous clipboard remains in memory."
        });
    }
    match write_snapshot_open_clipboard(&item.saved) {
        Ok(()) => {
            *pending = None;
            *ACTIVE.lock().unwrap() = None;
            serde_json::json!({
                "status": "restored",
                "message": "Previous clipboard restored."
            })
        }
        Err(_) => serde_json::json!({
            "status": "uncertain",
            "message": "Clipboard restoration failed; previous clipboard remains in memory."
        }),
    }
}

pub(crate) fn discard_pending(confirmed: bool) -> serde_json::Value {
    let Ok(_tx_guard) = TRANSACTION_LOCK.try_lock() else {
        return serde_json::json!({
            "status": "uncertain",
            "message": "A paste is currently in progress. Wait a moment and try again."
        });
    };
    if !confirmed {
        return serde_json::json!({
            "status": "uncertain",
            "message": "Confirm before discarding the saved previous clipboard."
        });
    }
    // If our owner still holds delayed formats from a failed cleanup, materialize them before dropping ACTIVE.
    let active_copy = ACTIVE.lock().unwrap().clone();
    if let Some(d) = active_copy {
        if let Some(owner) = ensure_owner() {
            if let Some(_clip) = OpenClip::acquire(owner) {
                let _ = materialize_if_owned(owner, &d);
                *ACTIVE.lock().unwrap() = None;
            } else {
                return serde_json::json!({
                    "status": "uncertain",
                    "message": "Clipboard is busy; previous clipboard remains in memory."
                });
            }
        }
    }
    let mut pending = PENDING.lock().unwrap();
    if pending.take().is_some() {
        serde_json::json!({
            "status": "resolved",
            "message": "Current clipboard kept; saved previous clipboard discarded."
        })
    } else {
        serde_json::json!({
            "status": "retained",
            "message": "No clipboard recovery is pending."
        })
    }
}

pub(crate) fn prewarm_owner() {
    let _ = ensure_owner();
}

pub(crate) fn shutdown() {
    let state = OWNER.lock().unwrap().take();
    if let Some(mut state) = state {
        if state.hwnd != 0 {
            unsafe { PostMessageW(state.hwnd as HWND, WM_CLOSE, 0, 0) };
        }
        if let Some(handle) = state.thread.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_attribution_handles_arboard_electron_webview2_uwp_and_blocks_foreign_sniffers() {
        // Before Ctrl+V is injected, no reader is attributed as our paste receipt.
        assert!(!should_attribute_reader(false, true, 0, 0, 100, 100, 500));
        assert!(!should_attribute_reader(false, false, 100, 500, 100, 100, 500));
        // After Ctrl+V: arboard / egui-winit uses OpenClipboard(NULL).
        assert!(should_attribute_reader(true, true, 0, 0, 100, 100, 500));
        // Electron / Chromium / Antigravity / VS Code matches target PID.
        assert!(should_attribute_reader(true, false, 100, 999, 100, 100, 500));
        // WebView2 (msedgewebview2.exe) or UWP child opens clipboard from its own HWND_MESSAGE
        // window whose PID equals `focus_pid` (distinct from host `target_pid` and `target_window`).
        assert!(should_attribute_reader(true, false, 200, 777, 100, 200, 500));
        // Hosted child HWND under target root window matches root HWND.
        assert!(should_attribute_reader(true, false, 300, 500, 100, 200, 500));
        // Unrelated background clipboard manager HWND is rejected.
        assert!(!should_attribute_reader(true, false, 999, 888, 100, 200, 500));
    }

    #[test]
    fn snapshot_preserves_windows_privacy_markers_and_skips_ole_internals() {
        for privacy in [
            "CanIncludeInClipboardHistory",
            "CanUploadToCloudClipboard",
            "ExcludeClipboardContentFromMonitorProcessing",
        ] {
            assert!(
                !should_skip_registered_format(privacy),
                "Privacy marker {privacy} must be preserved on restore so password managers stay protected"
            );
        }
        assert!(should_skip_registered_format("Ole Private Data"));
        assert!(should_skip_registered_format("DataObject"));
        assert!(!should_skip_registered_format("HTML Format"));
        assert!(!should_skip_registered_format("Rich Text Format"));
    }

    #[test]
    fn active_payload_normalizes_line_endings_and_encodes_utf16_and_ansi() {
        let target = Target::fixture();
        let active = build_active("Line 1\nLine 2\r\nLine 3", target);
        let expected_units: Vec<u16> = "Line 1\r\nLine 2\r\nLine 3\0".encode_utf16().collect();
        let expected_bytes: Vec<u8> = expected_units.iter().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(active.utf16, expected_bytes);
        assert_eq!(active.ansi.last(), Some(&0));
        assert_eq!(active.oem.last(), Some(&0));
    }

    #[test]
    fn safe_format_filter_excludes_gdi_bitmap_and_metafile_handles() {
        for safe in [CF_TEXT, CF_OEMTEXT, CF_DIB, CF_UNICODETEXT, CF_HDROP, CF_LOCALE, CF_DIBV5] {
            assert!(is_safe_standard_format(safe));
        }
        assert!(!is_safe_standard_format(CF_BITMAP));
        assert!(!is_safe_standard_format(3)); // CF_METAFILEPICT
        assert!(!is_safe_standard_format(14)); // CF_ENHMETAFILE
    }
}
