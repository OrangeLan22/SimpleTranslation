#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod translate;
mod tray;

#[cfg(target_os = "windows")]
mod selection;

use serde::{Deserialize, Serialize};
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

            for label in ["main", POPUP_LABEL, "selection", "splash", "notify"] {
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

const VERSION_CHANNEL: &str = "v";

#[tauri::command]
fn get_app_version() -> String {
    format!("{}{}", VERSION_CHANNEL, env!("CARGO_PKG_VERSION"))
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

pub fn selection_blocked_by_rules(cfg: &Config) -> Option<String> {
    let rules = &cfg.selection_rules;

        if rules.skip_fullscreen && selection::is_foreground_fullscreen() {
        return Some("前台处于全屏模式".into());
    }

        if !rules.excluded_apps.is_empty() {
        let current = selection::foreground_process_name();
        if !current.is_empty() {
            let hit = rules
                .excluded_apps
                .iter()
                .any(|name| name.trim().to_lowercase() == current);
            if hit {
                return Some(format!("程序 {current} 在排除列表中"));
            }
        }
    }

    None
}

#[tauri::command]
fn list_running_apps() -> Vec<serde_json::Value> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible,
    };

    struct CollectCtx {
        items: std::collections::BTreeMap<String, String>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam.0 as *mut CollectCtx);

        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }

                let title_len = GetWindowTextLengthW(hwnd);
        if title_len <= 0 {
            return BOOL(1);
        }

        let mut buf = vec![0u16; (title_len + 1) as usize];
        let len = GetWindowTextW(hwnd, &mut buf);
        if len <= 0 {
            return BOOL(1);
        }
        let title = String::from_utf16_lossy(&buf[..len as usize]);

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return BOOL(1);
        }

        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return BOOL(1);
        };

        let mut path_buf = [0u16; 260];
        let mut path_len = path_buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(path_buf.as_mut_ptr()),
            &mut path_len,
        );
        let _ = windows::Win32::Foundation::CloseHandle(process);

        if ok.is_err() {
            return BOOL(1);
        }

        let full = String::from_utf16_lossy(&path_buf[..path_len as usize]);
        let name = full
            .rsplit(|c| c == '\\' || c == '/')
            .next()
            .map(|n| {
                let lower = n.to_lowercase();
                lower
                    .strip_suffix(".exe")
                    .map(|s| s.to_string())
                    .unwrap_or(lower)
            })
            .unwrap_or_default();

        if name.is_empty() {
            return BOOL(1);
        }

                ctx.items.entry(name).or_insert(title);
        BOOL(1)
    }

    let mut ctx = CollectCtx {
        items: std::collections::BTreeMap::new(),
    };

    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut CollectCtx as isize));
    }

    ctx.items
        .into_iter()
        .map(|(name, title)| serde_json::json!({ "name": name, "title": title }))
        .collect()
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
        pub version: String,
        pub current_version: String,
        pub notes: String,
        pub page_url: String,
        pub download_url: String,
}

const GITEE_API: &str = "https://gitee.com/api/v5/repos/orangelan/SimpleTranslationDL/releases/latest";

fn parse_version(raw: &str) -> Option<(u64, u64, u64)> {
    let trimmed = raw.trim().trim_start_matches('v').trim_start_matches("beta");

        let core: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();

    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

fn is_newer(remote: &str, current: &str) -> bool {
    match (parse_version(remote), parse_version(current)) {
        (Some(r), Some(c)) => r > c,
                _ => false,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseEntry {
        pub version: String,
        pub name: String,
        pub notes: String,
        pub page_url: String,
        pub published_at: String,
        pub is_current: bool,
}

const GITEE_RELEASES: &str =
    "https://gitee.com/api/v5/repos/orangelan/SimpleTranslationDL/releases";

const LATEST_TTL_SECS: u64 = 10 * 60;

const RELEASES_TTL_SECS: u64 = 30 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct UpdateCache {
    #[serde(default)]
    releases: Vec<ReleaseEntry>,
    #[serde(default)]
    releases_at: u64,
    #[serde(default)]
    latest_tag: String,
    #[serde(default)]
    latest_checked_for: String,
    #[serde(default)]
    latest_info: Option<UpdateInfo>,
    #[serde(default)]
    latest_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleasesResult {
    releases: Vec<ReleaseEntry>,
        from_cache: bool,
        cached_at: String,
}

fn update_cache_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("update_cache.json"))
}

fn load_update_cache(app: &AppHandle) -> UpdateCache {
    let Some(path) = update_cache_path(app) else {
        return UpdateCache::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return UpdateCache::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_update_cache(app: &AppHandle, cache: &UpdateCache) {
    let Some(path) = update_cache_path(app) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(cache) {
        let _ = std::fs::write(&path, text);
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn format_cache_time(secs: u64) -> String {
    if secs == 0 {
        return String::new();
    }
        let delta = now_secs().saturating_sub(secs);
    if delta < 60 {
        "刚刚".into()
    } else if delta < 3600 {
        format!("{} 分钟前", delta / 60)
    } else if delta < 86400 {
        format!("{} 小时前", delta / 3600)
    } else {
        format!("{} 天前", delta / 86400)
    }
}

fn friendly_net_error(status: u16, raw: String) -> String {
    match status {
        403 => "请求过于频繁，本机IP已被限流，请等待恢复或直接前往项目地址查看".into(),
        404 => "Gitee 上还没有发布任何版本。".into(),
        429 => "请求太频繁，请过几分钟再试或直接前往项目地址查看。".into(),
        _ => raw,
    }
}

async fn fetch_releases() -> Result<Vec<ReleaseEntry>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("SimpleTranslation")
        .build()
        .map_err(|e| format!("初始化网络请求失败：{e}"))?;

    let url = format!("{GITEE_RELEASES}?page=1&per_page=100");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("无法连接 Gitee：{e}"))?;

    if !resp.status().is_success() {
        let code = resp.status().as_u16();
        return Err(friendly_net_error(
            code,
            format!("Gitee 返回 HTTP {code}"),
        ));
    }

    let body: Vec<serde_json::Value> = resp
        .json()
        .await
        .map_err(|e| format!("解析 Gitee 响应失败：{e}"))?;

    let current = env!("CARGO_PKG_VERSION");
    let current_key = parse_version(current);

    let mut list: Vec<ReleaseEntry> = body
        .iter()
        .filter_map(|item| {
            let tag = item.get("tag_name").and_then(|v| v.as_str())?.trim();
            if tag.is_empty() {
                return None;
            }
            let version = tag.trim_start_matches('v').to_string();

            let is_current = match (parse_version(&version), current_key) {
                (Some(a), Some(b)) => a == b,
                _ => version == current,
            };

            Some(ReleaseEntry {
                is_current,
                name: item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                notes: item
                    .get("body")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                page_url: format!(
                    "https://gitee.com/orangelan/SimpleTranslationDL/releases/tag/{tag}"
                ),
                published_at: item
                    .get("created_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .chars()
                    .take(10)
                    .collect(),
                version,
            })
        })
        .collect();

        list.sort_by(|a, b| match (parse_version(&a.version), parse_version(&b.version)) {
        (Some(x), Some(y)) => y.cmp(&x),
        _ => b.version.cmp(&a.version),
    });

    Ok(list)
}

fn refresh_current_flags(list: &mut [ReleaseEntry]) {
    let current = env!("CARGO_PKG_VERSION");
    let key = parse_version(current);
    for item in list.iter_mut() {
        item.is_current = match (parse_version(&item.version), key) {
            (Some(a), Some(b)) => a == b,
            _ => item.version == current,
        };
    }
}

#[tauri::command]
async fn list_releases(app: AppHandle) -> Result<ReleasesResult, String> {
    let mut cache = load_update_cache(&app);
    let age = now_secs().saturating_sub(cache.releases_at);

    if !cache.releases.is_empty() && age < RELEASES_TTL_SECS {
        let mut list = cache.releases;
        refresh_current_flags(&mut list);
        return Ok(ReleasesResult {
            releases: list,
            from_cache: true,
            cached_at: format_cache_time(cache.releases_at),
        });
    }

    match tokio::time::timeout(std::time::Duration::from_secs(20), fetch_releases()).await {
        Ok(Ok(list)) => {
            cache.releases = list.clone();
            cache.releases_at = now_secs();
            save_update_cache(&app, &cache);
            Ok(ReleasesResult {
                releases: list,
                from_cache: false,
                cached_at: String::new(),
            })
        }
        Ok(Err(err)) => {
                        if !cache.releases.is_empty() {
                let mut list = cache.releases;
                refresh_current_flags(&mut list);
                Ok(ReleasesResult {
                    cached_at: format_cache_time(cache.releases_at),
                    from_cache: true,
                    releases: list,
                })
            } else {
                Err(err)
            }
        }
        Err(_) => {
            if !cache.releases.is_empty() {
                let mut list = cache.releases;
                refresh_current_flags(&mut list);
                Ok(ReleasesResult {
                    cached_at: format_cache_time(cache.releases_at),
                    from_cache: true,
                    releases: list,
                })
            } else {
                Err("获取更新日志超时（20 秒）。请确认网络能访问 gitee.com".into())
            }
        }
    }
}

async fn fetch_latest_release(current: &str) -> Result<Option<UpdateInfo>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("SimpleTranslation")
        .build()
        .map_err(|e| format!("初始化网络请求失败：{e}"))?;

    let resp = client
        .get(GITEE_API)
        .send()
        .await
        .map_err(|e| format!("无法连接 Gitee：{e}"))?;

    if !resp.status().is_success() {
        let code = resp.status().as_u16();
        return Err(friendly_net_error(
            code,
            format!("Gitee 返回 HTTP {code}"),
        ));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 Gitee 响应失败：{e}"))?;

    let tag = body
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    if tag.is_empty() {
        return Err("Gitee 响应里没有版本号".into());
    }

    if !is_newer(&tag, current) {
        return Ok(None);
    }

    let notes = body
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

        let page_url = format!(
        "https://gitee.com/orangelan/SimpleTranslationDL/releases/tag/{tag}"
    );

        let download_url = body
        .get("assets")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|a| {
                    a.get("name")
                        .and_then(|n| n.as_str())
                        .map(|n| n.to_lowercase().ends_with(".exe"))
                        .unwrap_or(false)
                })
                .and_then(|a| a.get("browser_download_url"))
                .and_then(|u| u.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    Ok(Some(UpdateInfo {
        version: tag.trim_start_matches('v').to_string(),
        current_version: current.to_string(),
        notes,
        page_url,
        download_url,
    }))
}

#[cfg(target_os = "windows")]
fn place_notify_bottom_right(win: &WebviewWindow) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };

    let mut area = RECT::default();
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut area as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if ok.is_err() {
        return;
    }

    let scale = win.scale_factor().unwrap_or(1.0);
    let size = win
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new((348.0 * scale) as u32, (112.0 * scale) as u32));

    let margin = (14.0 * scale) as i32;
    let x = area.right - size.width as i32 - margin;
    let y = area.bottom - size.height as i32 - margin;

    let _ = win.set_position(PhysicalPosition::new(x, y));
}

#[cfg(not(target_os = "windows"))]
fn place_notify_bottom_right(_win: &WebviewWindow) {}

static PENDING_NOTIFY: Mutex<Option<UpdateInfo>> = Mutex::new(None);

#[tauri::command]
fn take_pending_notify() -> Option<UpdateInfo> {
    PENDING_NOTIFY.lock().ok().and_then(|mut guard| guard.take())
}

fn show_main_window(app: &AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
                if main.is_minimized().unwrap_or(false) {
            let _ = main.unminimize();
        }
        let _ = main.set_focus();
    }
}

fn notify_update(app: &AppHandle, info: &UpdateInfo) {
    let Some(win) = app.get_webview_window("notify") else {
        return;
    };

    let _ = win.show();
    place_notify_bottom_right(&win);

    #[cfg(target_os = "windows")]
    crate::selection::make_noactivate(&win);

    if let Ok(mut guard) = PENDING_NOTIFY.lock() {
        *guard = Some(info.clone());
    }

    let handle = app.clone();
    let payload = info.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(320)).await;
        let _ = handle.emit_to("notify", "notify-show", &payload);
    });
}

#[tauri::command]
fn close_notify(app: AppHandle) {
    if let Some(win) = app.get_webview_window("notify") {
        let _ = win.hide();
    }
}

#[tauri::command]
fn show_update_log(app: AppHandle) {
    show_main_window(&app);

        let payload = PENDING_NOTIFY
        .lock()
        .ok()
        .and_then(|guard| guard.clone());

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(260)).await;
        if let Some(info) = payload {
            let _ = handle.emit_to("main", "show-update-log", &info);
        }
    });
}

fn launched_from_autostart() -> bool {
    std::env::args().any(|arg| arg == "--autostart")
}

fn refresh_autostart_args(app: &AppHandle) {
    use tauri_plugin_autostart::ManagerExt;

    let manager = app.autolaunch();
    if !manager.is_enabled().unwrap_or(false) {
        return;
    }

                let _ = manager.disable();
    let _ = manager.enable();
}

fn spawn_startup_flow(app: &AppHandle) {
    let cfg = app.state::<AppState>().config(app);
    let enabled = cfg.update.enabled;

                if launched_from_autostart() {
        if enabled {
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                background_update_check(&handle).await;
            });
        }
        return;
    }

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

        let elapsed = started.elapsed().as_millis() as u64;
        if elapsed < SPLASH_MIN_MS {
            tokio::time::sleep(std::time::Duration::from_millis(SPLASH_MIN_MS - elapsed)).await;
        }

        if let Some(splash) = handle.get_webview_window("splash") {
            let _ = splash.hide();
        }

        show_main_window(&handle);

        let handle2 = handle.clone();
        tauri::async_runtime::spawn(async move {
            background_update_check(&handle2).await;
        });
    });
}

async fn background_update_check(app: &AppHandle) {
    let cfg = app.state::<AppState>().config(app);
    if !cfg.update.enabled {
        return;
    }

        tokio::time::sleep(std::time::Duration::from_millis(900)).await;

    let current = env!("CARGO_PKG_VERSION");

    match fetch_latest_release(current).await {
        Ok(Some(info)) => {
            let _ = app.emit_to("main", "update-available", &info);
            notify_update(app, &info);
        }
        Ok(None) => {
            let _ = app.emit_to(
                "main",
                "update-check-result",
                serde_json::json!({ "upToDate": true }),
            );
        }
        Err(err) => {
            let _ = app.emit_to(
                "main",
                "update-check-result",
                serde_json::json!({ "error": err }),
            );
        }
    }
}

#[tauri::command]
async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let current = env!("CARGO_PKG_VERSION");
    let mut cache = load_update_cache(&app);
    let age = now_secs().saturating_sub(cache.latest_at);

        if cache.latest_checked_for == current && age < LATEST_TTL_SECS {
        return Ok(cache.latest_info.clone());
    }

    match tokio::time::timeout(
        std::time::Duration::from_secs(20),
        fetch_latest_release(current),
    )
    .await
    {
        Ok(Ok(info)) => {
            cache.latest_checked_for = current.to_string();
            cache.latest_info = info.clone();
            cache.latest_at = now_secs();
            save_update_cache(&app, &cache);
            Ok(info)
        }
        Ok(Err(err)) => {
            if cache.latest_checked_for == current {
                Ok(cache.latest_info.clone())
            } else {
                Err(err)
            }
        }
        Err(_) => {
            if cache.latest_checked_for == current {
                Ok(cache.latest_info.clone())
            } else {
                Err("检查超时（20 秒）。请确认网络能访问 gitee.com".into())
            }
        }
    }
}


#[cfg(target_os = "windows")]
fn init_app_user_model_id() {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(&HSTRING::from("com.simpletranslation.app"));
    }
}

#[cfg(not(target_os = "windows"))]
fn init_app_user_model_id() {}

pub fn run() {
        init_app_user_model_id();

    let app = tauri::Builder::default()
                .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
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
            list_releases,
            list_running_apps,
            close_notify,
            show_update_log,
            take_pending_notify,
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
            get_app_version,
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

                        refresh_autostart_args(&handle);

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
