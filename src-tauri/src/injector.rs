use arboard::Clipboard;
use std::{thread, time::Duration};

#[cfg(not(target_os = "macos"))]
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

/// Paste text into the currently focused application.
///
/// Strategy: set clipboard → trigger Cmd+V (or Ctrl+V) → wait → restore clipboard.
/// macOS uses CGEvent API directly for reliability (no osascript process spawn).
pub fn paste_text(text: &str) -> Result<(), String> {
    eprintln!("injector: preparing clipboard");
    let mut clipboard = Clipboard::new().map_err(|e| format!("clipboard init failed: {e}"))?;
    let previous = clipboard.get_text().ok();

    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("clipboard set failed: {e}"))?;

    // Give the pasteboard time to sync across processes
    thread::sleep(Duration::from_millis(50));
    eprintln!("injector: triggering paste");

    #[cfg(target_os = "macos")]
    {
        paste_cmd_v_cgevent()?;
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
    thread::sleep(Duration::from_millis(100));

    if let Some(previous_text) = previous {
        let _ = clipboard.set_text(previous_text);
    }

    eprintln!("injector: done");
    Ok(())
}

/// Use macOS CGEvent API to send Cmd+V directly — no process spawn, more reliable.
#[cfg(target_os = "macos")]
fn paste_cmd_v_cgevent() -> Result<(), String> {
    use core_graphics::event::{CGEvent, CGEventFlags, CGKeyCode};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    // Key code 9 = 'v' on US keyboard layout
    const KEY_V: CGKeyCode = 9;

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "failed to create CGEventSource".to_string())?;

    let key_down = CGEvent::new_keyboard_event(source.clone(), KEY_V, true)
        .map_err(|_| "failed to create key-down event".to_string())?;
    key_down.set_flags(CGEventFlags::CGEventFlagCommand);

    let key_up = CGEvent::new_keyboard_event(source, KEY_V, false)
        .map_err(|_| "failed to create key-up event".to_string())?;
    key_up.set_flags(CGEventFlags::CGEventFlagCommand);

    key_down.post(core_graphics::event::CGEventTapLocation::HID);
    thread::sleep(Duration::from_millis(10));
    key_up.post(core_graphics::event::CGEventTapLocation::HID);

    Ok(())
}
