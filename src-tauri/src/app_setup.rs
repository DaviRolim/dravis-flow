use crate::state::StatusPayload;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

pub fn show_main_window(app: &AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _: Result<_, _> = main.show();
        let _: Result<_, _> = main.set_focus();
    }
}

pub fn position_widget_window(window: &WebviewWindow) -> Result<(), String> {
    if let Some(monitor) = window
        .current_monitor()
        .map_err(|e| format!("monitor query failed: {e}"))?
    {
        let monitor_size = monitor.size();
        let monitor_pos = monitor.position();
        let window_size = window
            .outer_size()
            .map_err(|e| format!("widget size query failed: {e}"))?;

        let x_offset = ((monitor_size.width as i32 - window_size.width as i32) / 2).max(0);
        let y_offset = monitor_size.height as i32 - window_size.height as i32 - 56;
        let x = monitor_pos.x + x_offset;
        let y = (monitor_pos.y + y_offset).max(monitor_pos.y);

        window
            .set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }))
            .map_err(|e| format!("widget position failed: {e}"))?;
    }
    Ok(())
}

pub fn build_tray(app: &AppHandle) -> Result<(), String> {
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)
        .map_err(|e| format!("failed creating settings menu item: {e}"))?;
    let about = MenuItem::with_id(app, "about", "About", true, None::<&str>)
        .map_err(|e| format!("failed creating about menu item: {e}"))?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)
        .map_err(|e| format!("failed creating quit menu item: {e}"))?;

    let menu = Menu::with_items(app, &[&settings, &about, &quit])
        .map_err(|e| format!("failed creating tray menu: {e}"))?;

    let icon = Image::from_bytes(include_bytes!("../icons/32x32.png"))
        .map_err(|e| format!("failed loading tray icon: {e}"))?;

    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => {
                show_main_window(app);
            }
            "about" => {
                let _ = app.emit(
                    "status",
                    StatusPayload {
                        status: "idle".to_string(),
                        message: Some("DraVis Flow - local voice transcription".to_string()),
                    },
                );
                show_main_window(app);
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|e| format!("failed creating tray icon: {e}"))?;

    Ok(())
}
