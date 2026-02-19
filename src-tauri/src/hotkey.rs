/// Threshold (ms) separating a quick tap (toggle mode) from a long hold (push-to-talk).
pub const HOLD_THRESHOLD_MS: u128 = 300;

/// Action the shortcut handler should dispatch after resolving input state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShortcutAction {
    /// Start recording (hotkey pressed while idle).
    Start,
    /// Stop recording and begin transcription pipeline.
    Stop,
    /// Quick-tap confirmed — switch to toggle mode, keep recording.
    ToggleActivated,
}

/// Determine what action (if any) to dispatch for a shortcut event.
///
/// This is a pure function that takes a snapshot of relevant state and returns an action,
/// making it fully testable without an `AppHandle`.
///
/// State mutations (`toggle_shortcut_held`, `toggle_active`, `press_instant`) are the
/// caller's responsibility and remain in `lib.rs` behind the `InnerState` Mutex.
///
/// # Parameters
/// - `pressed` — true for `ShortcutState::Pressed`, false for `Released`
/// - `is_idle` — true when `AppStatus == Idle`
/// - `is_recording` — true when `AppStatus == Recording`
/// - `toggle_shortcut_held` — whether the key is currently considered physically held
/// - `toggle_active` — whether we are in toggle mode (recording, waiting for second tap)
/// - `held_ms` — elapsed milliseconds since the key was pressed (`None` = unknown, treated as long hold)
pub fn resolve_shortcut_action(
    pressed: bool,
    is_idle: bool,
    is_recording: bool,
    toggle_shortcut_held: bool,
    toggle_active: bool,
    held_ms: Option<u128>,
) -> Option<ShortcutAction> {
    if pressed {
        // Ignore OS key-repeat events while the key is physically held.
        if toggle_shortcut_held {
            return None;
        }
        if toggle_active {
            // Second tap in toggle mode — stop recording.
            return Some(ShortcutAction::Stop);
        }
        if is_idle {
            return Some(ShortcutAction::Start);
        }
        None
    } else {
        // Released
        if is_recording && !toggle_active {
            let ms = held_ms.unwrap_or(HOLD_THRESHOLD_MS + 1);
            if ms >= HOLD_THRESHOLD_MS {
                // Held long enough: push-to-talk stop.
                Some(ShortcutAction::Stop)
            } else {
                // Quick tap: enter toggle mode.
                Some(ShortcutAction::ToggleActivated)
            }
        } else {
            None
        }
    }
}

/// Convert our config hotkey format ("ctrl+shift+space") to the format
/// expected by tauri-plugin-global-shortcut ("Control+Shift+Space").
pub fn config_combo_to_shortcut(combo: &str) -> Result<String, String> {
    let parts: Vec<String> = combo
        .split('+')
        .map(|p| match p.trim().to_lowercase().as_str() {
            "ctrl" | "control" => Ok("Control".to_string()),
            "shift" => Ok("Shift".to_string()),
            "alt" | "option" => Ok("Alt".to_string()),
            "cmd" | "meta" | "super" => Ok("Super".to_string()),
            "space" => Ok("Space".to_string()),
            other => Err(format!("unsupported key token: {other}")),
        })
        .collect::<Result<Vec<_>, _>>()?;

    if parts.is_empty() {
        return Err("empty hotkey".to_string());
    }

    Ok(parts.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── resolve_shortcut_action tests ────────────────────────────────────────

    #[test]
    fn quick_tap_activates_toggle_mode() {
        // Release after < 300ms while recording and not already in toggle mode
        let action = resolve_shortcut_action(
            false, // released
            false, // not idle
            true,  // is_recording
            false, // toggle_shortcut_held (already cleared by caller on release)
            false, // toggle_active
            Some(150), // held_ms < HOLD_THRESHOLD_MS
        );
        assert_eq!(action, Some(ShortcutAction::ToggleActivated));
    }

    #[test]
    fn long_hold_stops_recording() {
        // Release after >= 300ms while recording — push-to-talk stop
        let action = resolve_shortcut_action(
            false,
            false,
            true,
            false,
            false,
            Some(350), // held_ms >= HOLD_THRESHOLD_MS
        );
        assert_eq!(action, Some(ShortcutAction::Stop));
    }

    #[test]
    fn press_while_held_is_ignored() {
        // OS key-repeat: toggle_shortcut_held is already true → None
        let action = resolve_shortcut_action(
            true,  // pressed (key-repeat)
            false,
            true,
            true,  // toggle_shortcut_held = true
            false,
            None,
        );
        assert_eq!(action, None);
    }

    #[test]
    fn second_press_in_toggle_mode_stops() {
        // toggle_active = true, new Pressed event → Stop
        let action = resolve_shortcut_action(
            true,  // pressed
            false, // not idle (recording)
            true,
            false, // toggle_shortcut_held = false (fresh press)
            true,  // toggle_active
            None,
        );
        assert_eq!(action, Some(ShortcutAction::Stop));
    }

    // ── config_combo_to_shortcut tests ───────────────────────────────────────

    #[test]
    fn valid_combo_ctrl_shift_space() {
        let result = config_combo_to_shortcut("ctrl+shift+space").unwrap();
        assert_eq!(result, "Control+Shift+Space");
    }

    #[test]
    fn valid_combo_cmd_space() {
        let result = config_combo_to_shortcut("cmd+space").unwrap();
        assert_eq!(result, "Super+Space");
    }

    #[test]
    fn valid_combo_alt_option_alias() {
        let result = config_combo_to_shortcut("option+space").unwrap();
        assert_eq!(result, "Alt+Space");
    }

    #[test]
    fn valid_combo_with_whitespace() {
        let result = config_combo_to_shortcut(" ctrl + shift + space ").unwrap();
        assert_eq!(result, "Control+Shift+Space");
    }

    #[test]
    fn valid_combo_case_insensitive() {
        let result = config_combo_to_shortcut("CTRL+SHIFT+SPACE").unwrap();
        assert_eq!(result, "Control+Shift+Space");
    }

    #[test]
    fn unknown_token_returns_err() {
        let result = config_combo_to_shortcut("ctrl+banana");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("banana"));
    }

    #[test]
    fn empty_hotkey_returns_err() {
        let result = config_combo_to_shortcut("");
        assert!(result.is_err());
    }
}
