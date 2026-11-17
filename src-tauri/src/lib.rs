#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod translate;
mod tray;

#[cfg(target_os = "windows")]
mod selection;

use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use config::Config;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};

pub struct AppState {
    config: Mutex<Option<Config>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            config: Mutex::new(None),
        }
    }

                    pub fn config(&self, app: &AppHandle) -> Config {
        let mut guard = match self.config.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if guard.is_none() {
            *guard = Some(config::load_or_init(app));
        }
        guard.clone().unwrap_or_default()
    }

    pub fn replace(&self, cfg: Config) {
        let mut guard = match self.config.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        *guard = Some(cfg);
    }
}

static POPUP_PINNED: AtomicBool = AtomicBool::new(false);
static POPUP_READY: AtomicBool = AtomicBool::new(false);
pub static SELECTION_ENABLED: AtomicBool = AtomicBool::new(true);
static PENDING_TEXT: Mutex<String> = Mutex::new(String::new());

const POPUP_LABEL: &str = "popup";


pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn apply_theme(app: &AppHandle, theme: &str) {
    let dark = theme == "dark";
    let tauri_theme = if dark {
        tauri::Theme::Dark
    } else {
        tauri::Theme::Light
    };

            let bg = if dark {
        tauri::window::Color(0x14, 0x16, 0x1a, 255)
    } else {
        tauri::window::Color(0xf3, 0xf4, 0xf7, 255)
    };

    for label in ["main", POPUP_LABEL, "selection"] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.set_theme(Some(tauri_theme));
                        if label != "selection" {
                let _ = window.set_background_color(Some(bg));
            }
        }
    }
}

fn position_popup(window: &WebviewWindow, x: i32, y: i32) {
    let size = window
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new(470, 430));
    let w = size.width as i32;
    let h = size.height as i32;

    let mut px = x + 12;
    let mut py = y + 16;

    if let Ok(Some(monitor)) = window.current_monitor() {
        let ms = monitor.size();
        let mp = monitor.position();
        let left = mp.x;
        let top = mp.y;
        let right = mp.x + ms.width as i32;
        let bottom = mp.y + ms.height as i32;

        if px + w > right - 8 {
            px = right - w - 8;
        }
        if py + h > bottom - 8 {
            py = y - h - 16;
        }
        px = px.max(left + 8);
        py = py.max(top + 8);
    }

    let _ = window.set_position(PhysicalPosition::new(px, py));
}


static LAST_GEOMETRY_SAVE: AtomicU64 = AtomicU64::new(0);

fn point_on_some_monitor(win: &WebviewWindow, pos: tauri::PhysicalPosition<i32>) -> bool {
    let Ok(monitors) = win.available_monitors() else {
        return true;
    };
    if monitors.is_empty() {
        return true;
    }

    monitors.iter().any(|m| {
        let origin = m.position();
        let size = m.size();
        let left = origin.x;
        let top = origin.y;
        let right = left + size.width as i32;
        let bottom = top + size.height as i32;

                pos.x > left - 100
            && pos.x < right - 100
            && pos.y > top - 60
            && pos.y < bottom - 60
    })
}

fn apply_window_state(app: &AppHandle, ws: &config::WindowState) -> bool {
    let Some(win) = app.get_webview_window("main") else {
        return false;
    };

            let _ = win.set_min_size(Some(tauri::LogicalSize::new(
        config::MIN_WINDOW_WIDTH as f64,
        config::MIN_WINDOW_HEIGHT as f64,
    )));

    let _ = win.set_size(tauri::LogicalSize::new(
        ws.width.max(config::MIN_WINDOW_WIDTH) as f64,
        ws.height.max(config::MIN_WINDOW_HEIGHT) as f64,
    ));

    let mut position_applied = false;

    if let (Some(x), Some(y)) = (ws.x, ws.y) {
        let target = tauri::LogicalPosition::new(x as f64, y as f64);
        let scale = win.scale_factor().unwrap_or(1.0);
        let physical = target.to_physical::<i32>(scale);

                if point_on_some_monitor(&win, physical) {
            let _ = win.set_position(target);
            position_applied = true;
        }
    }

    if ws.maximized {
        let _ = win.maximize();
    }

    position_applied
}

pub fn save_window_state(app: &AppHandle) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };

        if win.is_minimized().unwrap_or(false) {
        return;
    }

    let state = app.state::<AppState>();
    let mut cfg = state.config(app);
    let scale = win.scale_factor().unwrap_or(1.0);

    if win.is_maximized().unwrap_or(false) {
                cfg.window.maximized = true;
    } else if let (Ok(size), Ok(pos)) = (win.inner_size(), win.outer_position()) {
                if point_on_some_monitor(&win, pos) {
            let logical_pos = pos.to_logical::<i32>(scale);
            cfg.window.x = Some(logical_pos.x);
            cfg.window.y = Some(logical_pos.y);
        }

        let logical_size = size.to_logical::<f64>(scale);
        cfg.window.width = logical_size.width.round().max(1.0) as u32;
        cfg.window.height = logical_size.height.round().max(1.0) as u32;
        cfg.window.maximized = false;
    }

    if config::save(app, &cfg).is_ok() {
        state.replace(cfg);
    }
}

fn schedule_save_window_state(app: &AppHandle) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let last = LAST_GEOMETRY_SAVE.load(Ordering::Relaxed);
    if now.saturating_sub(last) < 700 {
        return;
    }
    LAST_GEOMETRY_SAVE.store(now, Ordering::Relaxed);
    save_window_state(app);
}

fn register_hotkeys(app: &AppHandle, hk: &config::HotkeysConfig) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

    let manager = app.global_shortcut();
    let _ = manager.unregister_all();

    let entries = [
        ("唤起主窗口", &hk.show_window),
        ("划词翻译", &hk.translate),
        ("禁用划词", &hk.disable),
        ("启用划词", &hk.enable),
    ];

    let mut registered: Vec<Shortcut> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for (label, raw) in entries {
        let text = raw.trim();
        if text.is_empty() {
            continue;
        }
        match text.parse::<Shortcut>() {
            Ok(shortcut) => {
                                if registered.contains(&shortcut) {
                    continue;
                }
                match manager.register(shortcut) {
                    Ok(_) => registered.push(shortcut),
                    Err(e) => errors.push(format!(
                        "{label}「{text}」注册失败（可能被其他程序占用）：{e}"
                    )),
                }
            }
            Err(e) => errors.push(format!("{label}「{text}」格式无效：{e}")),
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

fn on_shortcut(app: &AppHandle, shortcut: &tauri_plugin_global_shortcut::Shortcut) {
    use tauri_plugin_global_shortcut::Shortcut;

    let cfg = app.state::<AppState>().config(app);
    let hk = &cfg.hotkeys;

    let hit = |raw: &str| -> bool {
        let text = raw.trim();
        if text.is_empty() {
            return false;
        }
        text.parse::<Shortcut>()
            .map(|s| s == *shortcut)
            .unwrap_or(false)
    };

    if hit(&hk.show_window) {
        show_main(app);
    } else if hit(&hk.translate) {
        on_hotkey(app);
    } else if hit(&hk.disable) {
        tray::set_selection_enabled(app, false);
    } else if hit(&hk.enable) {
        tray::set_selection_enabled(app, true);
    }
}

fn on_hotkey(app: &AppHandle) {
        if !SELECTION_ENABLED.load(Ordering::SeqCst) {
        return;
    }

        if app.state::<AppState>().config(app).trigger.mode == "button" {
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        let (text, pos) = {
                                                selection::wait_modifiers_released(700);
            std::thread::sleep(std::time::Duration::from_millis(30));

                        let cursor = selection::cursor_position();
            let text = match selection::read_selection_via_uia(cursor) {
                Some(t) => t,
                None => {
                                        let mut captured = selection::capture_selection_text().unwrap_or_default();
                    if captured.is_empty() {
                        std::thread::sleep(std::time::Duration::from_millis(120));
                        captured = selection::capture_selection_text().unwrap_or_default();
                    }
                    captured
                }
            };
            (text, cursor)
        };
        #[cfg(not(target_os = "windows"))]
        let (text, pos) = (String::new(), (0, 0));

        show_popup(&app, &text, Some(pos));
    });
}


pub fn show_popup(app: &AppHandle, text: &str, at: Option<(i32, i32)>) {
        POPUP_PINNED.store(false, Ordering::SeqCst);

    if let Ok(mut pending) = PENDING_TEXT.lock() {
        *pending = text.to_string();
    }

    let Some(popup) = app.get_webview_window(POPUP_LABEL) else {
        return;
    };

    if let Some((x, y)) = at {
        position_popup(&popup, x, y);
    }

    let _ = popup.show();
    let _ = popup.set_focus();

    if POPUP_READY.load(Ordering::SeqCst) {
        let _ = app.emit_to(POPUP_LABEL, "popup-text", text.to_string());
    }
}


#[tauri::command]
fn get_config(app: AppHandle, state: tauri::State<'_, AppState>) -> Config {
    state.config(&app)
}

#[tauri::command]
fn get_config_path(app: AppHandle) -> Result<String, String> {
    config::config_path(&app).map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn save_config(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    config: Config,
) -> Result<String, String> {
    config::save(&app, &config)?;
    register_hotkeys(&app, &config.hotkeys)?;
    apply_theme(&app, &config.theme);

        tray::apply_state(&app, config.selection_enabled);

        if config.trigger.mode == "hotkey" {
        #[cfg(target_os = "windows")]
        selection::hide_button(&app);
    }

    {
        state.replace(config.clone());
    }

    let _ = app.emit("config-changed", &config);
    get_config_path(app)
}

#[tauri::command]
fn reset_scene_prompt(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    scene_id: String,
) -> Option<String> {
    let default = config::default_prompt_for(&scene_id)?;
    let mut cfg = state.config(&app);
    if let Some(scene) = cfg.scenes.iter_mut().find(|s| s.id == scene_id) {
        scene.prompt = default.to_string();
    }
    state.replace(cfg);
    Some(default.to_string())
}

#[tauri::command]
fn popup_set_pinned(pinned: bool) -> bool {
    POPUP_PINNED.store(pinned, Ordering::SeqCst);
    pinned
}

#[tauri::command]
fn popup_is_pinned() -> bool {
    POPUP_PINNED.load(Ordering::SeqCst)
}

#[tauri::command]
fn popup_ready(app: AppHandle) {
    POPUP_READY.store(true, Ordering::SeqCst);
    let pending = PENDING_TEXT
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();
    if !pending.is_empty() {
        let _ = app.emit_to(POPUP_LABEL, "popup-text", pending);
    }
}

#[tauri::command]
fn close_popup(app: AppHandle) {
    POPUP_PINNED.store(false, Ordering::SeqCst);
    if let Some(w) = app.get_webview_window(POPUP_LABEL) {
        let _ = w.hide();
    }
}

#[tauri::command]
fn close_main(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

#[tauri::command]
fn window_minimize(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.minimize();
    }
}

#[tauri::command]
fn window_toggle_maximize(app: AppHandle) -> bool {
    if let Some(w) = app.get_webview_window("main") {
        let maximized = w.is_maximized().unwrap_or(false);
        if maximized {
            let _ = w.unmaximize();
            return false;
        }
        let _ = w.maximize();
        return true;
    }
    false
}

#[tauri::command]
fn window_is_maximized(app: AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.is_maximized().ok())
        .unwrap_or(false)
}

#[tauri::command]
fn selection_clicked(app: AppHandle, state: tauri::State<'_, AppState>) -> String {
    #[cfg(target_os = "windows")]
    {
        selection::hide_button(&app);
    }

                        #[cfg(target_os = "windows")]
    let text = {
        let cached = selection::take_last_text();
        if !cached.is_empty() {
            cached
        } else {
                        let point =
                selection::take_selection_point().unwrap_or_else(selection::cursor_position);
            match selection::read_selection_via_uia(point) {
                Some(t) => t,
                None => selection::capture_selection_text().unwrap_or_default(),
            }
        }
    };
    #[cfg(not(target_os = "windows"))]
    let text = String::new();

        if text.is_empty() {
        return String::new();
    }

        let max_len = state.config(&app).selection_threshold as usize;
    if max_len > 0 && text.chars().count() > max_len {
        return String::new();
    }

    let pos = {
        #[cfg(target_os = "windows")]
        {
            selection::cursor_position()
        }
        #[cfg(not(target_os = "windows"))]
        {
            (0, 0)
        }
    };

    show_popup(&app, &text, Some(pos));
    text
}

#[tauri::command]
fn get_last_selection() -> String {
    #[cfg(target_os = "windows")]
    {
        selection::peek_last_text()
    }
    #[cfg(not(target_os = "windows"))]
    {
        String::new()
    }
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    let trimmed = url.trim();

        let allowed = trimmed.starts_with("https://") || trimmed.starts_with("http://");
    if !allowed {
        return Err("只允许打开 http/https 链接".into());
    }

    tauri_plugin_opener::open_url(trimmed.to_string(), None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_config_dir(app: AppHandle) -> Result<(), String> {
    let path = config::config_path(&app)?;
    let dir = path.parent().ok_or("无法定位配置目录")?;
    tauri_plugin_opener::open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn reset_config(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<Config, String> {
    let mut fresh = Config::default();
    fresh.window = state.config(&app).window;

    config::save(&app, &fresh)?;
    register_hotkeys(&app, &fresh.hotkeys)?;
    apply_theme(&app, &fresh.theme);
    tray::apply_state(&app, fresh.selection_enabled);

    state.replace(fresh.clone());
    let _ = app.emit("config-changed", &fresh);

    Ok(fresh)
}

#[tauri::command]
fn open_settings(app: AppHandle) {
    if let Some(popup) = app.get_webview_window(POPUP_LABEL) {
        let _ = popup.hide();
    }
    show_main(&app);
    let _ = app.emit_to("main", "switch-to-settings", ());
}


#[tauri::command]
fn get_autostart(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|e| format!("设置开机自启失败：{e}"))
}


static MOCK_UPDATE: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: String,
    pub mock: bool,
}

fn mock_update_info() -> UpdateInfo {
    UpdateInfo {
        version: "0.9.9".into(),
        current_version: env!("CARGO_PKG_VERSION").into(),
        notes: "这是一次模拟更新，只用来测试界面流程，不会真的下载或安装任何东西。\n\n\
                · 修复了划词取词偶发失败的问题\n\
                · 新增源语言选择\n\
                · 更新窗口界面重做"
            .into(),
        mock: true,
    }
}

async fn check_update_info(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    use tauri_plugin_updater::UpdaterExt;

    if MOCK_UPDATE.load(Ordering::SeqCst) {
        return Ok(Some(mock_update_info()));
    }

    let updater = app.updater().map_err(|e| e.to_string())?;
    let found = updater.check().await.map_err(|e| e.to_string())?;

    Ok(found.map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        notes: u.body.clone().unwrap_or_default(),
        mock: false,
    }))
}

fn show_main_window(app: &AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    }
}

fn show_update_window(app: &AppHandle, info: &UpdateInfo) {
    if let Some(win) = app.get_webview_window("update") {
        let _ = win.show();
        let _ = win.set_focus();
    }

    let handle = app.clone();
    let payload = info.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(450)).await;
        let _ = handle.emit_to("update", "update-available", &payload);
    });
}

fn notify_update(app: &AppHandle, info: &UpdateInfo) {
    use tauri_plugin_notification::NotificationExt;

    let _ = app
        .notification()
        .builder()
        .title("简译有新版本")
        .body(format!("{} 可以更新了，点开简译查看", info.version))
        .show();
}

fn spawn_startup_flow(app: &AppHandle) {
    let cfg = app.state::<AppState>().config(app);
    let enabled = cfg.update.enabled;
    let auto_mode = cfg.update.mode == "auto";

        if !enabled {
        show_main_window(app);
        return;
    }

    if let Some(splash) = app.get_webview_window("splash") {
        let _ = splash.show();
    }

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let started = std::time::Instant::now();

                const SPLASH_MIN_MS: u64 = 600;

        let info = match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            check_update_info(&handle),
        )
        .await
        {
            Ok(Ok(found)) => found,
                        _ => None,
        };

        let elapsed = started.elapsed().as_millis() as u64;
        if elapsed < SPLASH_MIN_MS {
            tokio::time::sleep(std::time::Duration::from_millis(SPLASH_MIN_MS - elapsed)).await;
        }

        if let Some(splash) = handle.get_webview_window("splash") {
            let _ = splash.hide();
        }

        match info {
                        Some(update) if auto_mode => show_update_window(&handle, &update),
                        Some(update) => {
                show_main_window(&handle);
                let _ = handle.emit_to("main", "update-available", &update);
                notify_update(&handle, &update);
            }
            None => show_main_window(&handle),
        }
    });
}


fn pending_update_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("pending-update.json"))
}

fn write_pending_update(app: &AppHandle, version: &str) {
    if let Some(path) = pending_update_path(app) {
        let body = serde_json::json!({ "version": version }).to_string();
        let _ = std::fs::write(path, body);
    }
}

fn clear_pending_update(app: &AppHandle) {
    if let Some(path) = pending_update_path(app) {
        let _ = std::fs::remove_file(path);
    }
}

#[tauri::command]
fn take_pending_update(app: AppHandle) -> Option<String> {
    let path = pending_update_path(&app)?;
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);

    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[tauri::command]
async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    check_update_info(&app).await
}

#[tauri::command]
async fn start_update(app: AppHandle) -> Result<(), String> {
        if MOCK_UPDATE.load(Ordering::SeqCst) {
        for percent in (0..=100).step_by(4) {
            let _ = app.emit_to(
                "update",
                "update-progress",
                serde_json::json!({ "percent": percent }),
            );
            tokio::time::sleep(std::time::Duration::from_millis(55)).await;
        }
        let _ = app.emit_to("update", "update-done", serde_json::json!({ "mock": true }));
        return Ok(());
    }

    use tauri_plugin_updater::UpdaterExt;

    let updater = app.updater().map_err(|e| e.to_string())?;
    let found = updater.check().await.map_err(|e| e.to_string())?;
    let Some(update) = found else {
        return Err("没有可用的更新".into());
    };

        write_pending_update(&app, &update.version);

    let handle = app.clone();
    let result = update
        .download_and_install(
            move |downloaded, total| {
                let percent = total
                    .filter(|t| *t > 0)
                    .map(|t| (downloaded as f64 / t as f64 * 100.0).round() as u32)
                    .unwrap_or(0);
                let _ = handle.emit_to(
                    "update",
                    "update-progress",
                    serde_json::json!({ "percent": percent }),
                );
            },
            || {},
        )
        .await;

    match result {
        Ok(_) => {
            let _ = app.emit_to("update", "update-done", serde_json::json!({ "mock": false }));
            Ok(())
        }
        Err(e) => {
                        clear_pending_update(&app);
            Err(format!("更新失败：{e}"))
        }
    }
}

#[tauri::command]
fn mock_update(app: AppHandle, mode: String) -> Result<(), String> {
    MOCK_UPDATE.store(true, Ordering::SeqCst);
    let info = mock_update_info();

    match mode.as_str() {
        "auto" => show_update_window(&app, &info),
        "notify" => {
            show_main_window(&app);
            let _ = app.emit_to("main", "update-available", &info);
            notify_update(&app, &info);
        }
        _ => return Err("未知的模拟模式".into()),
    }

    Ok(())
}

#[tauri::command]
fn reset_mock_update() {
    MOCK_UPDATE.store(false, Ordering::SeqCst);
}

#[tauri::command]
fn open_update_window(app: AppHandle) {
    if let Some(win) = app.get_webview_window("update") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

#[tauri::command]
fn cancel_update(app: AppHandle) {
    if let Some(win) = app.get_webview_window("update") {
        let _ = win.hide();
    }
    show_main_window(&app);
}


pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        on_shortcut(app, shortcut);
                    }
                })
                .build(),
        )
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            get_config,
            get_config_path,
            save_config,
            get_autostart,
            set_autostart,
            check_update,
            start_update,
            mock_update,
            reset_mock_update,
            cancel_update,
            open_update_window,
            take_pending_update,
            reset_scene_prompt,
            popup_set_pinned,
            popup_is_pinned,
            popup_ready,
            close_popup,
            close_main,
            window_minimize,
            window_toggle_maximize,
            window_is_maximized,
            selection_clicked,
            get_last_selection,
            open_config_dir,
            open_url,
            open_settings,
            reset_config,
            translate::translate,
            translate::test_api,
            translate::fetch_models,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

                                    let cfg = app.state::<AppState>().config(&handle);
            let theme = cfg.theme.clone();
            let hotkeys = cfg.hotkeys.clone();

                        SELECTION_ENABLED.store(cfg.selection_enabled, Ordering::SeqCst);
            let _ = tray::create(&handle);

                        let _ = register_hotkeys(&handle, &hotkeys);

                        apply_theme(&handle, &theme);

                        let position_restored = apply_window_state(&handle, &cfg.window);

                        if let Some(main) = app.get_webview_window("main") {
                let win = main.clone();
                let h = handle.clone();
                main.on_window_event(move |event| match event {
                    tauri::WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        save_window_state(&h);
                        let _ = win.hide();
                    }
                    tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) => {
                        schedule_save_window_state(&h);
                    }
                    _ => {}
                });
            }

                        spawn_startup_flow(&handle);

                                    if !position_restored {
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                    save_window_state(&h);
                });
            }

                        if let Some(popup) = app.get_webview_window(POPUP_LABEL) {
                let win = popup.clone();
                popup.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(false) = event {
                        if !POPUP_PINNED.load(Ordering::SeqCst) {
                            let _ = win.hide();
                        }
                    }
                });
            }

                        #[cfg(target_os = "windows")]
            if let Some(btn) = app.get_webview_window("selection") {
                let _ = btn.set_size(tauri::LogicalSize::new(
                    selection::SELECTION_WINDOW_SIZE as f64,
                    selection::SELECTION_WINDOW_SIZE as f64,
                ));
                selection::make_noactivate(&btn);
            }

                        #[cfg(target_os = "windows")]
            selection::start(handle.clone());

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("SimpleTranslation 启动失败");

    app.run(|handle, event| {
                if let tauri::RunEvent::Exit = event {
            save_window_state(handle);
        }
    });
}
