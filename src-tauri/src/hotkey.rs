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
