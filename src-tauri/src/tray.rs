use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use std::sync::atomic::Ordering;
use std::sync::Mutex;

static TOGGLE_ITEM: Mutex<Option<MenuItem<Wry>>> = Mutex::new(None);

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let currently_on = crate::SELECTION_ENABLED.load(Ordering::SeqCst);
    let toggle = MenuItem::with_id(
        app,
        "toggle",
        if currently_on { "禁用划词" } else { "启用划词" },
        true,
        None::<&str>,
    )?;
    let config_dir = MenuItem::with_id(app, "config", "打开配置目录", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出简译", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle, &config_dir, &sep, &quit])?;

        if let Ok(mut guard) = TOGGLE_ITEM.lock() {
        *guard = Some(toggle.clone());
    }

    let mut builder = TrayIconBuilder::with_id("st-tray")
        .menu(&menu)
        .tooltip("简译 · 划词翻译")
        .show_menu_on_left_click(false);

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => crate::show_main(app),
            "toggle" => {
                let enabled = !crate::SELECTION_ENABLED.load(Ordering::SeqCst);
                set_selection_enabled(app, enabled);
            }
            "config" => {
                if let Ok(path) = crate::config::config_path(app) {
                    if let Some(dir) = path.parent() {
                        let _ = tauri_plugin_opener::open_path(
                            dir.to_string_lossy().to_string(),
                            None::<&str>,
                        );
                    }
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::show_main(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

pub fn apply_state(app: &AppHandle, enabled: bool) {
    crate::SELECTION_ENABLED.store(enabled, Ordering::SeqCst);

    if let Ok(guard) = TOGGLE_ITEM.lock() {
        if let Some(item) = guard.as_ref() {
            let _ = item.set_text(if enabled { "禁用划词" } else { "启用划词" });
        }
    }

    if !enabled {
        #[cfg(target_os = "windows")]
        crate::selection::hide_button(app);
    }
}

pub fn set_selection_enabled(app: &AppHandle, enabled: bool) {
    apply_state(app, enabled);

    let state = app.state::<crate::AppState>();
    let mut cfg = state.config(app);
    if cfg.selection_enabled == enabled {
        return;
    }
    cfg.selection_enabled = enabled;
    if crate::config::save(app, &cfg).is_ok() {
        state.replace(cfg.clone());
        let _ = app.emit("config-changed", &cfg);
    }
}
