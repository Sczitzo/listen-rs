use super::{DeliveryError, Target};
#[cfg(debug_assertions)]
use super::imp;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetWindowLongPtrW, GetWindowThreadProcessId, IsWindowUnicode, IsWindowVisible,
    RealGetWindowClassW, SendMessageTimeoutW, ES_AUTOHSCROLL, ES_LOWERCASE,
    ES_MULTILINE, ES_NUMBER,
    ES_PASSWORD, ES_READONLY, ES_UPPERCASE, GA_ROOT, GWL_STYLE, SMTO_ABORTIFHUNG, SMTO_BLOCK,
    SMTO_ERRORONEXIT, WM_GETTEXT, WS_DISABLED,
};

// Standard edit messages below WM_USER are marshalled by Windows across processes.
const EM_GETSEL: u32 = 0x00B0;
const EM_REPLACESEL: u32 = 0x00C2;
const EM_GETLIMITTEXT: u32 = 0x00D5;
const EM_GETTEXTMODE: u32 = 0x0400 + 90;
const TM_PLAINTEXT: usize = 1;
const TM_MULTICODEPAGE: usize = 32;
const MAX_FIELD_UNITS: usize = 65_534;
const READ_TIMEOUT_MS: u32 = 200;
const UNSUPPORTED: &str = "Listen could not verify this text field. Your text is saved for manual paste.";
const CHANGED: &str = "The original field changed before delivery. Your text is saved for manual paste.";
const UNKNOWN: &str = "The edit could not be confirmed. Wait for the original app to finish and check it before copying again.";

fn class_is(window: HWND, name: &str) -> bool {
    let mut class = [0u16; 64];
    let count = unsafe { RealGetWindowClassW(window, class.as_mut_ptr(), class.len() as u32) };
    count != 0 && class[..count as usize].iter().copied().eq(name.encode_utf16())
}

fn supports_text(style: i32, multiline: bool) -> bool {
    let unsafe_styles = ES_READONLY | ES_PASSWORD | ES_NUMBER | ES_LOWERCASE | ES_UPPERCASE | WS_DISABLED as i32;
    // Fixed-width single-line fields can truncate text before the reported character limit.
    let accepts_width = style & ES_MULTILINE != 0 || (!multiline && style & ES_AUTOHSCROLL != 0);
    style & unsafe_styles == 0 && accepts_width
}

fn verified_field(target: Target, multiline: bool) -> Option<(HWND, bool)> {
    let field = target.focus as HWND;
    let root = target.window as HWND;
    let mut pid = 0;
    let thread = unsafe { GetWindowThreadProcessId(field, &mut pid) };
    if thread != target.thread || pid != target.pid || field == root
        || unsafe { GetAncestor(field, GA_ROOT) } != root
        || unsafe { IsWindowUnicode(field) == 0 || IsWindowVisible(field) == 0 }
    {
        return None;
    }
    let style = unsafe { GetWindowLongPtrW(field, GWL_STYLE) } as i32;
    if !supports_text(style, multiline) {
        return None;
    }
    if class_is(field, "Edit") {
        return Some((field, false));
    }
    if class_is(field, "RICHEDIT50W") || class_is(field, "RichEditD2DPT") {
        let mode = message(field, EM_GETTEXTMODE, 0, 0, READ_TIMEOUT_MS)?;
        let plain_unicode = TM_PLAINTEXT | TM_MULTICODEPAGE;
        if mode & plain_unicode == plain_unicode {
            return Some((field, true));
        }
    }
    None
}

fn blocked_native_control(target: Target) -> bool {
    let field = target.focus as HWND;
    let root = target.window as HWND;
    if field.is_null() || field == root {
        return false;
    }
    let mut pid = 0;
    let thread = unsafe { GetWindowThreadProcessId(field, &mut pid) };
    if thread != target.thread || pid != target.pid || unsafe { GetAncestor(field, GA_ROOT) } != root {
        return false;
    }
    let style = unsafe { GetWindowLongPtrW(field, GWL_STYLE) } as i32;
    let protected = style & (ES_READONLY | ES_PASSWORD | WS_DISABLED as i32) != 0;
    if class_is(field, "Edit") {
        // Block protected classic Edit controls and fixed-width single-line Edit controls.
        return protected || (style & (ES_MULTILINE | ES_AUTOHSCROLL) == 0);
    }
    if class_is(field, "RICHEDIT50W") || class_is(field, "RichEditD2DPT") {
        // Never fall through to simulated typing/Ctrl+V on read-only, password, or disabled RichEdit.
        return protected;
    }
    false
}

pub(super) enum DirectResult {
    Confirmed,
    Unsupported,
    Failed(DeliveryError),
}

fn message(field: HWND, id: u32, word: usize, data: isize, timeout: u32) -> Option<usize> {
    let mut result = 0;
    let flags = SMTO_ABORTIFHUNG | SMTO_BLOCK | SMTO_ERRORONEXIT;
    let completed = unsafe { SendMessageTimeoutW(field, id, word, data, flags, timeout, &mut result) };
    (completed != 0).then_some(result)
}

fn value(field: HWND) -> Option<Vec<u16>> {
    let mut units = vec![0; MAX_FIELD_UNITS + 2];
    let count = message(field, WM_GETTEXT, units.len(), units.as_mut_ptr() as isize, READ_TIMEOUT_MS)?;
    if count > MAX_FIELD_UNITS || units[count] != 0 || units[..count].contains(&0) {
        return None;
    }
    units.truncate(count);
    Some(units)
}

fn field_value(field: HWND, rich_edit: bool) -> Option<Vec<u16>> {
    let units = value(field)?;
    if rich_edit {
        // Rich Edit selection indexes count a paragraph break as one character.
        let text = String::from_utf16(&units).ok()?;
        return Some(text.replace("\r\n", "\r").encode_utf16().collect());
    }
    Some(units)
}

fn selection(field: HWND) -> Option<(usize, usize)> {
    // Low/high 16-bit offsets live in the low 32 bits of LRESULT; mask instead of try_from so
    // sign-extended 64-bit LRESULT values (when end >= 32768) decode cleanly.
    let positions = message(field, EM_GETSEL, 0, 0, READ_TIMEOUT_MS)? as u32;
    (positions != u32::MAX).then_some(((positions & 0xffff) as usize, (positions >> 16) as usize))
}

fn replacement(text: &str) -> Vec<u16> {
    text.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n").encode_utf16().collect()
}

fn expected_after(before: &[u16], (start, end): (usize, usize), insert: &[u16], limit: usize) -> Option<Vec<u16>> {
    let splits_pair = |index| {
        index > 0 && index < before.len()
            && ((matches!(before[index - 1], 0xd800..=0xdbff) && matches!(before[index], 0xdc00..=0xdfff))
                || (before[index - 1] == 13 && before[index] == 10))
    };
    if start > end || end > before.len() || splits_pair(start) || splits_pair(end)
        || before.len() - (end - start) + insert.len() > limit.min(MAX_FIELD_UNITS)
    {
        return None;
    }
    Some([&before[..start], insert, &before[end..]].concat())
}

pub(super) fn try_deliver(
    text: &str, target: Target, confirm_ready: impl FnOnce() -> Result<(), DeliveryError>,
) -> DirectResult {
    if text.contains('\t') {
        return DirectResult::Unsupported;
    }
    let multiline = text.contains(['\r', '\n']);
    if verified_field(target, multiline).is_none() {
        if blocked_native_control(target) {
            return DirectResult::Failed(UNSUPPORTED.into());
        }
        return DirectResult::Unsupported;
    }
    match deliver_inner(text, target, confirm_ready) {
        Ok(()) => DirectResult::Confirmed,
        Err(DirectAttemptError::Unsupported) => DirectResult::Unsupported,
        Err(DirectAttemptError::Failed(error)) => DirectResult::Failed(error),
    }
}

enum DirectAttemptError {
    Unsupported,
    Failed(DeliveryError),
}

#[cfg(test)]
pub(super) fn deliver(
    text: &str, target: Target, confirm_ready: impl FnOnce() -> Result<(), DeliveryError>,
) -> Result<(), DeliveryError> {
    match deliver_inner(text, target, confirm_ready) {
        Ok(()) => Ok(()),
        Err(DirectAttemptError::Unsupported) => Err(UNSUPPORTED.into()),
        Err(DirectAttemptError::Failed(error)) => Err(error),
    }
}

fn deliver_inner(
    text: &str, target: Target, confirm_ready: impl FnOnce() -> Result<(), DeliveryError>,
) -> Result<(), DirectAttemptError> {
    let multiline = text.contains(['\r', '\n']);
    let (field, rich_edit) = verified_field(target, multiline).ok_or(DirectAttemptError::Unsupported)?;
    let before = field_value(field, rich_edit).ok_or(DirectAttemptError::Unsupported)?;
    let selected = selection(field).ok_or(DirectAttemptError::Unsupported)?;
    let limit = message(field, EM_GETLIMITTEXT, 0, 0, READ_TIMEOUT_MS).ok_or(DirectAttemptError::Unsupported)?;
    let mut insert = if rich_edit {
        text.replace("\r\n", "\r").replace('\n', "\r").encode_utf16().collect()
    } else {
        replacement(text)
    };
    let expected = expected_after(&before, selected, &insert, limit).ok_or(DirectAttemptError::Unsupported)?;
    if expected == before {
        return Err(DirectAttemptError::Failed("The selection already contains this text.".into()));
    }
    insert.push(0);
    if field_value(field, rich_edit).as_ref() != Some(&before) || selection(field) != Some(selected)
        || verified_field(target, multiline) != Some((field, rich_edit))
    {
        return Err(DirectAttemptError::Failed(CHANGED.into()));
    }
    confirm_ready().map_err(DirectAttemptError::Failed)?;

    // Windows marshals this system message across processes. A timed-out edit may still arrive.
    if message(field, EM_REPLACESEL, 1, insert.as_ptr() as isize, 750).is_none() {
        crate::diagnostics::error("Windows did not acknowledge a direct paste before the wait ended.");
        return Err(DirectAttemptError::Failed(DeliveryError::Uncertain(UNKNOWN.into())));
    }
    if verified_field(target, multiline) != Some((field, rich_edit))
        || field_value(field, rich_edit).as_ref() != Some(&expected)
    {
        return Err(DirectAttemptError::Failed(DeliveryError::Uncertain(UNKNOWN.into())));
    }
    Ok(())
}

#[cfg(debug_assertions)]
pub fn check_native(directory: &std::path::Path) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind, Read};
    use std::time::{Duration, Instant};
    #[derive(serde::Deserialize)]
    struct FixtureField { window: isize, field: isize, pid: u32 }
    impl FixtureField {
        fn target(&self) -> Option<Target> {
            let mut pid = 0;
            let root = self.window as HWND;
            let thread = unsafe { GetWindowThreadProcessId(root, &mut pid) };
            if thread == 0 || pid != self.pid || !class_is(root, "ListenRustMultilineProbe")
                || unsafe { GetAncestor(self.field as HWND, GA_ROOT) } != root
            {
                return None;
            }
            Some(Target { window: self.window, focus: self.field, pid, focus_pid: pid, thread })
        }
    }
    #[derive(serde::Deserialize)]
    struct Request {
        text: String,
        #[serde(flatten)]
        fixture: FixtureField,
        #[serde(default)]
        background: bool,
    }
    if std::env::var_os("APPDATA").as_deref() != Some(directory.as_os_str())
        || !directory.join("rust-test-isolated").is_file()
    {
        return Err(Error::new(ErrorKind::InvalidInput, "Use a disposable Rust test profile."));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(directory.join("request.json"))?.take(32_769).read_to_end(&mut bytes)?;
    let request: Request = serde_json::from_slice(&bytes)?;
    let result = (|| {
        let Some(target) = request.fixture.target() else { return "not_sent" };
        if !request.background && imp::capture_target() != Some(target) {
            return "not_sent";
        }
        if std::fs::write(directory.join("captured"), b"ready").is_err() {
            return "not_sent";
        }
        let end = Instant::now() + Duration::from_secs(6);
        while !directory.join("dispatch").is_file() {
            if Instant::now() >= end { return "not_sent"; }
            std::thread::sleep(Duration::from_millis(10));
        }
        let settings = crate::storage::Settings {
            destination: crate::storage::Destination::LastApp,
            sounds: false,
            ..Default::default()
        };
        let (core, _) = crate::engine::Core::new(settings, true);
        let deliver = |text: &str, target| {
            if !request.background {
                return imp::deliver_with_focus(text, target, false, true, || {
                    (imp::capture_target(), imp::modifiers_down())
                });
            }
            let mut bytes = Vec::new();
            let observed = std::fs::File::open(directory.join("observed.json"))
                .and_then(|file| file.take(513).read_to_end(&mut bytes)).ok()
                .and_then(|_| serde_json::from_slice::<FixtureField>(&bytes).ok())
                .filter(|field| field.pid == request.fixture.pid)
                .and_then(|field| field.target());
            imp::deliver_with_focus(text, target, false, true, || (observed, false))
        };
        let outcome = crate::engine::save_and_deliver_using(
            &core, request.text.clone(), String::new(), 0.0, 0.0, Some(target), deliver, |_| false,
        );
        match outcome.as_deref() {
            Some("paste confirmed") => "verified",
            Some("kept in Listen") => "not_sent",
            _ => "uncertain",
        }
    })();
    let saved = crate::storage::load_history();
    let receipt = serde_json::json!({
        "status": result,
        "saved_rows": saved.len(),
        "history_saved_exactly": saved.first().is_some_and(|entry| entry.text() == request.text),
        "saved_outcome": saved.first().map(|entry| entry.outcome.as_str()),
    });
    std::fs::write(directory.join("result.json"), receipt.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_plain_rich_edit_accepts_one_verified_replacement() {
        use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
        use windows_sys::Win32::System::LibraryLoader::{LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        use windows_sys::w;

        struct Window(HWND);
        impl Drop for Window {
            fn drop(&mut self) { unsafe { DestroyWindow(self.0) }; }
        }
        struct Library(HMODULE);
        impl Drop for Library {
            fn drop(&mut self) { unsafe { FreeLibrary(self.0) }; }
        }

        unsafe {
            let none = std::ptr::null_mut();
            let library = Library(LoadLibraryExW(w!("msftedit.dll"), none, LOAD_LIBRARY_SEARCH_SYSTEM32));
            assert!(!library.0.is_null());
            let root = Window(CreateWindowExW(WS_EX_NOACTIVATE, w!("STATIC"), w!("Listen paste test"),
                WS_POPUP | WS_VISIBLE, -30_000, -30_000, 300, 100, none, none, none, std::ptr::null()));
            assert!(!root.0.is_null());
            let field = Window(CreateWindowExW(0, w!("RICHEDIT50W"), w!(""),
                WS_CHILD | WS_VISIBLE | ES_MULTILINE as u32, 0, 0, 300, 100, root.0, none, none, std::ptr::null()));
            assert!(!field.0.is_null());
            let thread = GetWindowThreadProcessId(root.0, none.cast());
            let target = Target {
                window: root.0 as isize,
                focus: field.0 as isize,
                thread,
                pid: std::process::id(),
                focus_pid: std::process::id(),
            };
            assert!(verified_field(target, false).is_none(), "formatted fields use recovery");
            assert_eq!(SendMessageW(field.0, WM_USER + 89, TM_PLAINTEXT, 0), 0); // EM_SETTEXTMODE
            assert_ne!(SendMessageW(field.0, WM_SETTEXT, 0, w!("First\r\nreplace last") as isize), 0);
            SendMessageW(field.0, 0x00B1, 6, 13); // EM_SETSEL
            let changed = DeliveryError::from("Focus changed.");
            assert_eq!(deliver("café\nsecond", target, || Err("Focus changed.".into())), Err(changed));
            assert_eq!(String::from_utf16(&value(field.0).unwrap()).unwrap(), "First\r\nreplace last");
            assert_eq!(deliver("café\nsecond", target, || Ok(())), Ok(()));
            assert_eq!(String::from_utf16(&value(field.0).unwrap()).unwrap(), "First\r\ncafé\r\nsecond last");
        }
    }

    #[test]
    fn standard_edit_must_support_the_text_being_sent() {
        assert!(supports_text(ES_AUTOHSCROLL, false));
        assert!(!supports_text(ES_AUTOHSCROLL, true));
        assert!(!supports_text(0, false));
        assert!(supports_text(ES_MULTILINE, false));
        assert!(supports_text(ES_MULTILINE, true));
        assert!(!supports_text(0, true));
        for style in [ES_READONLY, ES_PASSWORD, ES_NUMBER, ES_LOWERCASE, ES_UPPERCASE, WS_DISABLED as i32] {
            assert!(!supports_text(style | ES_AUTOHSCROLL, false));
            assert!(!supports_text(style | ES_MULTILINE, true));
        }
    }

    #[test]
    fn preserves_unicode_selection_surroundings_and_blank_lines() {
        let before: Vec<_> = "🍏 replace suffix".encode_utf16().collect();
        let insert = replacement("café\r\n\n漢字\r🍎");
        let after = expected_after(&before, (3, 10), &insert, 100).unwrap();
        assert_eq!(String::from_utf16(&after).unwrap(), "🍏 café\r\n\r\n漢字\r\n🍎 suffix");
        assert!(expected_after(&before, (1, 10), &insert, 100).is_none());
        assert!(expected_after(&before, (3, 10), &insert, 11).is_none());
    }
}
