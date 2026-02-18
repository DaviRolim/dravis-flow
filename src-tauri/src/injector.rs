use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::{thread, time::Duration};

pub fn paste_text(text: &str) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|e| format!("clipboard init failed: {e}"))?;
    let previous = clipboard.get_text().ok();

    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("clipboard set failed: {e}"))?;

    thread::sleep(Duration::from_millis(50));

    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| format!("input init failed: {e}"))?;

    #[cfg(target_os = "macos")]
    {
        enigo
            .key(Key::Meta, Direction::Press)
            .map_err(|e| format!("press cmd failed: {e}"))?;
        enigo
            .key(Key::Unicode('v'), Direction::Click)
            .map_err(|e| format!("press v failed: {e}"))?;
        enigo
            .key(Key::Meta, Direction::Release)
            .map_err(|e| format!("release cmd failed: {e}"))?;
    }

    #[cfg(not(target_os = "macos"))]
    {
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

    thread::sleep(Duration::from_millis(50));

    if let Some(previous_text) = previous {
        let _ = clipboard.set_text(previous_text);
    }

    Ok(())
}
