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
