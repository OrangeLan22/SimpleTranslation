
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};
use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::core::implement;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationEventHandler,
    IUIAutomationEventHandler_Impl, IUIAutomationTextPattern, TextPatternRangeEndpoint_End,
    TextPatternRangeEndpoint_Start, TreeScope_Subtree, UIA_Text_TextSelectionChangedEventId,
    UIA_EVENT_ID, UIA_TextPatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, MAPVK_VK_TO_VSC,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_C, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN,
    VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetClassNameW, GetCursorPos, GetForegroundWindow,
    GetMessageW, GetWindowThreadProcessId, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_SYSKEYDOWN,
};

fn debug_log(_message: impl AsRef<str>) {
    }

enum HookEvent {
        Candidate(i32, i32),
        Dismiss,
}

static HOOK_TX: OnceLock<Sender<HookEvent>> = OnceLock::new();
static LAST_TEXT: Mutex<String> = Mutex::new(String::new());
static BUTTON_RECT: Mutex<Option<(i32, i32, i32, i32)>> = Mutex::new(None);
static PRESSED: AtomicBool = AtomicBool::new(false);
static DRAGGED: AtomicBool = AtomicBool::new(false);
static DOWN_X: AtomicI32 = AtomicI32::new(0);
static DOWN_Y: AtomicI32 = AtomicI32::new(0);
static LAST_UP_TICK: AtomicU64 = AtomicU64::new(0);
static BUTTON_SHOWN_AT: AtomicU64 = AtomicU64::new(0);

static SELECTION_POINT: Mutex<Option<(i32, i32)>> = Mutex::new(None);

const SELECTION_WINDOW: &str = "selection";

pub const SELECTION_BUTTON_SIZE: u32 = 38;

const SELECTION_PADDING: u32 = 4;

pub const SELECTION_WINDOW_SIZE: u32 = SELECTION_BUTTON_SIZE + SELECTION_PADDING * 2;


fn point_in_button(x: i32, y: i32) -> bool {
    if let Ok(rect) = BUTTON_RECT.lock() {
        if let Some((bx, by, bw, bh)) = *rect {
            return x >= bx - 4 && x <= bx + bw + 4 && y >= by - 4 && y <= by + bh + 4;
        }
    }
    false
}

fn foreground_is_self() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid != 0 && pid == GetCurrentProcessId()
    }
}

fn foreground_class_name() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 256];
        let len = GetClassNameW(hwnd, &mut buf);
        if len <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..len as usize])
    }
}

fn is_desktop_or_shell() -> bool {
    matches!(
        foreground_class_name().as_str(),
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
    )
}

fn shift_pressed() -> bool {
    unsafe { GetAsyncKeyState(VK_SHIFT.0 as i32) < 0 }
}

fn notify(ev: HookEvent) {
    if let Some(tx) = HOOK_TX.get() {
        let _ = tx.send(ev);
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;

        match msg {
            WM_LBUTTONDOWN => {
                let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
                let (x, y) = (info.pt.x, info.pt.y);

                PRESSED.store(true, Ordering::Relaxed);
                DRAGGED.store(false, Ordering::Relaxed);
                DOWN_X.store(x, Ordering::Relaxed);
                DOWN_Y.store(y, Ordering::Relaxed);

                                if !point_in_button(x, y) {
                    notify(HookEvent::Dismiss);
                }
            }
            WM_MOUSEMOVE => {
                if PRESSED.load(Ordering::Relaxed) {
                    let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
                    let dx = (info.pt.x - DOWN_X.load(Ordering::Relaxed)).abs();
                    let dy = (info.pt.y - DOWN_Y.load(Ordering::Relaxed)).abs();
                    if dx > 4 || dy > 4 {
                        DRAGGED.store(true, Ordering::Relaxed);
                    }
                }
            }
            WM_MOUSEWHEEL => {
                                notify(HookEvent::Dismiss);
            }
            WM_LBUTTONUP => {
                let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
                let (x, y) = (info.pt.x, info.pt.y);

                let was_pressed = PRESSED.swap(false, Ordering::Relaxed);
                let dragged = DRAGGED.load(Ordering::Relaxed);
                let now = GetTickCount64();
                let last = LAST_UP_TICK.load(Ordering::Relaxed);
                let near = (x - DOWN_X.load(Ordering::Relaxed)).abs() < 6
                    && (y - DOWN_Y.load(Ordering::Relaxed)).abs() < 6;
                let is_multi_click =
                    was_pressed && last != 0 && now.saturating_sub(last) < 450 && near;
                LAST_UP_TICK.store(now, Ordering::Relaxed);

                                                                                let shift_click = shift_pressed();
                if (dragged || is_multi_click || shift_click) && !point_in_button(x, y) {
                    notify(HookEvent::Candidate(x, y));
                }
            }
            _ => {}
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
            let vk = (*(lparam.0 as *const KBDLLHOOKSTRUCT)).vkCode as u16;

                                                let is_modifier = [
                VK_SHIFT,
                VK_LSHIFT,
                VK_RSHIFT,
                VK_CONTROL,
                VK_LCONTROL,
                VK_RCONTROL,
                VK_MENU,
                VK_LMENU,
                VK_RMENU,
                VK_LWIN,
                VK_RWIN,
            ]
            .contains(&VIRTUAL_KEY(vk));

            if !is_modifier {
                notify(HookEvent::Dismiss);
            }
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}

pub fn start(app: AppHandle) {
        start_selection_watcher();

    let (tx, rx) = channel::<HookEvent>();
    if HOOK_TX.set(tx).is_err() {
        return;
    }

        std::thread::spawn(move || unsafe {
        let Ok(mouse_hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0) else {
            return;
        };
        let keyboard_hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0).ok();

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = UnhookWindowsHookEx(mouse_hook);
        if let Some(hook) = keyboard_hook {
            let _ = UnhookWindowsHookEx(hook);
        }
    });

    std::thread::spawn(move || {
                unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        while let Ok(event) = rx.recv() {
            match event {
                HookEvent::Candidate(x, y) => handle_mouse_up(&app, x, y),
                HookEvent::Dismiss => handle_dismiss(&app),
            }
        }
    });
}

fn handle_mouse_up(app: &AppHandle, x: i32, y: i32) {
        if !crate::SELECTION_ENABLED.load(Ordering::SeqCst) {
        hide_button(app);
        return;
    }

        if point_in_button(x, y) {
        return;
    }

            if foreground_is_self() {
        hide_button(app);
        return;
    }

            if is_desktop_or_shell() {
        hide_button(app);
        return;
    }

    let mode = app.state::<crate::AppState>().config(app).trigger.mode;

    hide_button(app);

        if mode == "hotkey" {
        return;
    }

    debug_log(format!("── 鼠标在 ({x}, {y}) 抬起，开始判断有没有选中文本"));

    let max_len = app
        .state::<crate::AppState>()
        .config(app)
        .selection_threshold as usize;

        std::thread::sleep(Duration::from_millis(60));

                if let Some(text) = take_cached_selection(3000) {
        let len = text.chars().count();
        debug_log(format!("① 选区广播：拿到 {len} 个字符（松手前的最后一次变化）"));

        if max_len > 0 && len > max_len {
            debug_log(format!("   但超过设定上限 {max_len} 字，判定为误选，不弹按钮"));
            return;
        }

        if let Ok(mut guard) = LAST_TEXT.lock() {
            *guard = text;
        }
        debug_log("=> 结论：显示悬浮按钮");
        show_button(app, x, y);
        return;
    }

    debug_log("① 选区广播：没有缓存 → 改用主动探测");

                    let has_selection = match detect_text_selection((x, y)) {
        Some(_) => true,
        None => {
            debug_log("② 剪贴板探测：备份剪贴板 → 临时 Ctrl+C → 读取 → 立刻还原");
            let got = capture_selection_text();
            match &got {
                Some(text) => {
                    let len = text.chars().count();
                    debug_log(format!("② 剪贴板探测：复制到了新内容（{len} 个字符）"));
                    if max_len > 0 && len > max_len {
                        debug_log(format!("   超过设定上限 {max_len} 字，判定为误选，不弹按钮"));
                        return;
                    }
                }
                None => debug_log("② 剪贴板探测：没有复制到新内容"),
            }
            if let Some(text) = &got {
                if let Ok(mut guard) = LAST_TEXT.lock() {
                    *guard = text.clone();
                }
            }
            got.is_some()
        }
    };

    if has_selection {
        debug_log("=> 结论：显示悬浮按钮");
        show_button(app, x, y);
    } else {
        debug_log("=> 结论：不显示按钮");
    }
}


fn keyboard_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
            let scan = unsafe { MapVirtualKeyW(vk.0 as u32, MAPVK_VK_TO_VSC) } as u16;

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn is_key_down(vk: u16) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

pub fn wait_modifiers_released(timeout_ms: u64) {
    let start = std::time::Instant::now();
    loop {
        let busy = is_key_down(VK_LSHIFT.0)
            || is_key_down(VK_RSHIFT.0)
            || is_key_down(VK_LMENU.0)
            || is_key_down(VK_RMENU.0)
            || is_key_down(VK_LWIN.0)
            || is_key_down(VK_RWIN.0);

        if !busy || start.elapsed().as_millis() as u64 >= timeout_ms {
            return;
        }
        std::thread::sleep(Duration::from_millis(8));
    }
}

fn send_ctrl_c() {
    let send = |vk: VIRTUAL_KEY, up: bool| {
        let inputs = [keyboard_input(vk, up)];
        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    };

    send(VK_CONTROL, false);
    std::thread::sleep(Duration::from_millis(15));
    send(VK_C, false);
    std::thread::sleep(Duration::from_millis(15));
    send(VK_C, true);
    std::thread::sleep(Duration::from_millis(15));
    send(VK_CONTROL, true);
}

fn read_clipboard_text() -> Option<String> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    clipboard.get_text().ok()
}

fn read_clipboard_text_retry() -> Option<String> {
    for attempt in 0..6 {
        if let Some(text) = read_clipboard_text() {
            return Some(text);
        }
        if attempt < 5 {
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    None
}

pub fn detect_text_selection(point: (i32, i32)) -> Option<bool> {
    let result = unsafe {
        (|| -> Option<bool> {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;

            let pt = POINT {
                x: point.0,
                y: point.1,
            };
            let element = automation.ElementFromPoint(pt).ok()?;

            let pattern = element
                .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                .ok()?;
            let ranges = pattern.GetSelection().ok()?;
            let count = ranges.Length().unwrap_or(0);

            for i in 0..count {
                let Ok(range) = ranges.GetElement(i) else {
                    continue;
                };
                                match range.GetText(64) {
                    Ok(text) => {
                        if !text.to_string().trim().is_empty() {
                            return Some(true);
                        }
                    }
                                        Err(_) => return Some(true),
                }
            }

                                    None
        })()
    };

    match result {
        Some(_) => debug_log("① 系统无障碍接口：查到了选中的文本"),
        None => debug_log("① 系统无障碍接口：查不到选区 → 改用剪贴板探测"),
    }

    result
}

unsafe fn element_selection_text(element: &IUIAutomationElement) -> Option<String> {
    let pattern = element
        .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        .ok()?;
    let ranges = pattern.GetSelection().ok()?;
    let count = ranges.Length().unwrap_or(0);

    let mut collected = String::new();
    for i in 0..count {
        let Ok(range) = ranges.GetElement(i) else {
            continue;
        };
        let Ok(part) = range.GetText(-1) else {
            continue;
        };
        collected.push_str(&part.to_string());
    }

    let trimmed = collected.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn read_selection_via_uia(point: (i32, i32)) -> Option<String> {
    let text = unsafe {
        (|| -> Option<String> {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let pt = POINT {
                x: point.0,
                y: point.1,
            };
            let element = automation.ElementFromPoint(pt).ok()?;
            element_selection_text(&element)
        })()
    };

    match text {
        Some(t) if t.chars().count() <= 8000 => Some(t),
        _ => None,
    }
}


struct CachedSelection {
    text: String,
    at: u64,
}

static SELECTION_CACHE: Mutex<Option<CachedSelection>> = Mutex::new(None);

fn cache_selection(text: String) {
    if let Ok(mut cache) = SELECTION_CACHE.lock() {
        *cache = Some(CachedSelection {
            text,
            at: unsafe { GetTickCount64() },
        });
    }
}

fn take_cached_selection(max_age_ms: u64) -> Option<String> {
    let mut cache = SELECTION_CACHE.lock().ok()?;
    let cached = cache.take()?;
    let now = unsafe { GetTickCount64() };
    if now.saturating_sub(cached.at) <= max_age_ms {
        Some(cached.text)
    } else {
        None
    }
}

unsafe fn element_selection_text_strict(element: &IUIAutomationElement) -> Option<String> {
    let pattern = element
        .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        .ok()?;
    let ranges = pattern.GetSelection().ok()?;
    let count = ranges.Length().unwrap_or(0);

    let mut collected = String::new();
    for i in 0..count {
        let Ok(range) = ranges.GetElement(i) else {
            continue;
        };

                if let Ok(cmp) = range.CompareEndpoints(
            TextPatternRangeEndpoint_Start,
            &range,
            TextPatternRangeEndpoint_End,
        ) {
            if cmp == 0 {
                continue;
            }
        }

        let Ok(part) = range.GetText(-1) else {
            continue;
        };
        collected.push_str(&part.to_string());
    }

    let trimmed = collected.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[implement(IUIAutomationEventHandler)]
struct SelectionWatcher;

impl IUIAutomationEventHandler_Impl for SelectionWatcher_Impl {
    fn HandleAutomationEvent(
        &self,
        sender: Option<&IUIAutomationElement>,
        _eventid: UIA_EVENT_ID,
    ) -> windows::core::Result<()> {
        if let Some(element) = sender {
            unsafe {
                if let Some(text) = element_selection_text_strict(element) {
                    cache_selection(text);
                }
            }
        }
        Ok(())
    }
}

pub fn start_selection_watcher() {
    std::thread::spawn(|| unsafe {
        let _ = CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);

        let Ok(automation) =
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        else {
            debug_log("选区广播：创建 UIA 实例失败，退回主动探测");
            return;
        };

        let Ok(root) = automation.GetRootElement() else {
            return;
        };

        let handler: IUIAutomationEventHandler = SelectionWatcher.into();
        let registered = automation.AddAutomationEventHandler(
            UIA_Text_TextSelectionChangedEventId,
            &root,
            TreeScope_Subtree,
            None,
            &handler,
        );

        match registered {
            Ok(()) => debug_log("选区广播：已开始监听全局选区变化"),
            Err(_) => {
                debug_log("选区广播：注册失败，退回主动探测");
                return;
            }
        }

                loop {
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}

fn restore_clipboard(backup: Option<&str>) {
    let Ok(mut clipboard) = arboard::Clipboard::new() else {
        return;
    };

    match backup {
        Some(text) => {
            let _ = clipboard.set_text(text.to_string());
        }
                None => {
            let _ = clipboard.clear();
        }
    }
}

pub fn capture_selection_text() -> Option<String> {
        wait_modifiers_released(700);

        let backup = read_clipboard_text();
    let before = unsafe { GetClipboardSequenceNumber() };

    send_ctrl_c();

        let mut changed = false;
    for _ in 0..25 {
        std::thread::sleep(Duration::from_millis(20));
        if unsafe { GetClipboardSequenceNumber() } != before {
            changed = true;
            break;
        }
    }

    if !changed {
                debug_log("      · Ctrl+C 之后剪贴板内容没有变化（没有产生新选区）");
        return None;
    }

    let copied = read_clipboard_text_retry();
        restore_clipboard(backup.as_deref());

    let Some(text) = copied else {
        debug_log("      · 剪贴板读取失败（可能正被其他程序占用）");
        return None;
    };

    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 8000 {
        return None;
    }
    Some(trimmed.to_string())
}

pub fn cursor_position() -> (i32, i32) {
    let mut point = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut point);
    }
    (point.x, point.y)
}


fn show_button(app: &AppHandle, x: i32, y: i32) {
    let Some(win) = app.get_webview_window(SELECTION_WINDOW) else {
        return;
    };

            let _ = win.set_size(tauri::LogicalSize::new(
        SELECTION_WINDOW_SIZE as f64,
        SELECTION_WINDOW_SIZE as f64,
    ));

    let size = win
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new(69, 69));
    let w = size.width as i32;
    let h = size.height as i32;

        let mut px = x + 10;
    let mut py = y + 10;

    if let Ok(Some(monitor)) = win.current_monitor() {
        let ms = monitor.size();
        let mp = monitor.position();
        let right = mp.x + ms.width as i32;
        let bottom = mp.y + ms.height as i32;
        if px + w > right - 4 {
            px = x - w - 14;
        }
        if py + h > bottom - 4 {
            py = y - h - 14;
        }
    }

    let _ = win.set_position(PhysicalPosition::new(px, py));
    let _ = win.show();

            let _ = app.emit_to(SELECTION_WINDOW, "selection-shown", ());

    BUTTON_SHOWN_AT.store(unsafe { GetTickCount64() }, Ordering::Relaxed);
    if let Ok(mut rect) = BUTTON_RECT.lock() {
        *rect = Some((px, py, w, h));
    }
        if let Ok(mut point) = SELECTION_POINT.lock() {
        *point = Some((x, y));
    }
}

pub fn take_selection_point() -> Option<(i32, i32)> {
    SELECTION_POINT.lock().ok().and_then(|mut guard| guard.take())
}

fn handle_dismiss(app: &AppHandle) {
    let shown_at = BUTTON_SHOWN_AT.load(Ordering::Relaxed);
    if shown_at != 0 && unsafe { GetTickCount64() }.saturating_sub(shown_at) < 400 {
        return;
    }
    hide_button(app);
}

pub fn hide_button(app: &AppHandle) {
            let visible = BUTTON_RECT.lock().map(|r| r.is_some()).unwrap_or(false);
    if !visible {
        return;
    }

    if let Some(win) = app.get_webview_window(SELECTION_WINDOW) {
        let _ = win.hide();
    }
    if let Ok(mut rect) = BUTTON_RECT.lock() {
        *rect = None;
    }
    if let Ok(mut point) = SELECTION_POINT.lock() {
        *point = None;
    }
    BUTTON_SHOWN_AT.store(0, Ordering::Relaxed);
}

#[allow(dead_code)]
pub fn take_last_text() -> String {
    LAST_TEXT
        .lock()
        .map(|mut guard| std::mem::take(&mut *guard))
        .unwrap_or_default()
}

pub fn peek_last_text() -> String {
    LAST_TEXT
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

pub fn make_noactivate(window: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    if let Ok(hwnd) = window.hwnd() {
        let handle = HWND(hwnd.0 as *mut std::ffi::c_void);
        unsafe {
            let style = GetWindowLongPtrW(handle, GWL_EXSTYLE);
            SetWindowLongPtrW(
                handle,
                GWL_EXSTYLE,
                style | (WS_EX_NOACTIVATE.0 as isize) | (WS_EX_TOOLWINDOW.0 as isize),
            );
        }
    }
}
