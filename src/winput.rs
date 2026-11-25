//! Windows-only pieces: the global hold-to-talk key, typing/pasting into the
//! app the person was using, and a single-instance guard.

use crate::storage::Hotkey;

pub(crate) fn locks_recording(held: f32) -> bool {
    held < crate::engine::LOCK_TAP_S
}

pub(crate) fn starts_recording(held: f32) -> bool {
    held >= crate::engine::ANNOUNCE_S
}

pub(crate) fn exceeds_recording_limit(elapsed: f32) -> bool {
    elapsed > crate::engine::MAX_REC_S
}

pub(crate) fn clipboard_sequence() -> Option<u32> {
    #[cfg(windows)]
    { clipboard_win::seq_num().map(std::num::NonZeroU32::get) }
    #[cfg(not(windows))]
    { None }
}

#[cfg(windows)]
pub(crate) fn copy_if_clipboard_unchanged(text: &str, start: Option<u32>) -> bool {
    write_clipboard_if_unchanged(text, start).is_some()
}

#[cfg(windows)]
fn write_clipboard_if_unchanged(text: &str, start: Option<u32>) -> Option<u32> {
    if start.is_none() || text.is_empty() || text.contains('\0') {
        return None;
    }
    let _clipboard = clipboard_win::Clipboard::new_attempts(4).ok()?;
    // Check under the same clipboard lock as the write so a newer copy is preserved.
    if clipboard_sequence() != start {
        return None;
    }
    clipboard_win::raw::set_string(text).ok()?;
    clipboard_sequence()
}

#[cfg(not(windows))]
pub(crate) fn copy_if_clipboard_unchanged(_text: &str, _start: Option<u32>) -> bool {
    false
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Binding {
    Legacy(Hotkey),
    Registered { modifiers: u32, key: u32 },
    Controller(u16),
}

fn binding(value: &str) -> Option<Binding> {
    if let Some(key) = crate::settings_bridge::parse_hotkey(value) {
        return Some(Binding::Legacy(key));
    }
    if let Some(button) = crate::controller::binding(value) {
        return Some(Binding::Controller(button));
    }
    if value.len() > 32 || matches!(value, "alt+f4" | "ctrl+alt+delete") {
        return None;
    }
    let (prefix, trigger) = value.rsplit_once('+').unwrap_or(("", value));
    let modifiers = match prefix {
        "" if value == trigger => 0,
        "alt" => 1,
        "ctrl" => 2,
        "ctrl+alt" => 3,
        "shift" => 4,
        "alt+shift" => 5,
        "ctrl+shift" => 6,
        "ctrl+alt+shift" => 7,
        _ => return None,
    };
    let function = trigger.strip_prefix('f')
        .and_then(|number| number.parse::<u32>().ok())
        .filter(|number| (1..=24).contains(number) && *number != 12
            && trigger == format!("f{number}"));
    let key = if let Some(function) = function {
        0x6F + function
    } else if modifiers == 0 {
        return None;
    } else {
        match trigger {
            "space" => 0x20, "enter" => 0x0D, "backspace" => 0x08,
            "arrow_left" => 0x25, "arrow_up" => 0x26, "arrow_right" => 0x27, "arrow_down" => 0x28,
            "page_up" => 0x21, "page_down" => 0x22, "end" => 0x23, "home" => 0x24,
            "insert" => 0x2D, "delete" => 0x2E,
            _ => match trigger.as_bytes() {
                [byte] if byte.is_ascii_lowercase() || byte.is_ascii_digit() => u32::from(byte.to_ascii_uppercase()),
                _ => return None,
            },
        }
    };
    Some(Binding::Registered { modifiers, key })
}

type Assessment = (&'static str, &'static str);

fn usage_check(value: &str, parsed: Binding) -> Assessment {
    if matches!(parsed, Binding::Controller(_)) {
        return ("warning", "Other apps still receive this button. Use a button they do not need.");
    }
    let Binding::Registered { modifiers, key } = parsed else { return ("clear", "") };
    if matches!(value, "ctrl+1" | "ctrl+2" | "ctrl+3" | "ctrl+4" | "ctrl+k" | "ctrl+s" | "ctrl+enter") {
        return ("warning", "Listen already uses this shortcut. Using it for dictation takes priority.");
    }
    if modifiers & 3 == 3 && ((0x30..=0x5A).contains(&key) || key == 0x20) {
        return ("warning", "This shortcut can override characters typed with AltGr on some keyboards.");
    }
    if matches!(modifiers, 0 | 1 | 4)
        || modifiers == 2 && (b"ACDFHKNOPRSTVWXYZ".iter().any(|letter| u32::from(*letter) == key) || key == 0x74)
    {
        return ("warning", "This shortcut is common in other apps. Using it here can override their shortcut.");
    }
    ("clear", "")
}

#[derive(Debug, PartialEq, Eq)]
pub enum DeliveryError {
    NotSent(String),
    NotSentKeepClipboard(String),
    Uncertain(String),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Delivery {
    Confirmed,
    Sent,
    Copied,
}

impl From<&str> for DeliveryError {
    fn from(message: &str) -> Self {
        Self::NotSent(message.into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    window: isize,
    pid: u32,
    focus_pid: u32,
    thread: u32,
    focus: isize,
}

impl Target {
    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        let pid = std::process::id().wrapping_add(1);
        Self { window: 11, pid, focus_pid: pid, thread: 12, focus: 42 }
    }

    pub fn window(self) -> isize {
        self.window
    }

    pub fn is_ours(self) -> bool {
        self.pid == std::process::id()
    }

    pub fn history_app(self) -> Option<(String, Option<String>)> {
        #[cfg(windows)]
        { imp::history_app(self) }
        #[cfg(not(windows))]
        { None }
    }

    pub fn app_name(self) -> Option<String> {
        #[cfg(windows)]
        { imp::app_name(self) }
        #[cfg(not(windows))]
        { None }
    }
}

#[cfg(any(windows, test))]
fn app_label(path: &std::path::Path) -> Option<String> {
    let name = path.file_stem()?.to_str()?.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return None;
    }
    let label = match name.to_ascii_lowercase().as_str() {
        "code" => "VS Code", "notepad" => "Notepad", "explorer" => "File Explorer",
        "chrome" => "Google Chrome", "msedge" => "Microsoft Edge", "firefox" => "Firefox",
        _ => name,
    };
    Some(label.to_owned())
}

fn admit_delivery(
    target: Target,
    foreground: Option<Target>,
    modifiers_down: bool,
) -> Result<(), &'static str> {
    if target.is_ours() || Some(target) != foreground {
        return Err("The original destination lost focus. Your text was kept in Listen.");
    }
    if modifiers_down {
        return Err("A modifier key is still held. Your text was kept in Listen.");
    }
    Ok(())
}

#[cfg(windows)]
#[path = "clipboard_paste.rs"]
mod clipboard_paste;
#[cfg(windows)]
#[path = "multiline.rs"]
mod multiline;
#[cfg(all(windows, debug_assertions))]
pub use multiline::check_native as check_multiline;

#[cfg(windows)]
mod imp {
    use super::{admit_delivery, binding, usage_check, Assessment, Binding, Delivery, DeliveryError, Hotkey, Target};
    use crate::engine::{Cmd, Core, Phase};
    use super::{exceeds_recording_limit, locks_recording, starts_recording};
    use crate::storage;
    use std::cell::Cell;
    use std::ptr::null_mut;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::{self, SyncSender};
    use std::sync::{Arc, OnceLock};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HWND, ERROR_HOTKEY_ALREADY_REGISTERED};
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
        GUITHREADINFO, HHOOK, IsChild, IsIconic, KBDLLHOOKSTRUCT, LLKHF_INJECTED, PeekMessageW, MSG, PM_REMOVE,
        SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_HOTKEY,
        WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    const VK_ESCAPE: i32 = 0x1B;

    pub fn vk(h: Hotkey) -> i32 {
        match h {
            Hotkey::RightCtrl => 0xA3,
            Hotkey::RightShift => 0xA1,
            Hotkey::RightAlt => 0xA5,
            Hotkey::Pause => 0x13,
            Hotkey::ScrollLock => 0x91,
        }
    }

    fn down(vk: i32) -> bool {
        unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
    }

    const UNAVAILABLE: Assessment = ("blocked", "Windows could not confirm this shortcut. Try again.");
    const INVALID: Assessment = ("blocked", "Choose a supported key combination. Windows reserves some shortcuts.");

    struct Control {
        choice: String,
        acknowledge_warning: Option<bool>,
        expires: Instant,
        reply: SyncSender<Assessment>,
    }

    static HOTKEY_CONTROL: OnceLock<SyncSender<Control>> = OnceLock::new();

    fn configure(core: &Core, choice: &str, acknowledgement: Option<bool>) -> Assessment {
        let Some(parsed) = binding(choice) else { return INVALID };
        if !core.capture_paused.load(Ordering::Acquire) || core.phase() != Phase::Idle {
            return UNAVAILABLE;
        }
        if core.preview {
            let check = usage_check(choice, parsed);
            if acknowledgement.is_some_and(|allow| check.0 == "clear" || allow) {
                write_hotkey(core, choice, parsed).then_some(("saved", "")).unwrap_or(UNAVAILABLE)
            } else {
                check
            }
        } else {
            let Some(worker) = HOTKEY_CONTROL.get() else { return UNAVAILABLE };
            let timeout = Duration::from_secs(2);
            let (reply, receive) = mpsc::sync_channel(1);
            let control = Control {
                choice: choice.to_string(), acknowledge_warning: acknowledgement,
                expires: Instant::now() + timeout, reply,
            };
            if worker.try_send(control).is_err() {
                return UNAVAILABLE;
            }
            receive.recv_timeout(timeout).unwrap_or(UNAVAILABLE)
        }
    }

    pub fn check_hotkey(core: &Core, choice: &str) -> Assessment {
        configure(core, choice, None)
    }

    pub fn save_hotkey(core: &Core, choice: &str, warning_acknowledged: bool) -> bool {
        configure(core, choice, Some(warning_acknowledged)).0 == "saved"
    }

    fn write_hotkey(core: &Core, choice: &str, parsed: Binding) -> bool {
        let mut settings = core.settings.lock().unwrap();
        let mut proposed = settings.clone();
        match parsed {
            Binding::Legacy(key) => {
                proposed.hotkey = key;
                proposed.registered_hotkey = None;
                if proposed.command_hotkey == Some(key) {
                    proposed.command_hotkey = None;
                }
            }
            Binding::Registered { .. } | Binding::Controller(_) => {
                proposed.registered_hotkey = Some(choice.into());
                if proposed.command_hotkey.map(crate::settings_bridge::hotkey_id) == Some(choice) {
                    proposed.command_hotkey = None;
                }
            }
        }
        if !core.preview && storage::save_settings_checked(&proposed).is_err() {
            return false;
        }
        *settings = proposed;
        true
    }

    struct Registration {
        id: i32,
        binding: Binding,
        at: u32,
    }

    impl Registration {
        fn new(id: i32, binding: Binding) -> Result<Self, Assessment> {
            let Binding::Registered { modifiers, key } = binding else { return Err(INVALID) };
            if unsafe { RegisterHotKey(null_mut(), id, modifiers | MOD_NOREPEAT, key) } == 0 {
                return Err(if unsafe { GetLastError() } == ERROR_HOTKEY_ALREADY_REGISTERED {
                    ("blocked", "Another app already registered this shortcut in Windows.")
                } else {
                    UNAVAILABLE
                });
            }
            Ok(Self { id, binding, at: unsafe { GetTickCount() } })
        }

        fn release(self) -> bool {
            unsafe { UnregisterHotKey(null_mut(), self.id) != 0 }
        }
    }

    fn handle_control(
        core: &Core, request: &Control, current: &mut Option<Registration>, next_id: &mut i32,
    ) -> (Assessment, bool) {
        if Instant::now() >= request.expires || !core.capture_paused.load(Ordering::Acquire)
            || core.phase() != Phase::Idle
        {
            return (UNAVAILABLE, false);
        }
        let Some(parsed) = binding(&request.choice) else { return (INVALID, false) };
        let check = usage_check(&request.choice, parsed);
        let reused = current.as_ref().is_some_and(|registration| registration.binding == parsed);
        let proposed = if !reused && matches!(parsed, Binding::Registered { .. }) {
            let id = *next_id;
            *next_id = if id >= 0xBFFF { 1 } else { id + 1 };
            match Registration::new(id, parsed) {
                Ok(registration) => Some(registration),
                Err(check) => return (check, false),
            }
        } else {
            None
        };
        let Some(allow_warning) = request.acknowledge_warning else {
            let failed = proposed.is_some_and(|registration| !registration.release());
            return (if failed { UNAVAILABLE } else { check }, failed);
        };
        let ready = match parsed {
            Binding::Legacy(key) => !down(vk(key)),
            Binding::Registered { key, .. } => !down(key as i32),
            Binding::Controller(button) => !crate::controller::read().into_iter().flatten().any(|pad| pad & button != 0),
        } && !modifiers_down();
        if (check.0 == "warning" && !allow_warning) || !ready || !write_hotkey(core, &request.choice, parsed) {
            let failed = proposed.is_some_and(|registration| !registration.release());
            return (UNAVAILABLE, failed);
        }
        let failed = !reused && std::mem::replace(current, proposed).is_some_and(|old| !old.release());
        (("saved", ""), failed)
    }

    #[derive(Clone, Copy)]
    struct Press {
        registration: usize,
        tick: u32,
        target: Option<Target>,
    }

    thread_local! {
        static OBSERVED_BINDING: Cell<Option<(usize, Binding)>> = const { Cell::new(None) };
        static OBSERVED_PRESS: Cell<Option<Press>> = const { Cell::new(None) };
        static OBSERVED_DOWN: Cell<bool> = const { Cell::new(false) };
    }

    unsafe extern "system" fn observe_shortcut(code: i32, message: usize, data: isize) -> isize {
        if code == 0 && data != 0 {
            let event = unsafe { &*(data as *const KBDLLHOOKSTRUCT) };
            if event.flags & LLKHF_INJECTED == 0 {
                if let Some((registration, Binding::Registered { modifiers, key })) = OBSERVED_BINDING.get() {
                    if event.vkCode == key {
                        match message as u32 {
                            WM_KEYUP | WM_SYSKEYUP => OBSERVED_DOWN.set(false),
                            WM_KEYDOWN | WM_SYSKEYDOWN if !OBSERVED_DOWN.replace(true) => {
                                let active_modifiers = u32::from(down(0x12)) | (u32::from(down(0x11)) << 1)
                                    | (u32::from(down(0x10)) << 2);
                                if active_modifiers == modifiers && !down(0x5B) && !down(0x5C) {
                                    OBSERVED_PRESS.set(Some(Press { registration, tick: event.time, target: capture_target() }));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        unsafe { CallNextHookEx(null_mut(), code, message, data) }
    }

    struct ShortcutObserver(HHOOK);

    impl Drop for ShortcutObserver {
        fn drop(&mut self) {
            if !self.0.is_null() { unsafe { UnhookWindowsHookEx(self.0) }; }
        }
    }

    fn matching_target(press: Option<Press>, id: usize, tick: u32) -> Option<Target> {
        press.filter(|press| press.registration == id && press.tick == tick).and_then(|press| press.target)
    }

    #[test]
    fn an_unmatched_shortcut_never_uses_the_later_foreground() {
        let target = Target { window: 1, pid: 2, focus_pid: 2, thread: 3, focus: 4 };
        let observed = Some(Press { registration: 8, tick: 120, target: Some(target) });
        assert_eq!(matching_target(observed, 8, 120), Some(target));
        assert_eq!(matching_target(observed, 9, 120), None);
        assert_eq!(matching_target(observed, 8, 121), None);
        assert_eq!(matching_target(None, 8, 120), None);
    }

    fn received_press(registration: Option<&Registration>) -> Option<Option<Target>> {
        let mut pressed = None;
        let mut event: MSG = unsafe { std::mem::zeroed() };
        while unsafe { PeekMessageW(&mut event, null_mut(), WM_HOTKEY, WM_HOTKEY, PM_REMOVE) } != 0 {
            if let Some(registration) = registration {
                let packed = event.lParam as u32;
                let event_binding = Binding::Registered { modifiers: packed & 0xF, key: packed >> 16 };
                let age = unsafe { GetTickCount() }.wrapping_sub(event.time);
                let newer = event.time.wrapping_sub(registration.at);
                if event.wParam == registration.id as usize && age <= 100
                    && registration.binding == event_binding && (1..0x8000_0000).contains(&newer) {
                    // shortcut: unmatched Windows and physical events can still record for manual recovery.
                    pressed = Some(matching_target(OBSERVED_PRESS.take(), event.wParam, event.time));
                }
            }
        }
        pressed
    }

    pub fn capture_target() -> Option<Target> {
        let window = unsafe { GetForegroundWindow() };
        if window.is_null() || unsafe { IsIconic(window) } != 0 {
            return None;
        }
        let mut class = [0u16; 256];
        let count = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
        if count == 0 || matches!(String::from_utf16_lossy(&class[..count as usize]).as_str(),
            "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "NotifyIconOverflowWindow" |
            "TopLevelWindowForOverflowXamlIsland" | "Progman" | "WorkerW") {
            return None;
        }
        let mut pid = 0;
        let thread = unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if thread == 0 || pid == 0 {
            return None;
        }
        let mut info: GUITHREADINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        let have_info = unsafe { GetGUIThreadInfo(thread, &mut info) } != 0;
        if have_info && info.flags & (0x0004 | 0x0008 | 0x0010) != 0 {
            return None;
        }
        let mut focus = if have_info { info.hwndFocus } else { null_mut() };
        // WebView2 / XAML islands / embedded cross-thread child windows attach keyboard focus
        // to the child thread, leaving the top-level window's thread `hwndFocus` null.
        if focus.is_null() {
            let mut fg_info: GUITHREADINFO = unsafe { std::mem::zeroed() };
            fg_info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
            if unsafe { GetGUIThreadInfo(0, &mut fg_info) } != 0 {
                if fg_info.flags & (0x0004 | 0x0008 | 0x0010) != 0 {
                    return None;
                }
                if !fg_info.hwndFocus.is_null()
                    && (fg_info.hwndFocus == window || unsafe { IsChild(window, fg_info.hwndFocus) } != 0)
                {
                    focus = fg_info.hwndFocus;
                }
            }
        }
        // Modern GPU/Electron/custom windows may leave `hwndFocus` null or point to an owned
        // overlay while `window` itself is the active foreground input target.
        if focus.is_null() || (focus != window && unsafe { IsChild(window, focus) } == 0) {
            focus = window;
        }
        if unsafe { GetForegroundWindow() } != window {
            return None;
        }
        let mut focus_pid = 0;
        let _ = unsafe { GetWindowThreadProcessId(focus, &mut focus_pid) };
        if focus_pid == 0 {
            focus_pid = pid;
        }
        Some(Target { window: window as isize, pid, focus_pid, thread, focus: focus as isize })
    }

    fn target_executable(target: Target) -> Option<std::path::PathBuf> {
        let still_owned = || {
            let mut pid = 0;
            let thread = unsafe { GetWindowThreadProcessId(target.window as HWND, &mut pid) };
            thread == target.thread && pid == target.pid
        };
        if !still_owned() { return None; }
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, target.pid) };
        if process.is_null() { return None; }
        let mut path = vec![0u16; 32768];
        let mut length = path.len() as u32;
        let queried = unsafe {
            let result = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length);
            CloseHandle(process);
            result != 0
        };
        if !queried || !still_owned() { return None; }
        String::from_utf16(&path[..length as usize]).ok().map(Into::into)
    }

    pub(super) fn app_name(target: Target) -> Option<String> {
        let path = target_executable(target)?;
        super::app_label(&path)
    }

    pub(super) fn history_app(target: Target) -> Option<(String, Option<String>)> {
        let path = target_executable(target)?;
        let name = super::app_label(&path)?;
        let icon = crate::history_icon::from_executable(&path);
        let mut pid = 0;
        let thread = unsafe { GetWindowThreadProcessId(target.window as HWND, &mut pid) };
        (thread == target.thread && pid == target.pid).then_some((name, icon))
    }

    /// Keys that count as "another key" while the hotkey is held (so Right Ctrl+C stays a shortcut).
    fn other_keys(hot: i32) -> [bool; 256] {
        let mut out = [false; 256];
        for k in 0x08..=0xFEi32 {
            if k == hot || (0x10..=0x12).contains(&k) {
                continue;
            }
            out[k as usize] = down(k);
        }
        out
    }

    enum St {
        Idle,
        Pressed { at: Instant, trigger_up: Option<Instant>, announced: bool, target: Option<Target>, before: [bool; 256] },
        Locked { since: Instant },
        Finishing,
        WaitRelease,
    }

    pub fn spawn_hotkey_thread(core: Arc<Core>) {
        let (control, requests) = mpsc::sync_channel::<Control>(1);
        if HOTKEY_CONTROL.set(control).is_err() {
            return;
        }
        std::thread::Builder::new()
            .name("hotkey".into())
            .spawn(move || {
                let _observer = ShortcutObserver(unsafe {
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(observe_shortcut), GetModuleHandleW(std::ptr::null()), 0)
                });
                // A saved chord that Windows rejects must not quietly fall back to a different key.
                let saved = core.settings.lock().unwrap().registered_hotkey.clone();
                let registered = saved.as_deref().and_then(binding).filter(|choice| matches!(choice, Binding::Registered { .. }));
                let mut current = registered
                    .and_then(|parsed| Registration::new(1, parsed).ok());
                if registered.is_some() && current.is_none() {
                    core.set_shortcut_error("Windows could not start the saved shortcut. Open Settings to choose another.");
                }
                let mut next_id = 2;
                let mut st = St::WaitRelease;
                let mut was_down = false;
                let mut cmd_st = St::WaitRelease;
                let mut cmd_was_down = false;
                let mut pad = crate::controller::Gesture::default();
                loop {
                    std::thread::sleep(Duration::from_millis(8));
                    if let Ok(request) = requests.try_recv() {
                        let (result, stop) = handle_control(&core, &request, &mut current, &mut next_id);
                        let _ = request.reply.send(result);
                        if stop {
                            if let Some(registration) = current.take() { registration.release(); }
                            core.set_shortcut_error("Windows could not update the shortcut. Reopen Listen to try again.");
                            return;
                        }
                        if result.0 == "saved" { core.set_shortcut_error(""); }
                    }
                    let active = current.as_ref().map(|r| r.binding);
                    let observation = current.as_ref().map(|r| (r.id as usize, r.binding));
                    if OBSERVED_BINDING.replace(observation) != observation {
                        OBSERVED_PRESS.set(None);
                        OBSERVED_DOWN.set(false);
                    }
                    let (hot, registered, required_modifiers, pad_button, cmd_hot) = {
                        let settings = core.settings.lock().unwrap();
                        let cmd_hot = settings
                            .command_hotkey
                            .filter(|&ch| {
                                settings.registered_hotkey.as_deref()
                                    != Some(crate::settings_bridge::hotkey_id(ch))
                                    && (settings.registered_hotkey.is_some() || ch != settings.hotkey)
                            })
                            .map(vk)
                            .unwrap_or(0);
                        match active {
                            Some(Binding::Registered { modifiers, key }) => {
                                (key as i32, true, modifiers, None, cmd_hot)
                            }
                            _ => {
                                let hot = (settings.registered_hotkey.is_none())
                                    .then(|| vk(settings.hotkey))
                                    .unwrap_or(0);
                                let button = settings
                                    .registered_hotkey
                                    .as_deref()
                                    .and_then(crate::controller::binding);
                                (hot, false, 0, button, cmd_hot)
                            }
                        }
                    };
                    let message_edge = received_press(current.as_ref());
                    if core.capture_paused.load(Ordering::Acquire) {
                        if let Some(capture) = core.controller_capture.lock().unwrap().as_mut() {
                            if capture.poll(crate::controller::read()) { core.repaint(); }
                        }
                        st = St::WaitRelease;
                        cmd_st = St::WaitRelease;
                        pad = crate::controller::Gesture::default();
                        was_down = true;
                        cmd_was_down = true;
                        continue;
                    }
                    let (controller_down, cancel_controller) = pad_button.map_or((false, false), |button| {
                        pad.poll(crate::controller::read(), button)
                    });
                    if cancel_controller {
                        if core.phase() == Phase::Recording || matches!(st, St::Pressed { announced: true, .. }) {
                            core.send(Cmd::Cancel);
                        }
                        st = St::WaitRelease;
                    }
                    let trigger_down = controller_down || (hot != 0 && down(hot));
                    let held_modifiers = [(1, 0x12), (2, 0x11), (4, 0x10)].iter()
                        .any(|(flag, key)| required_modifiers & flag != 0 && down(*key));
                    let is_down = trigger_down || held_modifiers;
                    let edge = if registered { message_edge.is_some() } else { trigger_down && !was_down };
                    was_down = is_down;

                    let cmd_down = cmd_hot != 0 && down(cmd_hot);
                    let cmd_edge = cmd_down && !cmd_was_down;
                    cmd_was_down = cmd_down;
                    if cmd_hot == 0 {
                        cmd_st = St::Idle;
                    }

                    let phase = core.phase();

                    // Safety limit, whichever way recording started.
                    if phase == Phase::Recording {
                        let started = core.shared.lock().unwrap().started;
                        if started.is_some_and(|t| exceeds_recording_limit(t.elapsed().as_secs_f32())) {
                            core.send(Cmd::Finish);
                            st = St::Idle;
                            cmd_st = St::Idle;
                            continue;
                        }
                    }

                    let cmd_busy = !matches!(cmd_st, St::Idle);
                    st = match st {
                        St::Idle => {
                            if edge && !cmd_busy {
                                match phase {
                                    Phase::Idle => St::Pressed {
                                        at: Instant::now(),
                                        trigger_up: (!trigger_down).then(Instant::now),
                                        announced: false,
                                        target: if registered { message_edge.flatten() } else { capture_target() },
                                        before: if pad_button.is_some() { [false; 256] } else { other_keys(hot) },
                                    },
                                    Phase::Recording => St::Finishing,
                                    Phase::Working => St::WaitRelease,
                                }
                            } else {
                                if !cmd_busy && phase == Phase::Recording && down(VK_ESCAPE) {
                                    core.send(Cmd::Cancel);
                                }
                                St::Idle
                            }
                        }
                        St::Pressed { at, mut trigger_up, mut announced, target, before } => {
                            if trigger_down {
                                trigger_up = None;
                            } else if trigger_up.is_none() {
                                trigger_up = Some(Instant::now());
                            }
                            let shortcut = pad_button.is_none() && other_keys(hot).iter().enumerate()
                                .any(|(key, &pressed)| pressed && !before[key] && key != VK_ESCAPE as usize);
                            let held = trigger_up.unwrap_or_else(Instant::now).duration_since(at).as_secs_f32();
                            if shortcut || down(VK_ESCAPE) {
                                if announced {
                                    core.send(Cmd::Cancel);
                                }
                                St::WaitRelease
                            } else if !is_down {
                                if locks_recording(held) {
                                    if !announced {
                                        core.send(Cmd::Start { target, hands_free: true });
                                    } else {
                                        core.send(Cmd::MarkHandsFree);
                                    }
                                    St::Locked { since: Instant::now() }
                                } else {
                                    core.send(Cmd::Finish);
                                    St::Idle
                                }
                            } else {
                                if trigger_up.is_none() && !announced && starts_recording(held) {
                                    core.send(Cmd::Start { target, hands_free: false });
                                    announced = true;
                                }
                                St::Pressed { at, trigger_up, announced, target, before }
                            }
                        }
                        St::Locked { since } => {
                            if phase == Phase::Idle && since.elapsed().as_secs_f32() > 0.6 {
                                St::Idle // recording was refused, finished or cancelled elsewhere
                            } else if down(VK_ESCAPE) {
                                core.send(Cmd::Cancel);
                                St::Idle
                            } else if edge {
                                St::Finishing
                            } else {
                                St::Locked { since }
                            }
                        }
                        St::Finishing => {
                            if down(VK_ESCAPE) {
                                core.send(Cmd::Cancel);
                                St::WaitRelease
                            } else if is_down {
                                St::Finishing
                            } else {
                                core.send(Cmd::Finish);
                                St::Idle
                            }
                        }
                        St::WaitRelease => {
                            if is_down {
                                St::WaitRelease
                            } else {
                                St::Idle
                            }
                        }
                    };

                    if cmd_hot != 0 {
                        let dictate_busy = !matches!(st, St::Idle);
                        cmd_st = match cmd_st {
                            St::Idle => {
                                if cmd_edge && !dictate_busy {
                                    match phase {
                                        Phase::Idle => St::Pressed {
                                            at: Instant::now(),
                                            trigger_up: (!cmd_down).then(Instant::now),
                                            announced: false,
                                            target: capture_target(),
                                            before: other_keys(cmd_hot),
                                        },
                                        Phase::Recording => St::Finishing,
                                        Phase::Working => St::WaitRelease,
                                    }
                                } else {
                                    St::Idle
                                }
                            }
                            St::Pressed { at, mut trigger_up, mut announced, target, before } => {
                                if cmd_down {
                                    trigger_up = None;
                                } else if trigger_up.is_none() {
                                    trigger_up = Some(Instant::now());
                                }
                                let shortcut = other_keys(cmd_hot).iter().enumerate()
                                    .any(|(key, &pressed)| pressed && !before[key] && key != VK_ESCAPE as usize);
                                let held = trigger_up.unwrap_or_else(Instant::now).duration_since(at).as_secs_f32();
                                if shortcut || down(VK_ESCAPE) {
                                    if announced {
                                        core.send(Cmd::Cancel);
                                    }
                                    St::WaitRelease
                                } else if !cmd_down {
                                    if locks_recording(held) {
                                        if !announced {
                                            core.send(Cmd::StartCommand {
                                                target,
                                                selected_text: None,
                                                hands_free: true,
                                            });
                                        } else {
                                            core.send(Cmd::MarkHandsFree);
                                        }
                                        St::Locked { since: Instant::now() }
                                    } else {
                                        core.send(Cmd::Finish);
                                        St::Idle
                                    }
                                } else {
                                    if trigger_up.is_none() && !announced && starts_recording(held) {
                                        core.send(Cmd::StartCommand {
                                            target,
                                            selected_text: None,
                                            hands_free: false,
                                        });
                                        announced = true;
                                    }
                                    St::Pressed { at, trigger_up, announced, target, before }
                                }
                            }
                            St::Locked { since } => {
                                if phase == Phase::Idle && since.elapsed().as_secs_f32() > 0.6 {
                                    St::Idle
                                } else if down(VK_ESCAPE) {
                                    core.send(Cmd::Cancel);
                                    St::Idle
                                } else if cmd_edge {
                                    St::Finishing
                                } else {
                                    St::Locked { since }
                                }
                            }
                            St::Finishing => {
                                if down(VK_ESCAPE) {
                                    core.send(Cmd::Cancel);
                                    St::WaitRelease
                                } else if cmd_down {
                                    St::Finishing
                                } else {
                                    core.send(Cmd::Finish);
                                    St::Idle
                                }
                            }
                            St::WaitRelease => {
                                if cmd_down {
                                    St::WaitRelease
                                } else {
                                    St::Idle
                                }
                            }
                        };
                    }
                }
            })
            .expect("hotkey thread");
    }

    pub(super) fn modifiers_down() -> bool {
        [0x10, 0x11, 0x12, 0x5B, 0x5C, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5]
            .iter()
            .any(|k| down(*k))
    }

    const MAX_UNICODE_UNITS: usize = 4096;
    const MAX_CLIPBOARD_UNITS: usize = 100_000;

    fn can_send_unicode(text: &str) -> bool {
        !text.contains(['\r', '\n', '\t'])
            && text.chars().all(|c| (c as u32) <= 0xFFFF)
            && text.encode_utf16().count() <= MAX_UNICODE_UNITS
    }

    fn send_unicode_real(
        text: &str,
        target: Target,
        ready: impl FnOnce() -> Result<(), DeliveryError>,
    ) -> Result<Delivery, DeliveryError> {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
            KEYEVENTF_UNICODE,
        };
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut events = Vec::with_capacity(units.len() * 2);
        for unit in units {
            for flags in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP] {
                events.push(INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT { wVk: 0, wScan: unit, dwFlags: flags, time: 0, dwExtraInfo: 0 },
                    },
                });
            }
        }
        ready()?;
        // Final hardware-level guard right before SendInput.
        if unsafe { GetForegroundWindow() } != target.window as HWND {
            return Err("The original destination lost focus. Your text was kept in Listen.".into());
        }
        let expected = events.len() as u32;
        let accepted = unsafe {
            SendInput(expected, events.as_ptr(), std::mem::size_of::<INPUT>() as i32)
        };
        if accepted == expected {
            Ok(Delivery::Sent)
        } else if accepted == 0 {
            let code = unsafe { GetLastError() };
            Err(DeliveryError::NotSent(format!(
                "Windows did not accept text entry (error {code}). Your text is saved in Listen."
            )))
        } else {
            Err(DeliveryError::Uncertain(
                "Some text may already be in the destination. Check it before copying again.".into(),
            ))
        }
    }

    pub(crate) fn deliver_draft(
        text: &str,
        target: Option<Target>,
        allow_ctrl_v: bool,
        restore_clipboard: bool,
    ) -> Result<Delivery, DeliveryError> {
        deliver_with_focus(text, target, allow_ctrl_v, restore_clipboard, || (capture_target(), modifiers_down()))
    }

    pub(crate) fn deliver_dictation(
        text: &str,
        target: Option<Target>,
        allow_ctrl_v: bool,
        restore_clipboard: bool,
    ) -> Result<Delivery, DeliveryError> {
        deliver_with_focus(text, target, allow_ctrl_v, restore_clipboard, || (capture_target(), modifiers_down()))
    }

    pub(super) fn deliver_with_focus(
        text: &str,
        target: Option<Target>,
        allow_ctrl_v: bool,
        restore_clipboard: bool,
        focus: impl Fn() -> (Option<Target>, bool),
    ) -> Result<Delivery, DeliveryError> {
        deliver_with_transport(
            text,
            target,
            allow_ctrl_v,
            restore_clipboard,
            &focus,
            |text, target, ready| send_unicode_real(text, target, ready),
            |text, target, restore, ready| super::clipboard_paste::deliver(text, target, restore, ready),
        )
    }

    fn deliver_with_transport(
        text: &str,
        target: Option<Target>,
        allow_ctrl_v: bool,
        restore_clipboard: bool,
        focus: &dyn Fn() -> (Option<Target>, bool),
        send_unicode: impl FnOnce(&str, Target, &dyn Fn() -> Result<(), DeliveryError>) -> Result<Delivery, DeliveryError>,
        send_clipboard: impl FnOnce(&str, Target, bool, &dyn Fn() -> Result<(), DeliveryError>) -> Result<Delivery, DeliveryError>,
    ) -> Result<Delivery, DeliveryError> {
        let target = target.ok_or("No verified destination. Your text was kept in Listen.")?;
        let count = text.encode_utf16().count();
        if count == 0
            || count > MAX_CLIPBOARD_UNITS
            || text.chars().any(|c| {
                (c.is_control() && !matches!(c, '\r' | '\n' | '\t')) || matches!(c, '\u{2028}' | '\u{2029}')
            })
        {
            return Err("This text needs manual paste. It was kept in Listen.".into());
        }
        let started = Instant::now();
        loop {
            let (foreground, modifiers) = focus();
            admit_delivery(target, foreground, false)?;
            if !modifiers || started.elapsed() >= Duration::from_millis(1500) {
                break;
            }
            std::thread::sleep(Duration::from_millis(4));
        }
        let ready = || {
            let (foreground, modifiers) = focus();
            admit_delivery(target, foreground, modifiers).map_err(DeliveryError::from)
        };
        ready()?;

        // Tier 1: Direct verified edit in classic Win32 Edit or plain-text Unicode RichEdit.
        match super::multiline::try_deliver(text, target, ready) {
            super::multiline::DirectResult::Confirmed => return Ok(Delivery::Confirmed),
            super::multiline::DirectResult::Failed(error) => return Err(error),
            super::multiline::DirectResult::Unsupported => {}
        }

        // Tier 2 (default for modern apps): Instant delayed-render Ctrl+V paste with
        // receipt attribution and automatic clipboard snapshot/restore.
        if allow_ctrl_v {
            match send_clipboard(text, target, restore_clipboard, &ready) {
                Err(DeliveryError::NotSentKeepClipboard(_))
                    if !clipboard_recovery_pending() && can_send_unicode(text) =>
                {
                    // Clipboard held an unsnapshottable format (e.g. Office GDI/huge image)
                    // before anything was staged; fall back to direct Unicode typing so
                    // single-line dictation still lands without touching the clipboard.
                    return send_unicode(text, target, &ready);
                }
                other => return other,
            }
        }

        // Tier 3 (fallback when Ctrl+V is bound as the dictation hotkey): Single-line BMP
        // text via direct Unicode keystrokes so dictation still works without Ctrl+V.
        if can_send_unicode(text) {
            return send_unicode(text, target, &ready);
        }
        Err("Automatic Ctrl+V is paused while Ctrl+V is your dictation shortcut. Your text is saved in Listen.".into())
    }

    pub(crate) fn delivery_status() -> serde_json::Value {
        super::clipboard_paste::pending_status()
    }

    pub(crate) fn clipboard_recovery_pending() -> bool {
        super::clipboard_paste::has_pending()
    }

    pub(crate) type CommandClipboardSnapshot = super::clipboard_paste::CommandClipboardSnapshot;

    pub(crate) fn snapshot_clipboard_for_command(
        restore_clipboard: bool,
    ) -> Result<Option<CommandClipboardSnapshot>, String> {
        super::clipboard_paste::snapshot_for_command(restore_clipboard)
    }

    pub(crate) fn restore_clipboard_after_command(
        snapshot: Option<CommandClipboardSnapshot>,
        post_copy_seq: Option<u32>,
    ) {
        super::clipboard_paste::restore_after_command(snapshot, post_copy_seq);
    }

    pub(crate) fn restore_delivery_clipboard(confirmed: bool) -> serde_json::Value {
        super::clipboard_paste::restore_pending(confirmed)
    }

    pub(crate) fn discard_delivery_clipboard(confirmed: bool) -> serde_json::Value {
        super::clipboard_paste::discard_pending(confirmed)
    }

    pub(crate) fn prewarm_clipboard() {
        super::clipboard_paste::prewarm_owner();
    }

    pub(crate) fn shutdown_clipboard() {
        super::clipboard_paste::shutdown();
    }

    #[test]
    fn modern_window_receives_single_line_dictation_without_classic_edit_control() {
        use windows_sys::w;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WS_EX_NOACTIVATE, WS_POPUP, WS_VISIBLE,
        };
        struct TestWindow(HWND);
        impl Drop for TestWindow {
            fn drop(&mut self) {
                if !self.0.is_null() {
                    unsafe { DestroyWindow(self.0) };
                }
            }
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE, w!("STATIC"), w!("Modern App Surface"),
                WS_POPUP | WS_VISIBLE, -30_000, -30_000, 320, 120,
                null_mut(), null_mut(), null_mut(), std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null());
        let _guard = TestWindow(hwnd);
        let mut pid = 0;
        let thread = unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        // Non-self pid simulates an external modern app (Antigravity / VS Code / Craft Anything)
        // whose focused HWND is the top-level surface rather than a classic Win32 Edit control.
        let ext_pid = pid.wrapping_add(1);
        let external = Target { window: hwnd as isize, focus: hwnd as isize, thread, pid: ext_pid, focus_pid: ext_pid };

        // Default modern-app path: single-line dictation pastes instantaneously via Ctrl+V
        // (never visibly typing character-by-character).
        let pasted = std::cell::RefCell::new((String::new(), false));
        let focus = || (Some(external), false);
        let result = deliver_with_transport(
            "Hello from dictation. ",
            Some(external),
            true,
            true,
            &focus,
            |_, _, _| panic!("Modern apps should use instant clipboard paste when Ctrl+V is allowed"),
            |text, target, restore, ready| {
                ready()?;
                assert_eq!(target, external);
                *pasted.borrow_mut() = (text.to_owned(), restore);
                Ok(Delivery::Confirmed)
            },
        );
        assert_eq!(result, Ok(Delivery::Confirmed));
        assert_eq!(*pasted.borrow(), ("Hello from dictation. ".to_owned(), true));

        // Fallback when Ctrl+V is bound as the dictation hotkey: single-line BMP still delivers via Unicode.
        let unicode_received = std::cell::RefCell::new(String::new());
        let fallback = deliver_with_transport(
            "Fallback when ctrl+v is hotkey. ",
            Some(external),
            false,
            true,
            &focus,
            |text, target, ready| {
                ready()?;
                assert_eq!(target, external);
                *unicode_received.borrow_mut() = text.to_owned();
                Ok(Delivery::Sent)
            },
            |_, _, _, _| panic!("Must not send Ctrl+V when allow_ctrl_v is false"),
        );
        assert_eq!(fallback, Ok(Delivery::Sent));
        assert_eq!(unicode_received.borrow().as_str(), "Fallback when ctrl+v is hotkey. ");

        // Fallback when clipboard cannot be snapshotted (e.g. Office GDI/large image) before staging.
        let gdi_fallback_text = std::cell::RefCell::new(String::new());
        let gdi_fallback = deliver_with_transport(
            "Keep Office clipboard safe. ",
            Some(external),
            true,
            true,
            &focus,
            |text, target, ready| {
                ready()?;
                assert_eq!(target, external);
                *gdi_fallback_text.borrow_mut() = text.to_owned();
                Ok(Delivery::Sent)
            },
            |_, _, _, _| Err(DeliveryError::NotSentKeepClipboard("Clipboard kept untouched.".into())),
        );
        assert_eq!(gdi_fallback, Ok(Delivery::Sent));
        assert_eq!(gdi_fallback_text.borrow().as_str(), "Keep Office clipboard safe. ");
    }

    #[test]
    fn multiline_and_emoji_dictation_routes_to_clipboard_paste_with_restore_flag() {
        use windows_sys::w;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WS_EX_NOACTIVATE, WS_POPUP, WS_VISIBLE,
        };
        struct TestWindow(HWND);
        impl Drop for TestWindow {
            fn drop(&mut self) {
                if !self.0.is_null() {
                    unsafe { DestroyWindow(self.0) };
                }
            }
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE, w!("STATIC"), w!("Modern App Multiline"),
                WS_POPUP | WS_VISIBLE, -30_000, -30_000, 320, 120,
                null_mut(), null_mut(), null_mut(), std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null());
        let _guard = TestWindow(hwnd);
        let mut pid = 0;
        let thread = unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        let ext_pid = pid.wrapping_add(1);
        let external = Target { window: hwnd as isize, focus: hwnd as isize, thread, pid: ext_pid, focus_pid: ext_pid };
        let focus = || (Some(external), false);

        for sample in ["Shopping:\n- apples\n- milk", "Ship it 🚀", "col1\tcol2"] {
            let pasted = std::cell::RefCell::new((String::new(), false));
            let result = deliver_with_transport(
                sample,
                Some(external),
                true,
                true,
                &focus,
                |_, _, _| panic!("Multiline or emoji text must use clipboard paste, not Unicode keystrokes"),
                |text, target, restore, ready| {
                    ready()?;
                    assert_eq!(target, external);
                    *pasted.borrow_mut() = (text.to_owned(), restore);
                    Ok(Delivery::Confirmed)
                },
            );
            assert_eq!(result, Ok(Delivery::Confirmed));
            assert_eq!(*pasted.borrow(), (sample.to_owned(), true));
        }

        // Focus switch blocks both Unicode and clipboard paste.
        let shifted = Target { window: external.window.wrapping_add(1), ..external };
        let lost_focus = || (Some(shifted), false);
        let blocked = deliver_with_transport(
            "Do not send",
            Some(external),
            true,
            true,
            &lost_focus,
            |_, _, _| panic!("Must never send after focus changed"),
            |_, _, _, _| panic!("Must never paste after focus changed"),
        );
        assert!(matches!(blocked, Err(DeliveryError::NotSent(_))));

        // Ctrl+V hotkey pauses automatic Ctrl+V on multiline text.
        let paused = deliver_with_transport(
            "Line 1\nLine 2",
            Some(external),
            false,
            true,
            &focus,
            |_, _, _| panic!("Multiline text cannot use Unicode typing"),
            |_, _, _, _| panic!("Ctrl+V must stay paused when Ctrl+V is the hotkey"),
        );
        assert!(matches!(paused, Err(DeliveryError::NotSent(_))));
    }

    /// Auto-reset event signaled when a second Listen.exe launch asks the running instance to show its window.
    static SHOW_EVENT: std::sync::OnceLock<isize> = std::sync::OnceLock::new();

    /// Returns false when another copy is already running (and signals it to show its window).
    pub fn single_instance() -> bool {
        use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
        use windows_sys::Win32::System::Threading::{CreateEventW, CreateMutexW, OpenEventW, SetEvent, EVENT_MODIFY_STATE};
        let name: Vec<u16> = "Local\\ListenRsSingleInstance\0".encode_utf16().collect();
        let show_name: Vec<u16> = "Local\\ListenRsShowWindow\0".encode_utf16().collect();
        unsafe {
            let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if !h.is_null() && GetLastError() != ERROR_ALREADY_EXISTS {
                let ev = CreateEventW(std::ptr::null(), 0, 0, show_name.as_ptr());
                if !ev.is_null() {
                    let _ = SHOW_EVENT.set(ev as isize);
                }
                true
            } else {
                let ev = OpenEventW(EVENT_MODIFY_STATE, 0, show_name.as_ptr());
                if !ev.is_null() {
                    SetEvent(ev);
                    CloseHandle(ev);
                }
                false
            }
        }
    }

    /// Returns true once whenever another Listen.exe launch asked to bring this window to the front.
    pub fn take_show_request() -> bool {
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        let Some(&ev) = SHOW_EVENT.get() else { return false };
        unsafe { WaitForSingleObject(ev as windows_sys::Win32::Foundation::HANDLE, 0) == WAIT_OBJECT_0 }
    }

    /// Where the window was before it was tucked away, so "Open Listen" puts it back.
    static PARKED: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);

    /// "Hide" the main window (close → hide; Quit lives in the tray).
    ///
    /// A truly hidden window (SW_HIDE) stops getting paint messages, which froze the app's frame
    /// loop (measured: 2 frames in 3 s), and with it the floating pill and the tray icon. So the
    /// window is instead taken off the taskbar and Alt+Tab (tool-window style) and parked just
    /// outside every monitor, where it keeps running.
    pub fn hide_window(hwnd: isize) {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        if hwnd == 0 || is_parked() {
            return;
        }
        unsafe {
            let h = hwnd as HWND;
            if IsIconic(h) != 0 {
                ShowWindow(h, SW_RESTORE);
            }
            let mut r: windows_sys::Win32::Foundation::RECT = std::mem::zeroed();
            GetWindowRect(h, &mut r);
            *PARKED.lock().unwrap() = Some((r.left, r.top));
            ShowWindow(h, SW_HIDE);
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            SetWindowLongPtrW(h, GWL_EXSTYLE, (ex | WS_EX_TOOLWINDOW as isize) & !(WS_EX_APPWINDOW as isize));
            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let w = r.right - r.left;
            SetWindowPos(h, std::ptr::null_mut(), vx - w - 200, vy, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            ShowWindow(h, SW_SHOWNOACTIVATE);
        }
    }

    /// Bring the window back where it was, on the taskbar, in front ("Open Listen" in the tray).
    pub fn show_window(hwnd: isize) {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        if hwnd == 0 {
            return;
        }
        unsafe {
            let h = hwnd as HWND;
            // Showing or moving a window sends other Windows messages synchronously.
            let parked = PARKED.lock().unwrap().take();
            if let Some((x, y)) = parked {
                ShowWindow(h, SW_HIDE);
                let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
                SetWindowLongPtrW(h, GWL_EXSTYLE, ex & !(WS_EX_TOOLWINDOW as isize));
                SetWindowPos(h, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
            ShowWindow(h, if IsIconic(h) != 0 { SW_RESTORE } else { SW_SHOW });
            SetForegroundWindow(h);
        }
    }

    /// True while the window is tucked away by `hide_window`.
    pub fn is_parked() -> bool {
        PARKED.lock().unwrap().is_some()
    }

    pub fn window_visible(hwnd: isize) -> bool {
        use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;
        hwnd != 0 && unsafe { IsWindowVisible(hwnd as HWND) != 0 } && !is_parked()
    }

    pub(crate) fn show_quit_error(hwnd: isize) {
        use windows_sys::w;
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        unsafe { MessageBoxW(hwnd as HWND,
            w!("Listen could not save and close. Copy any unsaved text, then try closing again."),
            w!("Listen"), MB_OK | MB_ICONERROR) };
    }
}

#[cfg(windows)]
pub use imp::*;

#[cfg(not(windows))]
mod imp {
    use super::{Delivery, DeliveryError, Hotkey, Target};
    use crate::engine::Core;
    use std::sync::Arc;
    pub fn vk(_h: Hotkey) -> i32 {
        0
    }
    pub fn capture_target() -> Option<Target> {
        None
    }
    pub fn spawn_hotkey_thread(_core: Arc<Core>) {}
    pub(crate) fn deliver_draft(_text: &str, _target: Option<Target>, _allow_ctrl_v: bool, _restore_clipboard: bool) -> Result<Delivery, DeliveryError> {
        Err("typing into other apps only works on Windows".into())
    }
    pub(crate) fn deliver_dictation(text: &str, target: Option<Target>, allow_ctrl_v: bool, restore_clipboard: bool) -> Result<Delivery, DeliveryError> {
        deliver_draft(text, target, allow_ctrl_v, restore_clipboard)
    }
    pub(crate) fn delivery_status() -> serde_json::Value {
        serde_json::json!({ "pending": false, "message": "" })
    }
    pub(crate) fn clipboard_recovery_pending() -> bool {
        false
    }
    pub(crate) type CommandClipboardSnapshot = ();
    pub(crate) fn snapshot_clipboard_for_command(
        _restore_clipboard: bool,
    ) -> Result<Option<CommandClipboardSnapshot>, String> {
        Ok(None)
    }
    pub(crate) fn restore_clipboard_after_command(
        _snapshot: Option<CommandClipboardSnapshot>,
        _post_copy_seq: Option<u32>,
    ) {
    }
    pub(crate) fn restore_delivery_clipboard(_confirmed: bool) -> serde_json::Value {
        serde_json::json!({ "status": "retained", "message": "No clipboard recovery is pending." })
    }
    pub(crate) fn discard_delivery_clipboard(_confirmed: bool) -> serde_json::Value {
        serde_json::json!({ "status": "retained", "message": "No clipboard recovery is pending." })
    }
    pub(crate) fn prewarm_clipboard() {}
    pub(crate) fn shutdown_clipboard() {}
    pub fn single_instance() -> bool {
        true
    }
    pub fn take_show_request() -> bool {
        false
    }
    pub fn hide_window(_hwnd: isize) {}
    pub fn show_window(_hwnd: isize) {}
    pub fn is_parked() -> bool {
        false
    }
    pub fn window_visible(_hwnd: isize) -> bool {
        true
    }
}

#[cfg(not(windows))]
pub use imp::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    #[ignore = "opens the clipboard to check its generation without reading or writing its contents"]
    fn stale_clipboard_sequence_is_never_written() {
        let current = clipboard_sequence().expect("Windows clipboard sequence is available");
        assert!(!copy_if_clipboard_unchanged("Must stay out of the clipboard.", Some(current.wrapping_sub(1))));
        assert_eq!(clipboard_sequence(), Some(current));
    }

    #[test]
    fn keyboard_combinations_match_the_existing_picker() {
        assert_eq!(binding("ctrl+shift+f8"), Some(Binding::Registered { modifiers: 6, key: 0x77 }));
        assert_eq!(binding("alt+shift+arrow_left"), Some(Binding::Registered { modifiers: 5, key: 0x25 }));
        assert_eq!(binding("f24"), Some(Binding::Registered { modifiers: 0, key: 0x87 }));
        assert_eq!(binding("pause"), Some(Binding::Legacy(Hotkey::Pause)));
        for invalid in ["f12", "f01", "f+1", "+f8", "shift+ctrl+f8", "ctrl+ctrl+f8",
            "ctrl+a+f8", "win+a", "alt+f4", "ctrl+alt+delete", "a", "alt+F8"] {
            assert!(binding(invalid).is_none(), "accepted {invalid}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn fixture_respects_capture_and_requires_warning_confirmation() {
        use std::sync::atomic::Ordering;
        let (core, _) = crate::engine::Core::new(crate::storage::Settings::default(), true);
        assert!(!save_hotkey(&core, "ctrl+shift+f8", false));
        core.capture_paused.store(true, Ordering::Release);
        assert_eq!(check_hotkey(&core, "ctrl+1").0, "warning");
        assert!(!save_hotkey(&core, "ctrl+1", false));
        assert!(save_hotkey(&core, "ctrl+1", true));
        assert_eq!(core.settings.lock().unwrap().registered_hotkey.as_deref(), Some("ctrl+1"));
        assert!(save_hotkey(&core, "pause", false));
        let settings = core.settings.lock().unwrap();
        assert_eq!(settings.hotkey, Hotkey::Pause);
        assert_eq!(settings.registered_hotkey, None);
    }

    #[test]
    fn only_the_original_window_and_focused_control_can_receive_text() {
        let target = Target::fixture();
        assert!(admit_delivery(target, Some(target), false).is_ok());
        assert!(admit_delivery(target, Some(target), true).is_err());
        assert!(admit_delivery(target, None, false).is_err());
        for changed in [Target { focus: 43, ..target }, Target { window: 13, ..target },
            Target { pid: target.pid.wrapping_add(1), ..target }, Target { thread: 16, ..target }] {
            assert!(admit_delivery(target, Some(changed), false).is_err());
        }
        let own = Target { pid: std::process::id(), ..target };
        assert!(admit_delivery(own, Some(own), false).is_err());
    }
}
