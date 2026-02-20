//! Text injection via clipboard paste.
//!
//! Flow: save current clipboard → set text → simulate Cmd+V (CGEvent API on macOS,
//! osascript fallback) → restore original clipboard. The widget window has `focus: false`
//! so it never steals focus from the target application.

use arboard::Clipboard;
use std::{thread, time::Duration};

#[cfg(not(target_os = "macos"))]
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

/// Time for the macOS pasteboard to sync the new contents across processes before issuing Cmd+V.
const CLIPBOARD_SYNC_DELAY_MS: u64 = 50;

/// Time for the target app to read the clipboard and process the paste before we restore prior contents.
const PASTE_SETTLE_DELAY_MS: u64 = 100;

/// Gap between CGEvent key-down and key-up for the synthetic Cmd+V keystroke.
const KEY_EVENT_DELAY_MS: u64 = 10;

fn dlog_msg(msg: &str) {
    eprintln!("{msg}");
    crate::write_log(msg);
}

/// Returns the process ID of the currently frontmost application.
/// Used to restore focus to the target app before pasting.
#[cfg(target_os = "macos")]
pub fn get_frontmost_app_pid() -> Option<i32> {
    use std::ffi::c_char;
    use std::os::raw::{c_int, c_void};

    extern "C" {
        fn objc_getClass(name: *const c_char) -> *const c_void;
        fn sel_registerName(name: *const c_char) -> *const c_void;
        fn objc_msgSend(recv: *const c_void, sel: *const c_void) -> *const c_void;
    }

    type MsgSendRetInt = unsafe extern "C" fn(*const c_void, *const c_void) -> c_int;

    unsafe {
        let ws_cls = objc_getClass(b"NSWorkspace\0".as_ptr() as *const c_char);
        if ws_cls.is_null() {
            return None;
        }
        let sel_shared = sel_registerName(b"sharedWorkspace\0".as_ptr() as *const c_char);
        let workspace = objc_msgSend(ws_cls, sel_shared);
        if workspace.is_null() {
            return None;
        }
        let sel_frontmost = sel_registerName(b"frontmostApplication\0".as_ptr() as *const c_char);
        let front_app = objc_msgSend(workspace, sel_frontmost);
        if front_app.is_null() {
            return None;
        }
        let sel_pid = sel_registerName(b"processIdentifier\0".as_ptr() as *const c_char);
        // pid_t is i32; transmute to the correct return-type variant
        let msg_int: MsgSendRetInt = std::mem::transmute(objc_msgSend as *const ());
        let pid = msg_int(front_app, sel_pid);
        if pid > 0 {
            Some(pid)
        } else {
            None
        }
    }
}

/// Brings the application with the given PID to the foreground.
/// Called just before pasting to ensure Cmd+V reaches the target app.
#[cfg(target_os = "macos")]
pub fn activate_app_by_pid(pid: i32) {
    use std::ffi::c_char;
    use std::os::raw::{c_int, c_void};

    extern "C" {
        fn objc_getClass(name: *const c_char) -> *const c_void;
        fn sel_registerName(name: *const c_char) -> *const c_void;
        fn objc_msgSend(recv: *const c_void, sel: *const c_void) -> *const c_void;
    }

    // NSApplicationActivateIgnoringOtherApps = 1 << 1 = 2
    const ACTIVATE_IGNORING_OTHER_APPS: u64 = 2;

    type MsgSendRetPtrWithInt =
        unsafe extern "C" fn(*const c_void, *const c_void, c_int) -> *const c_void;
    type MsgSendRetBoolWithU64 =
        unsafe extern "C" fn(*const c_void, *const c_void, u64) -> u8;

    unsafe {
        let cls = objc_getClass(b"NSRunningApplication\0".as_ptr() as *const c_char);
        if cls.is_null() {
            return;
        }
        let sel_with_pid = sel_registerName(
            b"runningApplicationWithProcessIdentifier:\0".as_ptr() as *const c_char,
        );
        let msg_with_int: MsgSendRetPtrWithInt =
            std::mem::transmute(objc_msgSend as *const ());
        let app = msg_with_int(cls, sel_with_pid, pid as c_int);
        if app.is_null() {
            return;
        }
        let sel_activate =
            sel_registerName(b"activateWithOptions:\0".as_ptr() as *const c_char);
        let msg_activate: MsgSendRetBoolWithU64 =
            std::mem::transmute(objc_msgSend as *const ());
        let _ = msg_activate(app, sel_activate, ACTIVATE_IGNORING_OTHER_APPS);
    }
}

/// Returns true if this process has been granted macOS Accessibility permission.
/// CGEvent::post() silently does nothing without it on a signed/bundled app.
#[cfg(target_os = "macos")]
pub fn is_accessibility_trusted() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    unsafe { AXIsProcessTrusted() }
}

/// Triggers the macOS system dialog asking the user to grant Accessibility access.
/// Safe to call multiple times — does nothing if already trusted.
#[cfg(target_os = "macos")]
pub fn request_accessibility_permission() {
    use std::ffi::c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
        static kAXTrustedCheckOptionPrompt: *const c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFBooleanTrue: *const c_void;
        // Declared as u8 so we can take their address to pass as *const c_void
        static kCFTypeDictionaryKeyCallBacks: u8;
        static kCFTypeDictionaryValueCallBacks: u8;
        fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            num_values: isize,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }

    unsafe {
        let keys: [*const c_void; 1] = [kAXTrustedCheckOptionPrompt];
        let values: [*const c_void; 1] = [kCFBooleanTrue];
        let dict = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks as *const u8 as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const u8 as *const c_void,
        );
        AXIsProcessTrustedWithOptions(dict);
        if !dict.is_null() {
            CFRelease(dict);
        }
    }
}

/// Paste text into the currently focused application.
///
/// Strategy: set clipboard → trigger Cmd+V (or Ctrl+V) → wait → restore clipboard.
/// macOS uses CGEvent API directly for reliability (no osascript process spawn).
pub fn paste_text(text: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let trusted = is_accessibility_trusted();
        dlog_msg(&format!("injector: accessibility trusted = {trusted}"));
        if !trusted {
            return Err(
                "Accessibility permission required. Go to System Settings → Privacy & Security → Accessibility and enable DraVis Flow, then restart the app.".to_string()
            );
        }
    }

    dlog_msg(&format!("injector: setting clipboard ({} chars)", text.len()));
    let mut clipboard = Clipboard::new().map_err(|e| format!("clipboard init failed: {e}"))?;
    let previous = clipboard.get_text().ok();

    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("clipboard set failed: {e}"))?;
    dlog_msg("injector: clipboard set OK");

    // Give the pasteboard time to sync across processes
    thread::sleep(Duration::from_millis(CLIPBOARD_SYNC_DELAY_MS));
    dlog_msg("injector: triggering paste keystroke");

    #[cfg(target_os = "macos")]
    {
        // Try CGEvent first (fastest, most reliable), fall back to osascript
        if let Err(e) = paste_cmd_v_cgevent() {
            dlog_msg(&format!("injector: CGEvent failed ({e}), falling back to osascript"));
            paste_cmd_v_osascript()?;
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let mut enigo =
            Enigo::new(&Settings::default()).map_err(|e| format!("input init failed: {e}"))?;
        enigo
            .key(Key::Control, Direction::Press)
            .map_err(|e| format!("press ctrl failed: {e}"))?;
        enigo
            .key(Key::Unicode('v'), Direction::Click)
            .map_err(|e| format!("press v failed: {e}"))?;
        enigo
            .key(Key::Control, Direction::Release)
            .map_err(|e| format!("release ctrl failed: {e}"))?;
    }

    // Wait for the target app to read the clipboard before restoring
    thread::sleep(Duration::from_millis(PASTE_SETTLE_DELAY_MS));

    if let Some(previous_text) = previous {
        let _ = clipboard.set_text(previous_text);
    }

    dlog_msg("injector: paste sequence complete");
    Ok(())
}

/// Fallback: use osascript to send Cmd+V.
#[cfg(target_os = "macos")]
fn paste_cmd_v_osascript() -> Result<(), String> {
    let status = std::process::Command::new("osascript")
        .arg("-e")
        .arg("tell application \"System Events\" to keystroke \"v\" using command down")
        .status()
        .map_err(|e| format!("failed to run osascript: {e}"))?;
    if !status.success() {
        return Err(format!("osascript paste failed with status: {status}"));
    }
    Ok(())
}

/// Use macOS CGEvent API to send Cmd+V directly — no process spawn, more reliable.
#[cfg(target_os = "macos")]
fn paste_cmd_v_cgevent() -> Result<(), String> {
    use core_graphics::event::{CGEvent, CGEventFlags, CGKeyCode};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    // Key code 9 = 'v' on US keyboard layout
    const KEY_V: CGKeyCode = 9;

    dlog_msg("injector: creating CGEventSource (HIDSystemState)");
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "failed to create CGEventSource".to_string())?;

    let key_down = CGEvent::new_keyboard_event(source.clone(), KEY_V, true)
        .map_err(|_| "failed to create key-down event".to_string())?;
    key_down.set_flags(CGEventFlags::CGEventFlagCommand);

    let key_up = CGEvent::new_keyboard_event(source, KEY_V, false)
        .map_err(|_| "failed to create key-up event".to_string())?;
    key_up.set_flags(CGEventFlags::CGEventFlagCommand);

    dlog_msg("injector: posting CGEvent key-down (Cmd+V)");
    key_down.post(core_graphics::event::CGEventTapLocation::HID);
    thread::sleep(Duration::from_millis(KEY_EVENT_DELAY_MS));
    dlog_msg("injector: posting CGEvent key-up");
    key_up.post(core_graphics::event::CGEventTapLocation::HID);

    Ok(())
}
