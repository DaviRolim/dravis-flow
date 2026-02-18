use arboard::Clipboard;
use std::{thread, time::Duration};

#[cfg(not(target_os = "macos"))]
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

pub fn paste_text(text: &str) -> Result<(), String> {
    eprintln!("injector: preparing clipboard");
    let mut clipboard = Clipboard::new().map_err(|e| format!("clipboard init failed: {e}"))?;
    let previous = clipboard.get_text().ok();

    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("clipboard set failed: {e}"))?;

    thread::sleep(Duration::from_millis(20));
    eprintln!("injector: triggering paste shortcut");

    #[cfg(target_os = "macos")]
    {
        // Use AppleScript for paste simulation to avoid hard crashes from lower-level input APIs.
        let status = std::process::Command::new("osascript")
            .arg("-e")
            .arg("tell application \"System Events\" to keystroke \"v\" using command down")
            .status()
            .map_err(|e| format!("failed to run osascript: {e}"))?;
        if !status.success() {
            return Err(format!("osascript paste failed with status: {status}"));
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

    thread::sleep(Duration::from_millis(20));

    if let Some(previous_text) = previous {
        let _ = clipboard.set_text(previous_text);
    }

    eprintln!("injector: done");

    Ok(())
}
