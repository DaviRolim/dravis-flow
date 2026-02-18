use rdev::{EventType, Key};
use std::collections::HashSet;
use std::str::FromStr;

#[derive(Debug, Clone, Copy)]
pub enum HotkeyAction {
    Pressed,
    Released,
}

#[derive(Debug, Clone)]
struct HotkeyConfig {
    modifiers: Vec<Key>,
    key: Key,
}

impl FromStr for HotkeyConfig {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts = s
            .split('+')
            .map(|p| p.trim().to_lowercase())
            .collect::<Vec<_>>();

        if parts.is_empty() {
            return Err("empty hotkey".to_string());
        }

        let mut modifiers = Vec::new();
        let mut key = None;

        for p in parts {
            match p.as_str() {
                "ctrl" | "control" => modifiers.push(Key::ControlLeft),
                "shift" => modifiers.push(Key::ShiftLeft),
                "alt" | "option" => modifiers.push(Key::Alt),
                "cmd" | "meta" | "super" => modifiers.push(Key::MetaLeft),
                "space" => key = Some(Key::Space),
                other => return Err(format!("unsupported key token: {other}")),
            }
        }

        let key = key.ok_or_else(|| "missing non-modifier key".to_string())?;
        Ok(Self { modifiers, key })
    }
}

pub fn start_listener<F>(combo: &str, callback: F) -> Result<(), String>
where
    F: Fn(HotkeyAction) + Send + Sync + 'static,
{
    let config = HotkeyConfig::from_str(combo)?;

    std::thread::spawn(move || {
        let mut pressed = HashSet::<Key>::new();
        let mut active = false;

        let callback_ref = callback;
        let result = rdev::listen(move |event| match event.event_type {
            EventType::KeyPress(k) => {
                pressed.insert(k);

                let modifiers_down = config.modifiers.iter().all(|m| {
                    pressed.contains(m)
                        || match m {
                            Key::ControlLeft => pressed.contains(&Key::ControlRight),
                            Key::ShiftLeft => pressed.contains(&Key::ShiftRight),
                            Key::MetaLeft => pressed.contains(&Key::MetaRight),
                            _ => false,
                        }
                });

                if modifiers_down && k == config.key && !active {
                    active = true;
                    callback_ref(HotkeyAction::Pressed);
                }
            }
            EventType::KeyRelease(k) => {
                pressed.remove(&k);
                if k == config.key && active {
                    active = false;
                    callback_ref(HotkeyAction::Released);
                }
            }
            _ => {}
        });

        if let Err(e) = result {
            eprintln!("hotkey listener failed: {e:?}");
        }
    });

    Ok(())
}
